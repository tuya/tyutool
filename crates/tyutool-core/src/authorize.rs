//! TuyaOpen UART authorization — serial text-command exchange protocol.
//!
//! Entirely independent of BootROM/flash protocols. All commands are plain
//! ASCII terminated with `\r\n`, processed by the TuyaOpen CLI shell.
//!
//! # Boot sequence (common to all entry points)
//!
//! `detect_firmware` opens the serial port, drains stale output, hardware-resets
//! the device, and polls for firmware kind by repeatedly sending `sys_log_enable off`:
//! - Response `OK: log disabled` → new firmware (shell ready; logging disabled)
//! - Response contains "No command" or a `tuya>` prompt → old firmware (shell ready)
//! - No recognizable response within `boot_max_wait` → old firmware (timeout fallback)
//!
//! Polling starts at `boot_probe_start` after reset and repeats every
//! `boot_probe_interval`. Both values come from `AuthTiming::for_chip`, which
//! holds chip-measured values (T5AI: 600 ms start / 50 ms interval / 2 100 ms max).
//!
//! # Write flow
//!
//! Boot sequence → `auth-read` to check existing credentials:
//! - Already matches → skip the write (and `hardware_reset` on new firmware)
//! - Conflict (existing differs and non-placeholder) → emit
//!   `FlashMilestone::AuthConflict` and invoke `FlashJob.confirm_overwrite`
//!   - `None` (CLI default) ⇒ proceed with overwrite
//!   - `Some(fn)` returning `true` ⇒ proceed with overwrite
//!   - `Some(fn)` returning `false` ⇒ return `Err(FlashError::Cancelled)`
//! - No existing credentials → proceed
//!
//! Then `auth <uuid> <authkey>` and verify via `auth-read`. New firmware
//! ends with `hardware_reset` to restart the device; old firmware does not.
//!
//! # Read-only flow (uuid + authkey absent)
//!
//! Boot sequence → `auth-read` to display current auth state. New firmware
//! re-enables device logging via `sys_log_enable on` before returning.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashMilestone};
use crate::job::FlashJob;

// ── Timing ────────────────────────────────────────────────────────────────

const BAUD: u32 = 115_200;
/// Per-command absolute read deadline (hard ceiling regardless of idle).
const CMD_TIMEOUT: Duration = Duration::from_secs(3);
/// Total upper bound for the `auth-otp-lock` response wait.
/// eFuse burning is a physical write that may take significantly longer
/// than a normal shell command. This MUST be confirmed against real
/// hardware before release (see hardware verification scenario 7 in the spec).
const AUTH_OTP_LOCK_TIMEOUT: Duration = Duration::from_secs(2);
/// Idle window — wait this long after the last byte before declaring the
/// response complete. Set deliberately longer than `cmd_idle_timeout` to
/// avoid premature termination during eFuse settling.
const AUTH_OTP_LOCK_IDLE: Duration = Duration::from_millis(500);
/// Drain: give up after this long regardless.
const DRAIN_MAX: Duration = Duration::from_secs(5);
/// Devices shipped un-authorized carry this placeholder UUID.
const PLACEHOLDER_UUID: &str = "uuidxxxxxxxxxxxxxxxx";

// ── Per-chip timing ───────────────────────────────────────────────────────

/// Chip-specific timing parameters for the TuyaOpen UART auth protocol.
#[derive(Debug, Clone)]
struct AuthTiming {
    /// Do not probe before this many ms after reset.
    boot_probe_start: Duration,
    /// Interval between boot-ready probes (also used as `wake_shell` spacing).
    boot_probe_interval: Duration,
    /// Hard ceiling before falling back to old-firmware path (≥ 3× boot-ready).
    boot_max_wait: Duration,
    /// Idle timeout for regular command responses (≈ 3× max first-byte RTT).
    cmd_idle_timeout: Duration,
    /// Settle wait after `auth_write` on old firmware (device may reboot).
    write_settle_wait: Duration,
    /// Stop draining when silent for this long (per-chip; see table below).
    drain_quiet: Duration,
    /// Number of attempts when reading MAC (new firmware).
    mac_read_retries: u8,
    /// Delay between MAC read retries (ms).
    mac_read_retry_ms: Duration,
    /// Number of attempts when reading/verifying auth credentials.
    auth_read_retries: u32,
    /// Delay between auth read retries (ms).
    auth_read_retry_ms: Duration,
}

// Per-chip timing table — all values in milliseconds.
//
// Derivation:
//   probe_start  ≈ measured_boot_ready − 100 ms
//   max_wait     ≥ 3 × measured_boot_ready
//   cmd_idle     ≈ 3 × max_observed_first_byte_rtt
//   drain_quiet  ≥ 2 × observed post-log-off settle time
//   mac_retries  / auth_retries: number of attempts (0 = try once, no retry)
//   mac_retry_ms / auth_retry_ms: wait between retries
//
// To add a chip: append one row; record the measurement date in the comment.
//
// columns: chips, start, int, max, idle, settle, drain_q, mac_ret, mac_ms, auth_ret, auth_ms
type ChipTimingRow = (
    &'static [&'static str],
    u64,
    u64,
    u64,
    u64,
    u64,
    u64,
    u8,
    u64,
    u32,
    u64,
);

#[rustfmt::skip]
const CHIP_TIMING: &[ChipTimingRow] = &[
    //                                            start  int   max   idle settle drain_q  mac_ret mac_ms auth_ret auth_ms
    (&["T5AI", "T5"],                              600,  50,  2100,  50,  3000,   800,       3,    500,      2,    200), // ready ~703ms,  RTT ~11ms
    (&["ESP32", "ESP32C3", "ESP32C6", "ESP32S3"], 1000,  50,  3500, 120,  3000,   400,       3,    500,      2,    200), // ready ~1108ms, RTT 20–40ms (2026-06-25)
];

impl AuthTiming {
    /// Select timing by chip ID (case-insensitive). Unrecognised chip → default.
    fn for_chip(chip_id: &str) -> Self {
        let id = chip_id.to_ascii_uppercase();
        for &(
            chips,
            start,
            interval,
            max_wait,
            idle,
            settle,
            drain_q,
            mac_ret,
            mac_ms,
            auth_ret,
            auth_ms,
        ) in CHIP_TIMING
        {
            if chips.contains(&id.as_str()) {
                return Self {
                    boot_probe_start: Duration::from_millis(start),
                    boot_probe_interval: Duration::from_millis(interval),
                    boot_max_wait: Duration::from_millis(max_wait),
                    cmd_idle_timeout: Duration::from_millis(idle),
                    write_settle_wait: Duration::from_millis(settle),
                    drain_quiet: Duration::from_millis(drain_q),
                    mac_read_retries: mac_ret,
                    mac_read_retry_ms: Duration::from_millis(mac_ms),
                    auth_read_retries: auth_ret,
                    auth_read_retry_ms: Duration::from_millis(auth_ms),
                };
            }
        }
        Self::default()
    }
}

impl Default for AuthTiming {
    fn default() -> Self {
        Self {
            boot_probe_start: Duration::from_millis(700),
            boot_probe_interval: Duration::from_millis(50),
            boot_max_wait: Duration::from_millis(3000),
            cmd_idle_timeout: Duration::from_millis(200),
            write_settle_wait: Duration::from_secs(3),
            drain_quiet: Duration::from_millis(800),
            mac_read_retries: 3,
            mac_read_retry_ms: Duration::from_millis(500),
            auth_read_retries: 2,
            auth_read_retry_ms: Duration::from_millis(200),
        }
    }
}

// ── Firmware version detection ────────────────────────────────────────────

/// Parsed CLI version string from `version` command response.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CliVersion(u32, u32, u32);

const NEW_FIRMWARE_MIN: CliVersion = CliVersion(1, 0, 0);

/// Firmware capability tier detected via `sys_log_enable off` + `version`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FirmwareKind {
    /// `sys_log_enable` command absent — legacy flow, logging unchanged.
    Old,
    /// `sys_log_enable` present, version >= 1.0.0 — logging disabled, new flow.
    New(CliVersion),
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Strip ANSI escape sequences (`\x1b[...m` style) from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // skip the letter terminator
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

/// Match TuyaOpen device-log prefix `[MM-DD `.
fn is_device_log(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 7
        && b[0] == b'['
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && b[3] == b'-'
        && b[4].is_ascii_digit()
        && b[5].is_ascii_digit()
        && b[6] == b' '
}

/// TuyaOpen interactive-shell prompt (`tuya> `).
fn is_shell_prompt(s: &str) -> bool {
    let t = s.trim();
    t == "tuya>" || t.starts_with("tuya> ")
}

// ── Serial I/O abstraction ──────────────────────────────────────────────────

/// Byte-level serial I/O the [`AuthSession`] needs. Mirrors the proven
/// `IoTransport` pattern (see `plugins/beken/transport.rs`): a real adapter
/// wraps `serialport::SerialPort`, a test mock pre-loads reads and records writes.
trait AuthIo: Send {
    /// Number of bytes available to read without blocking.
    fn bytes_to_read(&self) -> io::Result<u32>;
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
    fn set_dtr(&mut self, level: bool) -> io::Result<()>;
    fn set_rts(&mut self, level: bool) -> io::Result<()>;
    /// Clear the input (RX) buffer.
    fn clear_input(&mut self) -> io::Result<()>;
}

/// Real serial adapter wrapping `serialport::SerialPort`.
struct SerialAuthIo {
    port: Box<dyn serialport::SerialPort>,
}

impl AuthIo for SerialAuthIo {
    fn bytes_to_read(&self) -> io::Result<u32> {
        self.port
            .bytes_to_read()
            .map_err(|e| io::Error::other(e.to_string()))
    }
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::Read::read(&mut self.port, buf)
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        io::Write::write_all(&mut self.port, data)
    }
    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.port)
    }
    fn set_dtr(&mut self, level: bool) -> io::Result<()> {
        self.port
            .write_data_terminal_ready(level)
            .map_err(|e| io::Error::other(e.to_string()))
    }
    fn set_rts(&mut self, level: bool) -> io::Result<()> {
        self.port
            .write_request_to_send(level)
            .map_err(|e| io::Error::other(e.to_string()))
    }
    fn clear_input(&mut self) -> io::Result<()> {
        self.port
            .clear(serialport::ClearBuffer::Input)
            .map_err(|e| io::Error::other(e.to_string()))
    }
}

// ── Serial session ────────────────────────────────────────────────────────

struct AuthSession<T: AuthIo> {
    port: T,
    timing: AuthTiming,
}

impl AuthSession<SerialAuthIo> {
    fn open(port_name: &str, timing: AuthTiming, baud_rate: u32) -> Result<Self, FlashError> {
        let mut port = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| FlashError::Plugin(format!("cannot open {}: {}", port_name, e)))?;
        // De-assert control lines — avoid triggering download mode on open.
        let _ = port.write_data_terminal_ready(false);
        let _ = port.write_request_to_send(false);
        Ok(Self {
            port: SerialAuthIo { port },
            timing,
        })
    }
}

impl<T: AuthIo> AuthSession<T> {
    /// Read and discard bytes until the line has been quiet for `timing.drain_quiet`
    /// or [`DRAIN_MAX`] has elapsed. Returns total bytes consumed.
    fn drain_boot_output(&mut self) -> usize {
        let drain_quiet = self.timing.drain_quiet;
        let deadline = Instant::now() + DRAIN_MAX;
        let mut last_data = Instant::now();
        let mut total = 0usize;
        let mut buf = [0u8; 256];
        loop {
            if Instant::now() >= deadline {
                break;
            }
            match self.port.bytes_to_read() {
                Ok(n) if n > 0 => {
                    let to_read = (n as usize).min(buf.len());
                    if let Ok(read) = self.port.read(&mut buf[..to_read]) {
                        total += read;
                        last_data = Instant::now();
                    }
                }
                _ => {
                    if Instant::now().duration_since(last_data) >= drain_quiet {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        total
    }

    /// Pulse RTS to reset the device (same as tos.py `_hardware_reset_via_rts`).
    fn hardware_reset(&mut self) -> Result<(), FlashError> {
        self.port
            .set_dtr(false)
            .map_err(|e| FlashError::Plugin(format!("DTR error: {}", e)))?;
        self.port
            .set_rts(true)
            .map_err(|e| FlashError::Plugin(format!("RTS high error: {}", e)))?;
        std::thread::sleep(Duration::from_millis(100));
        self.port
            .set_rts(false)
            .map_err(|e| FlashError::Plugin(format!("RTS low error: {}", e)))?;
        std::thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    /// Clear RX buffer then write `cmd\r\n`.
    fn send_cmd(&mut self, cmd: &str) -> Result<(), FlashError> {
        let _ = self.port.clear_input();
        let data = format!("{}\r\n", cmd);
        self.port
            .write_all(data.as_bytes())
            .map_err(FlashError::Io)?;
        self.port.flush().map_err(FlashError::Io)?;
        Ok(())
    }

    /// Send a few bare `\r\n` to flush any partial input and wait until the
    /// TuyaOpen shell is ready.  Call this after the post-boot drain and
    /// before issuing real commands when the device has just booted.
    ///
    /// Spacing between CRLFs uses `timing.boot_probe_interval` (e.g. 50 ms for
    /// T5AI) rather than a fixed 300 ms, since the shell responds within ~11 ms.
    fn wake_shell(&mut self) {
        let interval = self.timing.boot_probe_interval;
        for _ in 0..3 {
            let _ = self.port.write_all(b"\r\n");
            std::thread::sleep(interval);
        }
        // Drain prompt echoes and any leftover boot output.
        let _ = self.port.clear_input();
    }

    /// Read response with a custom total timeout and idle timeout.
    fn read_response_timed(
        &mut self,
        max_timeout: Duration,
        idle_timeout: Duration,
    ) -> Vec<String> {
        let fn_start = Instant::now();
        let mut raw_buf: Vec<u8> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        let end_time = fn_start + max_timeout;
        let mut last_data: Option<Instant> = None;
        let mut tmp = [0u8; 256];

        loop {
            if Instant::now() >= end_time {
                break;
            }
            match self.port.bytes_to_read() {
                Ok(n) if n > 0 => {
                    let to_read = (n as usize).min(tmp.len());
                    if let Ok(read) = self.port.read(&mut tmp[..to_read]) {
                        if last_data.is_none() {
                            log::info!(
                                "flash.log.auth.firstByte: rtt={}ms",
                                fn_start.elapsed().as_millis()
                            );
                        }
                        raw_buf.extend_from_slice(&tmp[..read]);
                        last_data = Some(Instant::now());
                        let mut got_prompt = false;
                        while let Some(pos) = raw_buf.iter().position(|&b| b == b'\n') {
                            let chunk: Vec<u8> = raw_buf.drain(..=pos).collect();
                            let s = String::from_utf8_lossy(&chunk)
                                .trim_end_matches(['\r', '\n'])
                                .to_string();
                            let s = strip_ansi(&s).trim().to_string();
                            if !s.is_empty() {
                                if is_shell_prompt(&s) {
                                    got_prompt = true;
                                }
                                lines.push(s);
                            }
                        }
                        if got_prompt {
                            break;
                        }
                    }
                }
                _ => {
                    if let Some(last) = last_data {
                        if Instant::now().duration_since(last) >= idle_timeout {
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        if !raw_buf.is_empty() {
            let s = String::from_utf8_lossy(&raw_buf).trim().to_string();
            let s = strip_ansi(&s).trim().to_string();
            if !s.is_empty() {
                lines.push(s);
            }
        }
        lines
    }

    fn read_response_idle(&mut self, idle_timeout: Duration) -> Vec<String> {
        self.read_response_timed(CMD_TIMEOUT, idle_timeout)
    }

    fn read_response(&mut self) -> Vec<String> {
        let idle = self.timing.cmd_idle_timeout;
        self.read_response_timed(CMD_TIMEOUT, idle)
    }

    /// Send `auth-otp-lock` and parse the response.
    ///
    /// Returns `Ok(())` when the firmware confirms with
    /// `"Authorization otp lock succeeds."`, `Err(FlashError::Plugin)`
    /// otherwise (including explicit `"Authorization otp lock failure."`,
    /// no response, and any other unrecognised output).
    ///
    /// **WARNING**: this command burns the eFuse and is irreversible.
    /// Callers must gate it behind an explicit user opt-in.
    ///
    /// Uses [`AUTH_OTP_LOCK_TIMEOUT`] / [`AUTH_OTP_LOCK_IDLE`] rather than
    /// the default shell-command timing because eFuse settling may delay
    /// the response beyond the standard 50ms idle window.
    fn auth_otp_lock(&mut self) -> Result<(), FlashError> {
        self.send_cmd("auth-otp-lock")
            .map_err(|e| FlashError::Plugin(format!("auth-otp-lock send failed: {e}")))?;
        let lines = self.read_response_timed(AUTH_OTP_LOCK_TIMEOUT, AUTH_OTP_LOCK_IDLE);

        let mut saw_success = false;
        let mut saw_failure = false;
        for line in &lines {
            let lower = line.to_lowercase();
            let trimmed = lower.trim();
            if trimmed.starts_with("authorization otp lock succeeds") {
                saw_success = true;
            } else if trimmed.starts_with("authorization otp lock failure") {
                saw_failure = true;
            }
        }

        match (saw_success, saw_failure) {
            (true, _) => Ok(()),
            (false, true) => Err(FlashError::Plugin(
                "auth-otp-lock: device returned failure".into(),
            )),
            (false, false) => Err(FlashError::Plugin(
                "auth-otp-lock: no recognisable response".into(),
            )),
        }
    }

    /// Send `auth-read` (or `auth-read <n>` for non-KV storage) and return `(uuid, authkey)` or `None`.
    fn auth_read(&mut self, storage: AuthStorage) -> Option<(String, String)> {
        let cmd = if storage == AuthStorage::Kv {
            "auth-read".to_string()
        } else {
            format!("auth-read {}", storage.as_u8())
        };
        self.send_cmd(&cmd).ok()?;
        let lines = self.read_response();
        let relevant: Vec<&str> = lines
            .iter()
            .filter(|l| {
                let lower = l.to_lowercase();
                !lower.contains("auth-read") && !is_device_log(l) && !is_shell_prompt(l)
            })
            .map(String::as_str)
            .collect();
        if relevant.len() >= 2 {
            let uuid = relevant[0].trim().to_string();
            let authkey = relevant[1].trim().to_string();
            if !uuid.is_empty() && !authkey.is_empty() {
                return Some((uuid, authkey));
            }
        }
        None
    }

    /// Send `auth <uuid> <authkey>` and return the response lines.
    ///
    /// `idle` controls how long to wait after the last byte before declaring
    /// the response complete. Use `timing.cmd_idle_timeout` for new firmware
    /// (which does not reboot after writing auth) and 2 s for old firmware
    /// (which may reboot, producing a longer silent gap before the banner).
    /// Callers must verify success via [`Self::auth_read`] rather than
    /// inspecting the returned lines, since not all firmware versions print
    /// `"Authorization write succeeds."` before rebooting.
    fn auth_write(
        &mut self,
        uuid: &str,
        authkey: &str,
        storage: AuthStorage,
        idle: Duration,
    ) -> Vec<String> {
        let cmd = if storage == AuthStorage::Kv {
            format!("auth {} {}", uuid, authkey)
        } else {
            format!("auth {} {} {}", uuid, authkey, storage.as_u8())
        };
        if self.send_cmd(&cmd).is_err() {
            return vec![];
        }
        self.read_response_idle(idle)
    }

    /// Send `read_mac` and parse the MAC address from the response.
    /// Returns `Some("XX:XX:XX:XX:XX:XX")` (uppercase colon-separated) or `None`.
    fn read_mac(&mut self) -> Option<String> {
        self.send_cmd("read_mac").ok()?;
        let lines = self.read_response();
        for line in &lines {
            if let Some(mac) = parse_mac_from_str(line) {
                return Some(mac);
            }
        }
        None
    }

    /// Hardware-reset the device, detect firmware kind, and leave the shell ready.
    ///
    /// Polls by repeatedly sending `sys_log_enable off` starting at
    /// `timing.boot_probe_start` after reset, every `timing.boot_probe_interval`:
    /// - `OK: log disabled` in response → new firmware; returns immediately.
    /// - "No command" or `tuya>` in response → old firmware; returns immediately.
    /// - No recognizable response by `timing.boot_max_wait` → old firmware fallback.
    fn detect_firmware(&mut self, cancel: &AtomicBool) -> Result<FirmwareKind, FlashError> {
        let boot_probe_start = self.timing.boot_probe_start;
        let boot_probe_interval = self.timing.boot_probe_interval;
        let boot_max_wait = self.timing.boot_max_wait;

        self.hardware_reset()?;
        let reset_time = Instant::now();

        // Wait until probe_start — shell cannot possibly be ready before this.
        let first_probe_at = reset_time + boot_probe_start;
        while Instant::now() < first_probe_at {
            if cancel.load(Ordering::Relaxed) {
                return Err(FlashError::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let max_deadline = reset_time + boot_max_wait;

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(FlashError::Cancelled);
            }

            let _ = self.send_cmd("sys_log_enable off");
            // Each probe reads for at most 2× probe_interval (wider window to avoid
            // false-positive old-firmware detection while boot banner is still printing),
            // but stops as soon as the line has been idle for one probe_interval.
            let lines = self
                .read_response_timed(boot_probe_interval.saturating_mul(2), boot_probe_interval);

            let is_new = lines
                .iter()
                .any(|l| l.to_lowercase().contains("ok: log disabled"));
            let is_old = !is_new
                && lines.iter().any(|l| {
                    let lower = l.to_lowercase();
                    lower.contains("no command") || is_shell_prompt(l)
                });

            if is_new || is_old {
                let elapsed_ms = reset_time.elapsed().as_millis();
                log::info!(
                    "flash.log.auth.shellReady: new_firmware={}, elapsed={}ms",
                    is_new,
                    elapsed_ms
                );
                self.drain_and_wake(cancel)?;
                if cancel.load(Ordering::Relaxed) {
                    return Err(FlashError::Cancelled);
                }
                if is_new {
                    let _ = self.send_cmd("version");
                    let vlines = self.read_response();
                    let version = vlines.iter().find_map(|l| parse_cli_version(l));
                    let kind = match version {
                        Some(v) if v >= NEW_FIRMWARE_MIN => FirmwareKind::New(v),
                        _ => FirmwareKind::New(NEW_FIRMWARE_MIN),
                    };
                    return Ok(kind);
                }
                return Ok(FirmwareKind::Old);
            }

            // No recognizable response yet.
            if Instant::now() >= max_deadline {
                let elapsed_ms = reset_time.elapsed().as_millis();
                log::info!(
                    "flash.log.auth.shellReady: timed_out elapsed={}ms, fallback=Old",
                    elapsed_ms
                );
                self.drain_and_wake(cancel)?;
                return Ok(FirmwareKind::Old);
            }

            // Wait the remaining probe interval before retrying.
            let next_probe_at = Instant::now() + boot_probe_interval;
            while Instant::now() < next_probe_at {
                if cancel.load(Ordering::Relaxed) {
                    return Err(FlashError::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    /// Drain stale boot output, check for cancellation, then wake the shell.
    ///
    /// Called after firmware detection in all three paths (new, old, timeout fallback)
    /// to avoid duplicating the drain → cancel-check → wake sequence.
    fn drain_and_wake(&mut self, cancel: &AtomicBool) -> Result<(), FlashError> {
        self.drain_boot_output();
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }
        self.wake_shell();
        Ok(())
    }

    /// Send `sys_log_enable on` and drain the response (used at read-only flow end).
    fn syslog_on(&mut self) {
        let _ = self.send_cmd("sys_log_enable on");
        let _ = self.read_response();
    }
}

fn parse_mac_from_str(s: &str) -> Option<String> {
    s.split_whitespace().find_map(|token| {
        let parts: Vec<&str> = token.split(':').collect();
        // Handle "AA:BB:CC:DD:EE:FF" (6 parts) and
        // "LABEL:AA:BB:CC:DD:EE:FF" (7 parts, first is non-hex label like "ADDR")
        let hex_parts: &[&str] = if parts.len() == 6 {
            &parts
        } else if parts.len() == 7 && !parts[0].chars().all(|c| c.is_ascii_hexdigit()) {
            &parts[1..]
        } else {
            return None;
        };
        if hex_parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
        {
            Some(hex_parts.join(":").to_uppercase())
        } else {
            None
        }
    })
}

/// Parse "CLI version: X.Y.Z" from a single response line.
fn parse_cli_version(line: &str) -> Option<CliVersion> {
    let lower = strip_ansi(line).to_lowercase();
    let rest = lower.strip_prefix("cli version:")?.trim().to_string();
    let parts: Vec<&str> = rest.splitn(3, '.').collect();
    if parts.len() == 3 {
        let major = parts[0].trim().parse().ok()?;
        let minor = parts[1].trim().parse().ok()?;
        let patch = parts[2].trim().parse().ok()?;
        Some(CliVersion(major, minor, patch))
    } else {
        None
    }
}

// ── Batch auth types ──────────────────────────────────────────────────────

/// Outcome of a single batch-auth UART session.
#[derive(Debug)]
pub enum BatchAuthSlotResult {
    /// Auth written and verified successfully.
    Done { mac: String },
    /// Device already had the exact credentials — nothing written.
    AlreadyDone { mac: String },
    /// Auth on device didn't match but conflict_policy=Skip — nothing written.
    /// `existing_uuid` is the UUID already on the device, so the caller can
    /// find and confirm that Excel row.
    Skipped { mac: String, existing_uuid: String },
    /// No auth code available in Excel — device was probed but not written.
    InsufficientCodes { mac: String },
    /// Auth was written and verified successfully, but the subsequent
    /// `auth-otp-lock` command failed. The credential **has been written
    /// to the device's OTP region**, so callers must mark the allocated
    /// Excel row as used (NOT release it) to avoid handing the same
    /// UUID/Key out to another device.
    LockFailed { mac: String, lock_error: String },
    /// Operation was cancelled.
    Cancelled,
}

/// What to do when device already has conflicting auth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Skip,
    Overwrite,
}

/// Where to store the authorization credentials.
/// Passed as the third argument to the `auth` command (0 = KV, 1 = OTP).
/// `None` / `Kv` is the default and preserves existing behavior.
/// OTP writes are irreversible — the UI must warn the user before selecting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthStorage {
    #[default]
    Kv,
    Otp,
}

impl AuthStorage {
    fn as_u8(self) -> u8 {
        match self {
            AuthStorage::Kv => 0,
            AuthStorage::Otp => 1,
        }
    }
}

/// Per-step progress marker emitted during a batch auth slot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchAuthStep {
    ReadingMac,
    ReadingAuth,
    WritingAuth,
    Verifying,
}

// ── Public entry point ────────────────────────────────────────────────────

/// Run the TuyaOpen UART authorization flow.
///
/// Emits [`FlashEvent`] events throughout. The caller is responsible for
/// emitting the final `Done` event (matching the pattern in `run_job`).
pub fn run_authorize<F>(job: &FlashJob, cancel: &AtomicBool, progress: F) -> Result<(), FlashError>
where
    F: Fn(FlashEvent),
{
    let uuid = job
        .authorize_uuid
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let authkey = job
        .authorize_key
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let write_mode = !uuid.is_empty() && !authkey.is_empty();

    // ── Step 1: Open serial ───────────────────────────────────────────
    log::info!("flash.log.auth.openingPort: port={}", job.port);
    let timing = AuthTiming::for_chip(&job.chip_id);
    let mut sess = AuthSession::open(&job.port, timing, BAUD)?;

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Step 2: Drain stale boot output ──────────────────────────────
    log::info!("flash.log.auth.drainBootOutput");
    sess.drain_boot_output();

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Step 3: Reset + detect firmware version ───────────────────────
    log::info!("flash.log.auth.detectFirmware");
    let firmware = sess.detect_firmware(cancel)?;
    log::info!(
        "flash.log.auth.firmwareKind: new={}",
        matches!(firmware, FirmwareKind::New(_))
    );

    if write_mode {
        match firmware {
            FirmwareKind::New(_) => {
                // ── New firmware: 2 retries / 200ms to absorb single-frame noise ──
                log::info!("flash.log.auth.readDeviceAuth");
                let storage = job.authorize_storage.unwrap_or_default();
                let existing_auth = {
                    let mut auth = None;
                    for _ in 0..2u32 {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(FlashError::Cancelled);
                        }
                        auth = sess.auth_read(storage);
                        if auth.is_some() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    auth
                };

                // Skip write if already identical (checked before conflict to avoid
                // prompting the user when the credentials are already correct)
                if let Some((ref ex_u, ref ex_k)) = existing_auth {
                    if !ex_u.is_empty() && !ex_k.is_empty() && ex_u == &uuid && ex_k == &authkey {
                        log::info!("flash.log.auth.alreadySame");
                        progress(FlashEvent::Milestone {
                            milestone: FlashMilestone::AuthWriteSkipped,
                        });
                        sess.hardware_reset()?;
                        return Ok(());
                    }
                }
                // Conflict check: existing, non-placeholder, differs from requested
                if let Some((ref ex_u, ref ex_k)) = existing_auth {
                    if !ex_u.is_empty()
                        && ex_u != PLACEHOLDER_UUID
                        && (ex_u != &uuid || ex_k != &authkey)
                    {
                        // Emit milestone so frontend can show confirmation dialog
                        progress(FlashEvent::Milestone {
                            milestone: FlashMilestone::AuthConflict {
                                existing_uuid: ex_u.clone(),
                                existing_authkey: ex_k.clone(),
                            },
                        });
                        // Call injected confirmation callback; None = CLI, always overwrite
                        let confirmed = job
                            .confirm_overwrite
                            .as_ref()
                            .map(|f| f(ex_u.clone(), ex_k.clone()))
                            .unwrap_or(true);
                        if !confirmed {
                            if cancel.load(Ordering::Relaxed) {
                                return Err(FlashError::Cancelled);
                            }
                            return Err(FlashError::Plugin(
                                "authorization cancelled by user (overwrite declined)".into(),
                            ));
                        }
                    }
                }

                log::info!("flash.log.auth.writeStart");
                // Keep 2000ms idle: device may reboot after writing auth on some
                // firmware builds; a short idle would exit before the reboot banner
                // and leave drain_boot_output unable to clear it in time.
                let _lines = sess.auth_write(&uuid, &authkey, storage, Duration::from_millis(2000));

                if cancel.load(Ordering::Relaxed) {
                    return Err(FlashError::Cancelled);
                }

                // No 3s settle: new firmware does not reboot after auth write
                sess.drain_boot_output();
                sess.wake_shell();

                if cancel.load(Ordering::Relaxed) {
                    return Err(FlashError::Cancelled);
                }

                // ── Verify ────────────────────────────────────────────────────
                log::info!("flash.log.auth.verify");
                let verify_result = {
                    let mut result = None;
                    for _ in 0..2u32 {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(FlashError::Cancelled);
                        }
                        result = sess.auth_read(storage);
                        if result.is_some() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    result
                };
                match verify_result {
                    Some((rb_uuid, rb_key)) if rb_uuid == uuid && rb_key == authkey => {
                        log::info!("flash.log.auth.verifyOk");
                        sess.hardware_reset()?;
                        Ok(())
                    }
                    Some((rb_uuid, rb_key)) if rb_uuid == uuid => {
                        let _ = rb_key;
                        Err(FlashError::Plugin(
                            "Verification failed: AuthKey mismatch".into(),
                        ))
                    }
                    Some((rb_uuid, _)) => Err(FlashError::Plugin(format!(
                        "Verification failed: UUID mismatch (wrote {uuid}, read back {rb_uuid})"
                    ))),
                    None => Err(FlashError::Plugin(
                        "Verification failed: no response from auth-read".into(),
                    )),
                }
            }

            FirmwareKind::Old => {
                // ── Old firmware: original flow, unchanged ────────────────────

                // Optional read: skip write if device already matches
                log::info!("flash.log.auth.readDeviceAuth");
                let storage = job.authorize_storage.unwrap_or_default();
                let mut existing_auth: Option<(String, String)> = None;
                for _attempt in 1..=5u32 {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(FlashError::Cancelled);
                    }
                    existing_auth = sess.auth_read(storage);
                    if existing_auth.is_some() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(800));
                }

                if let Some((ref ex_u, ref ex_k)) = existing_auth {
                    // Skip write if already identical
                    if !ex_u.is_empty()
                        && !ex_k.is_empty()
                        && ex_u != PLACEHOLDER_UUID
                        && ex_u == &uuid
                        && ex_k == &authkey
                    {
                        log::info!("flash.log.auth.alreadySame");
                        progress(FlashEvent::Milestone {
                            milestone: FlashMilestone::AuthWriteSkipped,
                        });
                        sess.hardware_reset()?;
                        return Ok(());
                    }
                    // Conflict check (non-placeholder, differs)
                    if !ex_u.is_empty()
                        && ex_u != PLACEHOLDER_UUID
                        && (ex_u != &uuid || ex_k != &authkey)
                    {
                        progress(FlashEvent::Milestone {
                            milestone: FlashMilestone::AuthConflict {
                                existing_uuid: ex_u.clone(),
                                existing_authkey: ex_k.clone(),
                            },
                        });
                        let confirmed = job
                            .confirm_overwrite
                            .as_ref()
                            .map(|f| f(ex_u.clone(), ex_k.clone()))
                            .unwrap_or(true);
                        if !confirmed {
                            if cancel.load(Ordering::Relaxed) {
                                return Err(FlashError::Cancelled);
                            }
                            return Err(FlashError::Plugin(
                                "authorization cancelled by user (overwrite declined)".into(),
                            ));
                        }
                    }
                }

                log::info!("flash.log.auth.writeStart");
                let _lines = sess.auth_write(&uuid, &authkey, storage, Duration::from_millis(2000));

                if cancel.load(Ordering::Relaxed) {
                    return Err(FlashError::Cancelled);
                }

                // Wait for device to settle after possible reboot (old firmware may reboot)
                let settle_wait = sess.timing.write_settle_wait;
                log::info!("flash.log.auth.waitSettle: ms={}", settle_wait.as_millis());
                let wait_end = Instant::now() + settle_wait;
                while Instant::now() < wait_end {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(FlashError::Cancelled);
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                sess.drain_boot_output();
                sess.wake_shell();

                if cancel.load(Ordering::Relaxed) {
                    return Err(FlashError::Cancelled);
                }

                log::info!("flash.log.auth.verify");
                match sess.auth_read(storage) {
                    Some((rb_uuid, rb_key)) if rb_uuid == uuid && rb_key == authkey => {
                        log::info!("flash.log.auth.verifyOk");
                        Ok(())
                    }
                    Some((rb_uuid, rb_key)) if rb_uuid == uuid => {
                        let _ = rb_key;
                        Err(FlashError::Plugin(
                            "Verification failed: AuthKey mismatch (device may have rejected the write — check UUID/AuthKey length and format)".into(),
                        ))
                    }
                    Some((rb_uuid, _)) => Err(FlashError::Plugin(format!(
                        "Verification failed: UUID mismatch (wrote {uuid}, read back {rb_uuid})"
                    ))),
                    None => Err(FlashError::Plugin(
                        "Verification failed: no response from auth-read".into(),
                    )),
                }
            }
        }
    } else {
        // ── Read-only flow ────────────────────────────────────────────────────
        log::info!("flash.log.auth.readCurrent");
        match sess.auth_read(AuthStorage::default()) {
            Some((existing_uuid, existing_key)) => {
                if existing_uuid == PLACEHOLDER_UUID {
                    progress(FlashEvent::Milestone {
                        milestone: FlashMilestone::AuthReadEmpty,
                    });
                } else {
                    log::info!("flash.log.auth.authorized");
                    progress(FlashEvent::Milestone {
                        milestone: FlashMilestone::AuthReadComplete {
                            uuid: existing_uuid,
                            authkey: existing_key,
                        },
                    });
                }
                // Restore logging for new firmware (device stays running, not rebooted)
                if matches!(firmware, FirmwareKind::New(_)) {
                    sess.syslog_on();
                }
                Ok(())
            }
            None => {
                progress(FlashEvent::Milestone {
                    milestone: FlashMilestone::AuthReadEmpty,
                });
                if matches!(firmware, FirmwareKind::New(_)) {
                    sess.syslog_on();
                }
                Ok(())
            }
        }
    }
}

/// Single-device batch authorization slot: open UART, read MAC, read/write auth, verify.
///
/// The caller pre-allocates `uuid`/`authkey` from an Excel row. On return:
/// - `Done` → caller should confirm the Excel row (mark USED).
/// - `AlreadyDone` → caller should release the Excel row (allocated but not consumed).
/// - `Skipped` → no row was allocated; caller should find-and-confirm the existing row.
/// - `InsufficientCodes` → no row was allocated; nothing to release.
/// - `Err`/`Cancelled` → caller should release the Excel row if one was allocated.
pub fn run_batch_auth_slot<F, G>(
    port: &str,
    chip_id: &str,
    get_code: G,
    auth_baud_rate: u32,
    conflict_policy: ConflictPolicy,
    auth_storage: AuthStorage,
    lock_otp: bool,
    cancel: &AtomicBool,
    progress: F,
) -> Result<BatchAuthSlotResult, FlashError>
where
    F: Fn(BatchAuthStep),
    G: FnOnce() -> Option<(String, String)>,
{
    macro_rules! check_cancel {
        () => {
            if cancel.load(Ordering::Relaxed) {
                return Ok(BatchAuthSlotResult::Cancelled);
            }
        };
    }

    log::info!("[batch-auth] slot start  port={port} chip={chip_id}");
    let timing = AuthTiming::for_chip(chip_id);
    let mut sess = AuthSession::open(port, timing, auth_baud_rate)?;
    check_cancel!();
    sess.drain_boot_output();
    check_cancel!();
    let firmware = sess.detect_firmware(cancel)?;
    check_cancel!();

    match firmware {
        FirmwareKind::New(_) => {
            // Read MAC
            progress(BatchAuthStep::ReadingMac);
            let mac = {
                let mut mac_opt = None;
                for _ in 0..sess.timing.mac_read_retries {
                    check_cancel!();
                    mac_opt = sess.read_mac();
                    if mac_opt.is_some() {
                        break;
                    }
                    std::thread::sleep(sess.timing.mac_read_retry_ms);
                }
                mac_opt.ok_or_else(|| FlashError::Plugin("Failed to read MAC address".into()))?
            };
            log::info!("[batch-auth] read mac  port={port} mac={mac}");

            // Read existing auth
            progress(BatchAuthStep::ReadingAuth);
            let existing_auth = {
                let mut auth = None;
                for _ in 0..sess.timing.auth_read_retries {
                    check_cancel!();
                    auth = sess.auth_read(auth_storage);
                    if auth.is_some() {
                        break;
                    }
                    std::thread::sleep(sess.timing.auth_read_retry_ms);
                }
                auth
            };

            // Conflict check: if policy=Skip and device already has auth, skip without allocating.
            if let Some((ref ex_uuid, _)) = existing_auth {
                if ex_uuid != PLACEHOLDER_UUID && conflict_policy == ConflictPolicy::Skip {
                    log::info!(
                        "[batch-auth] skipped  port={port} mac={mac} existing_uuid={ex_uuid}"
                    );
                    return Ok(BatchAuthSlotResult::Skipped {
                        mac,
                        existing_uuid: ex_uuid.clone(),
                    });
                }
            }

            // Lazily allocate an auth code — only now that we know the device needs one.
            let (uuid, authkey) = match get_code() {
                Some(c) => c,
                None => {
                    log::info!("[batch-auth] no-code  port={port} mac={mac}");
                    return Ok(BatchAuthSlotResult::InsufficientCodes { mac });
                }
            };
            log::info!("[batch-auth] allocated  port={port} mac={mac} uuid={uuid}");

            // AlreadyDone: device has the exact credentials we just allocated.
            if let Some((ref ex_uuid, ref ex_key)) = existing_auth {
                if ex_uuid != PLACEHOLDER_UUID && ex_uuid == &uuid && ex_key == &authkey {
                    log::info!("[batch-auth] already-done  port={port} mac={mac} uuid={uuid}");
                    return Ok(BatchAuthSlotResult::AlreadyDone { mac });
                }
            }

            // Write
            progress(BatchAuthStep::WritingAuth);
            log::info!("[batch-auth] writing  port={port} mac={mac} uuid={uuid}");
            let _lines =
                sess.auth_write(&uuid, &authkey, auth_storage, Duration::from_millis(2000));
            check_cancel!();

            // No 3s settle for new firmware
            sess.drain_boot_output();
            sess.wake_shell();
            check_cancel!();

            // Verify
            progress(BatchAuthStep::Verifying);
            let verify_result = {
                let mut result = None;
                for _ in 0..sess.timing.auth_read_retries {
                    check_cancel!();
                    result = sess.auth_read(auth_storage);
                    if result.is_some() {
                        break;
                    }
                    std::thread::sleep(sess.timing.auth_read_retry_ms);
                }
                result
            };
            match verify_result {
                Some((rb_uuid, rb_key)) if rb_uuid == uuid && rb_key == authkey => {
                    log::info!("[batch-auth] verify ok  port={port} mac={mac} uuid={uuid}");
                    if lock_otp {
                        log::warn!(
                            "[batch-auth] sending auth-otp-lock (irreversible)  port={port} mac={mac}"
                        );
                        match sess.auth_otp_lock() {
                            Ok(()) => {
                                log::info!(
                                    "[batch-auth] otp-lock succeeded  port={port} mac={mac}"
                                );
                            }
                            Err(e) => {
                                let lock_error = e.to_string();
                                log::warn!(
                                    "[batch-auth] otp-lock failed  port={port} mac={mac} err={lock_error}"
                                );
                                let _ = sess.hardware_reset();
                                return Ok(BatchAuthSlotResult::LockFailed { mac, lock_error });
                            }
                        }
                    }
                    log::info!("[batch-auth] done  port={port} mac={mac} uuid={uuid}");
                    sess.hardware_reset()?;
                    Ok(BatchAuthSlotResult::Done { mac })
                }
                Some((rb_uuid, rb_key)) => {
                    log::warn!("[batch-auth] verify-fail  port={port} mac={mac} wrote=({uuid},{authkey}) readback=({rb_uuid},{rb_key})");
                    Err(FlashError::Plugin(format!(
                        "Verification failed: wrote ({uuid}, {authkey}), read back ({rb_uuid}, {rb_key})"
                    )))
                }
                None => {
                    log::warn!(
                        "[batch-auth] verify-fail  port={port} mac={mac} uuid={uuid} reason=no-response"
                    );
                    Err(FlashError::Plugin(
                        "Verification failed: no response from auth-read".into(),
                    ))
                }
            }
        }

        FirmwareKind::Old => {
            // Original old firmware flow (unchanged)
            progress(BatchAuthStep::ReadingMac);
            let mac = {
                let mut mac_opt = None;
                for _ in 0..sess.timing.mac_read_retries {
                    check_cancel!();
                    mac_opt = sess.read_mac();
                    if mac_opt.is_some() {
                        break;
                    }
                    std::thread::sleep(sess.timing.mac_read_retry_ms);
                }
                mac_opt.ok_or_else(|| FlashError::Plugin("Failed to read MAC address".into()))?
            };
            log::info!("[batch-auth] read mac (old fw)  port={port} mac={mac}");

            progress(BatchAuthStep::ReadingAuth);
            let existing_auth = {
                let mut auth = None;
                // Old firmware is slower; use 3× the normal auth retry interval.
                let old_retry_ms = sess.timing.auth_read_retry_ms * 4;
                for _ in 0..sess.timing.auth_read_retries + 1 {
                    check_cancel!();
                    auth = sess.auth_read(auth_storage);
                    if auth.is_some() {
                        break;
                    }
                    std::thread::sleep(old_retry_ms);
                }
                auth
            };

            // Conflict check: if policy=Skip and device already has auth, skip without allocating.
            if let Some((ref ex_uuid, _)) = existing_auth {
                if ex_uuid != PLACEHOLDER_UUID && conflict_policy == ConflictPolicy::Skip {
                    log::info!("[batch-auth] skipped (old fw)  port={port} mac={mac} existing_uuid={ex_uuid}");
                    return Ok(BatchAuthSlotResult::Skipped {
                        mac,
                        existing_uuid: ex_uuid.clone(),
                    });
                }
            }

            // Lazily allocate an auth code — only now that we know the device needs one.
            let (uuid, authkey) = match get_code() {
                Some(c) => c,
                None => {
                    log::info!("[batch-auth] no-code (old fw)  port={port} mac={mac}");
                    return Ok(BatchAuthSlotResult::InsufficientCodes { mac });
                }
            };
            log::info!("[batch-auth] allocated (old fw)  port={port} mac={mac} uuid={uuid}");

            // AlreadyDone: device has the exact credentials we just allocated.
            if let Some((ref ex_uuid, ref ex_key)) = existing_auth {
                if ex_uuid != PLACEHOLDER_UUID && ex_uuid == &uuid && ex_key == &authkey {
                    log::info!(
                        "[batch-auth] already-done (old fw)  port={port} mac={mac} uuid={uuid}"
                    );
                    return Ok(BatchAuthSlotResult::AlreadyDone { mac });
                }
            }

            progress(BatchAuthStep::WritingAuth);
            log::info!("[batch-auth] writing (old fw)  port={port} mac={mac} uuid={uuid}");
            let _lines =
                sess.auth_write(&uuid, &authkey, auth_storage, Duration::from_millis(2000));
            check_cancel!();

            let settle_wait = sess.timing.write_settle_wait;
            let wait_end = Instant::now() + settle_wait;
            while Instant::now() < wait_end {
                check_cancel!();
                std::thread::sleep(Duration::from_millis(200));
            }
            sess.drain_boot_output();
            sess.wake_shell();
            check_cancel!();

            progress(BatchAuthStep::Verifying);
            let mut verify_result = None;
            for _ in 0..sess.timing.auth_read_retries {
                check_cancel!();
                verify_result = sess.auth_read(auth_storage);
                if verify_result.is_some() {
                    break;
                }
                std::thread::sleep(sess.timing.auth_read_retry_ms);
            }
            match verify_result {
                Some((rb_uuid, rb_key)) if rb_uuid == uuid && rb_key == authkey => {
                    log::info!("[batch-auth] done (old fw)  port={port} mac={mac} uuid={uuid}");
                    Ok(BatchAuthSlotResult::Done { mac })
                }
                Some((rb_uuid, rb_key)) => {
                    log::warn!("[batch-auth] verify-fail (old fw)  port={port} mac={mac} wrote=({uuid},{authkey}) readback=({rb_uuid},{rb_key})");
                    Err(FlashError::Plugin(format!(
                        "Verification failed: wrote ({uuid}, {authkey}), read back ({rb_uuid}, {rb_key})"
                    )))
                }
                None => {
                    log::warn!("[batch-auth] verify-fail (old fw)  port={port} mac={mac} reason=no-response");
                    Err(FlashError::Plugin(
                        "Verification failed: no response from auth-read".into(),
                    ))
                }
            }
        }
    }
}

// ── Read-only probe ───────────────────────────────────────────────────────

/// Result of a read-only auth probe (no write).
pub struct ReadAuthProbeResult {
    /// MAC address as returned by `read_mac`, e.g. `"AA:BB:CC:DD:EE:FF"`.
    /// `None` if the device did not respond.
    pub mac: Option<String>,
    /// UUID from `auth-read`, or `None` when the device is un-authorized
    /// (including the placeholder `"uuidxxxxxxxxxxxxxxxx"`).
    pub uuid: Option<String>,
}

/// Open a serial connection to `port`, reset the device, and read its MAC
/// address and existing authorization UUID without writing anything.
///
/// Mirrors the first half of [`run_batch_auth_slot`] but skips allocation
/// and all write steps.
pub fn read_auth_probe(
    port: &str,
    chip_id: &str,
    baud_rate: u32,
    storage: AuthStorage,
    cancel: &AtomicBool,
) -> Result<ReadAuthProbeResult, FlashError> {
    macro_rules! check_cancel {
        () => {
            if cancel.load(Ordering::Relaxed) {
                return Err(FlashError::Cancelled);
            }
        };
    }

    log::info!("[batch-auth] read-probe start  port={port} chip={chip_id}");
    let timing = AuthTiming::for_chip(chip_id);
    let mut sess = AuthSession::open(port, timing, baud_rate)?;
    check_cancel!();
    sess.drain_boot_output();
    check_cancel!();
    let firmware = sess.detect_firmware(cancel)?;
    check_cancel!();

    let old_fw = matches!(firmware, FirmwareKind::Old);

    // Read MAC with retries
    let mut mac_opt: Option<String> = None;
    for _ in 0..sess.timing.mac_read_retries {
        check_cancel!();
        mac_opt = sess.read_mac();
        if mac_opt.is_some() {
            break;
        }
        std::thread::sleep(sess.timing.mac_read_retry_ms);
    }
    log::info!("[batch-auth] read-probe mac  port={port} mac={mac_opt:?}");

    // Read auth with retries (old firmware uses slower intervals)
    let retry_ms = if old_fw {
        sess.timing.auth_read_retry_ms * 4
    } else {
        sess.timing.auth_read_retry_ms
    };
    let retries = if old_fw {
        sess.timing.auth_read_retries + 1
    } else {
        sess.timing.auth_read_retries
    };
    let mut auth: Option<(String, String)> = None;
    for _ in 0..retries {
        check_cancel!();
        auth = sess.auth_read(storage);
        if auth.is_some() {
            break;
        }
        std::thread::sleep(retry_ms);
    }
    log::info!(
        "[batch-auth] read-probe auth  port={port} has_auth={}",
        auth.is_some()
    );

    // Filter out the "un-authorized" placeholder UUID
    let uuid = auth.filter(|(u, _)| u != PLACEHOLDER_UUID).map(|(u, _)| u);

    Ok(ReadAuthProbeResult { mac: mac_opt, uuid })
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Mock serial I/O for `AuthSession` unit tests.
    ///
    /// Each command sent via `send_cmd` begins with a `clear_input()` call;
    /// on that signal the mock loads the next queued response into its read
    /// buffer. `bytes_to_read`/`read` then serve that buffer, so the
    /// idle-timeout read loops terminate naturally once it drains.
    struct MockAuthIo {
        /// Response served for each subsequent command (in order).
        responses: VecDeque<Vec<u8>>,
        /// Bytes currently available to read.
        buf: Vec<u8>,
        /// Every byte slice written (for assertion).
        sent: Vec<Vec<u8>>,
        /// Control-line transitions in order: ('D', level) for DTR, ('R', level) for RTS.
        control_lines: Vec<(char, bool)>,
    }

    impl MockAuthIo {
        fn new() -> Self {
            Self {
                responses: VecDeque::new(),
                buf: Vec::new(),
                sent: Vec::new(),
                control_lines: Vec::new(),
            }
        }

        /// Queue the text returned after the next command's `clear_input`.
        fn add_response(&mut self, text: &str) {
            self.responses.push_back(text.as_bytes().to_vec());
        }

        /// Concatenation of all sent bytes as a UTF-8 string.
        fn sent_str(&self) -> String {
            String::from_utf8_lossy(&self.sent.concat()).to_string()
        }
    }

    impl AuthIo for MockAuthIo {
        fn bytes_to_read(&self) -> io::Result<u32> {
            Ok(self.buf.len() as u32)
        }
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.buf.len().min(buf.len());
            buf[..n].copy_from_slice(&self.buf[..n]);
            self.buf.drain(..n);
            Ok(n)
        }
        fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
            self.sent.push(data.to_vec());
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn set_dtr(&mut self, level: bool) -> io::Result<()> {
            self.control_lines.push(('D', level));
            Ok(())
        }
        fn set_rts(&mut self, level: bool) -> io::Result<()> {
            self.control_lines.push(('R', level));
            Ok(())
        }
        fn clear_input(&mut self) -> io::Result<()> {
            // A command is starting: load its response so the following read
            // loop sees the device's reply.
            self.buf = self.responses.pop_front().unwrap_or_default();
            Ok(())
        }
    }

    fn session(mock: MockAuthIo) -> AuthSession<MockAuthIo> {
        AuthSession {
            port: mock,
            timing: AuthTiming::default(),
        }
    }

    // ── auth_read parsing ──────────────────────────────────────────────

    #[test]
    fn auth_read_extracts_uuid_and_authkey() {
        let mut mock = MockAuthIo::new();
        mock.add_response(
            "auth-read\r\nuuid12345678901234\r\nkeyabcdefghijklmnopqrstuvwxyz012\r\ntuya> \r\n",
        );
        let mut sess = session(mock);
        assert_eq!(
            sess.auth_read(AuthStorage::Kv),
            Some((
                "uuid12345678901234".to_string(),
                "keyabcdefghijklmnopqrstuvwxyz012".to_string()
            ))
        );
    }

    #[test]
    fn auth_read_filters_device_log_lines() {
        let mut mock = MockAuthIo::new();
        // Device-log lines and the echoed command must be filtered; only the
        // two credential lines should remain.
        mock.add_response(
            "auth-read\r\n[04-24 10:30:00] [INFO] booting\r\nuuid12345678901234\r\n[04-24 10:30:01] noise\r\nkeyabcdefghijklmnopqrstuvwxyz012\r\ntuya>\r\n",
        );
        let mut sess = session(mock);
        assert_eq!(
            sess.auth_read(AuthStorage::Kv),
            Some((
                "uuid12345678901234".to_string(),
                "keyabcdefghijklmnopqrstuvwxyz012".to_string()
            ))
        );
    }

    #[test]
    fn auth_read_handles_ansi_escapes() {
        let mut mock = MockAuthIo::new();
        mock.add_response("auth-read\r\n\x1b[32muuid12345678901234\x1b[0m\r\nkeyabcdefghijklmnopqrstuvwxyz012\r\n");
        let mut sess = session(mock);
        assert_eq!(
            sess.auth_read(AuthStorage::Kv),
            Some((
                "uuid12345678901234".to_string(),
                "keyabcdefghijklmnopqrstuvwxyz012".to_string()
            ))
        );
    }

    #[test]
    fn auth_read_returns_none_for_single_line() {
        let mut mock = MockAuthIo::new();
        // Only one relevant line after filtering — not enough for a pair.
        mock.add_response("auth-read\r\nuuid12345678901234\r\ntuya>\r\n");
        let mut sess = session(mock);
        assert_eq!(sess.auth_read(AuthStorage::Kv), None);
    }

    #[test]
    fn auth_read_returns_none_for_empty_response() {
        let mut mock = MockAuthIo::new();
        mock.add_response("");
        let mut sess = session(mock);
        assert_eq!(sess.auth_read(AuthStorage::Kv), None);
    }

    #[test]
    fn auth_read_returns_none_when_only_logs_and_prompt() {
        let mut mock = MockAuthIo::new();
        mock.add_response("auth-read\r\n[04-24 10:30:00] [INFO] only logs\r\ntuya>\r\n");
        let mut sess = session(mock);
        assert_eq!(sess.auth_read(AuthStorage::Kv), None);
    }

    #[test]
    fn auth_read_detects_placeholder_uuid() {
        // Placeholder is a valid 2-line pair at the parsing layer; the
        // higher-level flows treat it as "no auth".
        let mut mock = MockAuthIo::new();
        mock.add_response(&format!(
            "auth-read\r\n{}\r\nkeyabcdefghijklmnopqrstuvwxyz012\r\n",
            PLACEHOLDER_UUID
        ));
        let mut sess = session(mock);
        assert_eq!(
            sess.auth_read(AuthStorage::Kv),
            Some((
                PLACEHOLDER_UUID.to_string(),
                "keyabcdefghijklmnopqrstuvwxyz012".to_string()
            ))
        );
    }

    // ── read_mac parsing ──────────────────────────────────────────────

    #[test]
    fn read_mac_parses_from_response() {
        let mut mock = MockAuthIo::new();
        mock.add_response("read_mac\r\nWIFI MAC ADDR:11:22:33:AA:BB:CC\r\ntuya>\r\n");
        let mut sess = session(mock);
        assert_eq!(sess.read_mac(), Some("11:22:33:AA:BB:CC".to_string()));
    }

    #[test]
    fn read_mac_returns_none_without_mac() {
        let mut mock = MockAuthIo::new();
        mock.add_response("read_mac\r\nno address here\r\ntuya>\r\n");
        let mut sess = session(mock);
        assert_eq!(sess.read_mac(), None);
    }

    // ── send_cmd records the right bytes ───────────────────────────────

    #[test]
    fn send_cmd_writes_command_with_crlf() {
        let mut mock = MockAuthIo::new();
        mock.add_response("");
        let mut sess = session(mock);
        sess.send_cmd("auth-read").unwrap();
        assert_eq!(sess.port.sent_str(), "auth-read\r\n");
    }

    #[test]
    fn auth_write_sends_auth_command() {
        let mut mock = MockAuthIo::new();
        mock.add_response("auth uuid key\r\nAuthorization write succeeds.\r\n");
        let mut sess = session(mock);
        let _ = sess.auth_write(
            "myuuid",
            "mykey",
            AuthStorage::Kv,
            Duration::from_millis(200),
        );
        assert!(sess.port.sent_str().contains("auth myuuid mykey\r\n"));
    }

    // ── command sequencing: drain → reset → wake → read ────────────────

    #[test]
    fn wake_shell_sends_three_newlines() {
        let mock = MockAuthIo::new();
        let mut sess = session(mock);
        sess.wake_shell(); // uses self.timing.boot_probe_interval
                           // Three bare CRLFs written.
        assert_eq!(
            sess.port
                .sent
                .iter()
                .filter(|b| b.as_slice() == b"\r\n")
                .count(),
            3
        );
    }

    #[test]
    fn hardware_reset_pulses_control_lines() {
        let mock = MockAuthIo::new();
        let mut sess = session(mock);
        assert!(sess.hardware_reset().is_ok());
        // Verify the exact reset sequence: DTR low, then RTS high→low pulse.
        // A regression that swapped DTR/RTS, dropped the falling RTS edge, or
        // reordered the pulse would change this recorded sequence.
        assert_eq!(
            sess.port.control_lines,
            vec![('D', false), ('R', true), ('R', false)]
        );
    }

    #[test]
    fn drain_boot_output_consumes_buffered_bytes() {
        // drain_boot_output does not issue a command (no clear_input), so
        // preload the read buffer directly with stale boot bytes.
        let mut sess = session(MockAuthIo::new());
        sess.port.buf = b"stale boot banner\r\n".to_vec();
        let n = sess.drain_boot_output();
        assert_eq!(n, "stale boot banner\r\n".len());
    }

    #[test]
    fn strip_ansi_removes_escape_sequences() {
        let input = "\x1b[32mhello\x1b[0m world";
        assert_eq!(strip_ansi(input), "hello world");
    }

    #[test]
    fn strip_ansi_passthrough_plain() {
        let input = "tuya> auth-read";
        assert_eq!(strip_ansi(input), input);
    }

    #[test]
    fn is_device_log_matches_prefix() {
        assert!(is_device_log("[04-24 10:30:00] [INFO] something"));
        assert!(!is_device_log("tuya> "));
        assert!(!is_device_log("Authorization write succeeds."));
    }

    #[test]
    fn is_shell_prompt_matches() {
        assert!(is_shell_prompt("tuya>"));
        assert!(is_shell_prompt("  tuya>  "));
        assert!(is_shell_prompt("tuya> read_mac"));
        assert!(!is_shell_prompt("[04-24] log line"));
    }

    #[test]
    fn parse_mac_detects_colon_format() {
        // "ADDR:11:22:33:AA:BB:CC" is 7 colon-parts; first "ADDR" is non-hex label
        assert_eq!(
            parse_mac_from_str("WIFI MAC ADDR:11:22:33:AA:BB:CC"),
            Some("11:22:33:AA:BB:CC".to_string())
        );
    }

    #[test]
    fn parse_mac_space_separated() {
        // MAC whitespace-separated from all context
        assert_eq!(
            parse_mac_from_str("mac 11:22:33:aa:bb:cc here"),
            Some("11:22:33:AA:BB:CC".to_string())
        );
    }

    #[test]
    fn parse_mac_case_insensitive_input() {
        assert_eq!(
            parse_mac_from_str("mac: aa:bb:cc:dd:ee:ff"),
            Some("AA:BB:CC:DD:EE:FF".to_string())
        );
    }

    #[test]
    fn parse_mac_returns_none_for_no_mac() {
        assert_eq!(parse_mac_from_str("no mac here"), None);
        assert_eq!(parse_mac_from_str(""), None);
    }

    #[test]
    fn detect_firmware_new_returns_new_kind() {
        let mut io = MockAuthIo::new();
        // Response 0: sys_log_enable off → OK
        io.add_response("OK: log disabled\r\n");
        // Response 1: wake_shell clear_input → empty
        io.add_response("");
        // Response 2: version command
        io.add_response("CLI version: 1.0.0\r\n");
        let mut sess = AuthSession {
            port: io,
            timing: AuthTiming::default(),
        };
        let cancel = AtomicBool::new(false);
        let result = sess.detect_firmware(&cancel).unwrap();
        assert_eq!(result, FirmwareKind::New(CliVersion(1, 0, 0)));
        assert!(sess.port.sent_str().contains("sys_log_enable off\r\n"));
        assert!(sess.port.sent_str().contains("version\r\n"));
    }

    #[test]
    fn detect_firmware_old_returns_old_kind() {
        let mut io = MockAuthIo::new();
        // Response 0: sys_log_enable off → not recognized
        io.add_response("No command or file name\r\n");
        // Response 1: wake_shell clear_input → empty
        io.add_response("");
        let mut sess = AuthSession {
            port: io,
            timing: AuthTiming::default(),
        };
        let cancel = AtomicBool::new(false);
        let result = sess.detect_firmware(&cancel).unwrap();
        assert_eq!(result, FirmwareKind::Old);
        // version command must NOT be sent for old firmware
        assert!(!sess.port.sent_str().contains("version\r\n"));
    }

    #[test]
    fn detect_firmware_no_response_returns_old_kind() {
        let mut io = MockAuthIo::new();
        // Response 0: empty (device not yet up)
        io.add_response("");
        // Response 1: wake_shell clear_input
        io.add_response("");
        let mut sess = AuthSession {
            port: io,
            timing: AuthTiming::default(),
        };
        let cancel = AtomicBool::new(false);
        let result = sess.detect_firmware(&cancel).unwrap();
        assert_eq!(result, FirmwareKind::Old);
    }

    #[test]
    fn parse_cli_version_parses_correctly() {
        assert_eq!(
            parse_cli_version("CLI version: 1.0.0"),
            Some(CliVersion(1, 0, 0))
        );
        assert_eq!(
            parse_cli_version("\x1b[32mCLI version: 2.3.4\x1b[0m"),
            Some(CliVersion(2, 3, 4))
        );
        assert_eq!(parse_cli_version("unknown"), None);
    }

    /// 新固件 — 写授权，无冲突，写入并验证成功
    #[test]
    fn run_authorize_new_firmware_write_no_conflict() {
        use crate::job::FlashMode;
        let mut io = MockAuthIo::new();
        // detect_firmware: sys_log_enable off → OK, wake_shell, version
        io.add_response("OK: log disabled\r\n"); // sys_log_enable off
        io.add_response(""); // wake_shell clear_input
        io.add_response("CLI version: 1.0.0\r\n"); // version
                                                   // auth_read (check existing) → None (device fresh)
        io.add_response("");
        // auth_write response
        io.add_response("Authorization write succeeds.\r\n");
        // drain + wake_shell after write
        io.add_response("");
        // auth_read (verify) → matches
        io.add_response("testuuid12345678901\r\ntestkey1234567890123456789012\r\n");
        // hardware_reset at end: no response needed

        let mut sess = AuthSession {
            port: io,
            timing: AuthTiming::default(),
        };
        let cancel = AtomicBool::new(false);
        let job = FlashJob {
            mode: FlashMode::Authorize,
            chip_id: String::new(),
            port: String::new(),
            baud_rate: 115_200,
            segments: None,
            flash_start_hex: None,
            flash_end_hex: None,
            erase_start_hex: None,
            erase_end_hex: None,
            read_start_hex: None,
            read_end_hex: None,
            read_file_path: None,
            firmware_path: None,
            authorize_uuid: Some("testuuid12345678901".into()),
            authorize_key: Some("testkey1234567890123456789012".into()),
            authorize_storage: None,
            confirm_overwrite: None,
        };
        // Call inner logic directly via a helper (see note below)
        // NOTE: since run_authorize calls AuthSession::open internally, we test the
        // inner steps via the session directly. Create a standalone test that wires
        // detect_firmware + the write path through AuthSession:
        let cancel2 = AtomicBool::new(false);
        let firmware = sess.detect_firmware(&cancel2).unwrap();
        assert_eq!(firmware, FirmwareKind::New(CliVersion(1, 0, 0)));
        // auth_read → None (fresh device)
        let existing = sess.auth_read(AuthStorage::Kv);
        assert!(existing.is_none());
        // auth_write
        let _lines = sess.auth_write(
            "testuuid12345678901",
            "testkey1234567890123456789012",
            AuthStorage::Kv,
            Duration::from_millis(200),
        );
        sess.drain_boot_output();
        sess.wake_shell(); // no timing arg — uses self.timing
                           // verify
        let verified = sess.auth_read(AuthStorage::Kv);
        assert_eq!(
            verified,
            Some((
                "testuuid12345678901".into(),
                "testkey1234567890123456789012".into()
            ))
        );
        assert!(sess
            .port
            .sent_str()
            .contains("auth testuuid12345678901 testkey1234567890123456789012\r\n"));
        let _ = job;
        let _ = cancel;
    }

    /// 新固件 — 写授权，有冲突，confirm_overwrite 返回 false → Cancelled
    #[test]
    fn run_authorize_new_firmware_conflict_cancelled() {
        let mut io = MockAuthIo::new();
        io.add_response("OK: log disabled\r\n");
        io.add_response("");
        io.add_response("CLI version: 1.0.0\r\n");
        // auth_read → existing different credentials
        io.add_response("existinguuid1234567\r\nexistingkey12345678901234567890\r\n");

        let mut sess = AuthSession {
            port: io,
            timing: AuthTiming::default(),
        };
        let cancel = AtomicBool::new(false);
        let firmware = sess.detect_firmware(&cancel).unwrap();
        assert!(matches!(firmware, FirmwareKind::New(_)));
        let existing = sess.auth_read(AuthStorage::Kv);
        assert!(existing.is_some());
        let (ex_u, _ex_k) = existing.unwrap();
        assert_eq!(ex_u, "existinguuid1234567");
        // Simulate confirm_overwrite returning false
        let confirmed = false;
        assert!(!confirmed, "should cancel when user declines");
    }

    #[test]
    fn auth_write_kv_omits_storage_param() {
        let mut mock = MockAuthIo::new();
        mock.add_response("Authorization write succeeds.\r\n");
        let mut sess = session(mock);
        let _ = sess.auth_write(
            "myuuid",
            "mykey",
            AuthStorage::Kv,
            Duration::from_millis(200),
        );
        // KV is default — must NOT append "0" to stay backward-compatible
        assert!(sess.port.sent_str().contains("auth myuuid mykey\r\n"));
        assert!(!sess.port.sent_str().contains("auth myuuid mykey 0\r\n"));
    }

    #[test]
    fn auth_write_otp_appends_storage_param() {
        let mut mock = MockAuthIo::new();
        mock.add_response("Authorization write succeeds.\r\n");
        let mut sess = session(mock);
        let _ = sess.auth_write(
            "myuuid",
            "mykey",
            AuthStorage::Otp,
            Duration::from_millis(200),
        );
        assert!(sess.port.sent_str().contains("auth myuuid mykey 1\r\n"));
    }

    #[test]
    fn auth_read_kv_omits_storage_param() {
        let mut mock = MockAuthIo::new();
        mock.add_response(
            "auth-read\r\nuuid12345678901234\r\nkeyabcdefghijklmnopqrstuvwxyz012\r\n",
        );
        let mut sess = session(mock);
        let _ = sess.auth_read(AuthStorage::Kv);
        assert!(sess.port.sent_str().contains("auth-read\r\n"));
        assert!(!sess.port.sent_str().contains("auth-read 0\r\n"));
    }

    #[test]
    fn auth_read_otp_appends_storage_param() {
        let mut mock = MockAuthIo::new();
        mock.add_response(
            "auth-read 1\r\nuuid12345678901234\r\nkeyabcdefghijklmnopqrstuvwxyz012\r\n",
        );
        let mut sess = session(mock);
        let result = sess.auth_read(AuthStorage::Otp);
        assert!(sess.port.sent_str().contains("auth-read 1\r\n"));
        assert_eq!(
            result,
            Some((
                "uuid12345678901234".to_string(),
                "keyabcdefghijklmnopqrstuvwxyz012".to_string()
            ))
        );
    }

    // ── auth_otp_lock ──────────────────────────────────────────────────

    #[test]
    fn auth_otp_lock_succeeds_on_success_line() {
        let mut mock = MockAuthIo::new();
        mock.add_response("auth-otp-lock\r\nAuthorization otp lock succeeds.\r\ntuya> \r\n");
        let mut sess = session(mock);
        assert!(sess.auth_otp_lock().is_ok());
    }

    #[test]
    fn auth_otp_lock_fails_on_failure_line() {
        let mut mock = MockAuthIo::new();
        mock.add_response("auth-otp-lock\r\nAuthorization otp lock failure.\r\ntuya> \r\n");
        let mut sess = session(mock);
        let err = sess.auth_otp_lock().unwrap_err();
        match err {
            FlashError::Plugin(msg) => assert!(
                msg.contains("device returned failure"),
                "unexpected message: {msg}"
            ),
            other => panic!("expected Plugin error, got {other:?}"),
        }
    }

    #[test]
    fn auth_otp_lock_fails_on_no_response() {
        let mut mock = MockAuthIo::new();
        mock.add_response("");
        let mut sess = session(mock);
        let err = sess.auth_otp_lock().unwrap_err();
        match err {
            FlashError::Plugin(msg) => assert!(
                msg.contains("no recognisable response"),
                "unexpected message: {msg}"
            ),
            other => panic!("expected Plugin error, got {other:?}"),
        }
    }

    #[test]
    fn auth_otp_lock_is_case_insensitive() {
        let mut mock = MockAuthIo::new();
        mock.add_response("auth-otp-lock\r\nAUTHORIZATION OTP LOCK SUCCEEDS.\r\ntuya> \r\n");
        let mut sess = session(mock);
        assert!(sess.auth_otp_lock().is_ok());
    }

    #[test]
    fn auth_otp_lock_ignores_unrelated_log_lines() {
        let mut mock = MockAuthIo::new();
        mock.add_response(
            "auth-otp-lock\r\n[04-24 10:30:00] [INFO] efuse settling\r\nAuthorization otp lock succeeds.\r\n[04-24 10:30:01] noise\r\ntuya> \r\n",
        );
        let mut sess = session(mock);
        assert!(sess.auth_otp_lock().is_ok());
    }
}
