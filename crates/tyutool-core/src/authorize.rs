//! TuyaOpen UART authorization — serial text-command exchange protocol.
//!
//! Entirely independent of BootROM/flash protocols. All commands are plain
//! ASCII terminated with `\r\n`, processed by the TuyaOpen CLI shell.
//!
//! # Write flow (uuid + authkey provided)
//! 1. Open serial at 115 200 baud
//! 2. Drain stale boot output
//! 3. Hardware reset via DTR/RTS pulse (same as tos.py)
//! 4. Wait 3 s for device to boot, drain again
//! 5. Optional `auth-read`: if already matches requested credentials, skip write
//! 6. `auth <uuid> <authkey>` → write authorization
//! 7. `auth-read` → verify written values
//!
//! Overwrite confirmation when device auth differs is implemented in the GUI (probe + dialog);
//! the core always performs the UART write when credentials are supplied.
//!
//! # Read-only flow (uuid + authkey absent)
//! Steps 1–4, then `auth-read` to display current auth state.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashMilestone};
use crate::job::FlashJob;

// ── Timing (aligned with auth_handler.py) ────────────────────────────────

const BAUD: u32 = 115_200;
/// Per-command read window.
const CMD_TIMEOUT: Duration = Duration::from_secs(3);
/// Stop reading after this long with no new data.
const IDLE_TIMEOUT: Duration = Duration::from_millis(300);
/// Drain: stop when silent for this long.
const DRAIN_QUIET: Duration = Duration::from_millis(800);
/// Drain: give up after this long regardless.
const DRAIN_MAX: Duration = Duration::from_secs(5);
/// Wait after hardware reset before sending the first command.
const POST_RESET_WAIT: Duration = Duration::from_secs(3);
/// Devices shipped un-authorized carry this placeholder UUID.
const PLACEHOLDER_UUID: &str = "uuidxxxxxxxxxxxxxxxx";

/// Parsed device authorization (used by GUI / [`probe_device_authorization`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthorization {
    pub uuid: String,
    pub authkey: String,
}

/// Open UART, boot shell, and read current `auth-read` pair. Returns `None` if unparseable,
/// empty, or factory placeholder — caller may treat as “no conflicting auth”.
pub fn probe_device_authorization(
    port: &str,
    cancel: &AtomicBool,
) -> Result<Option<DeviceAuthorization>, FlashError> {
    let mut sess = AuthSession::open(port)?;

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }
    sess.drain_boot_output();

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }
    sess.hardware_reset()?;

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    let wait_end = Instant::now() + POST_RESET_WAIT;
    while Instant::now() < wait_end {
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    sess.drain_boot_output();

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }
    sess.wake_shell();

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    let mut pair: Option<(String, String)> = None;
    for _ in 1..=5u32 {
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }
        pair = sess.auth_read();
        if pair.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(800));
    }

    Ok(match pair {
        Some((u, k)) if !u.is_empty() && !k.is_empty() && u != PLACEHOLDER_UUID => {
            Some(DeviceAuthorization {
                uuid: u,
                authkey: k,
            })
        }
        _ => None,
    })
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
}

impl AuthSession<SerialAuthIo> {
    fn open(port_name: &str) -> Result<Self, FlashError> {
        let mut port = serialport::new(port_name, BAUD)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| FlashError::Plugin(format!("cannot open {}: {}", port_name, e)))?;
        // De-assert control lines — avoid triggering download mode on open.
        let _ = port.write_data_terminal_ready(false);
        let _ = port.write_request_to_send(false);
        Ok(Self {
            port: SerialAuthIo { port },
        })
    }
}

impl<T: AuthIo> AuthSession<T> {
    /// Read and discard bytes until the line has been quiet for [`DRAIN_QUIET`]
    /// or [`DRAIN_MAX`] has elapsed. Returns total bytes consumed.
    fn drain_boot_output(&mut self) -> usize {
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
                    if Instant::now().duration_since(last_data) >= DRAIN_QUIET {
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
    fn wake_shell(&mut self) {
        for _ in 0..3 {
            let _ = self.port.write_all(b"\r\n");
            std::thread::sleep(Duration::from_millis(300));
        }
        // Drain prompt echoes and any leftover boot output.
        let _ = self.port.clear_input();
    }

    /// Read response lines within [`CMD_TIMEOUT`], returning early after
    /// `idle_timeout` of silence once data has started arriving.
    fn read_response_idle(&mut self, idle_timeout: Duration) -> Vec<String> {
        let mut raw_buf: Vec<u8> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        let end_time = Instant::now() + CMD_TIMEOUT;
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
                        raw_buf.extend_from_slice(&tmp[..read]);
                        last_data = Some(Instant::now());
                        // Extract complete `\n`-terminated lines
                        while let Some(pos) = raw_buf.iter().position(|&b| b == b'\n') {
                            let chunk: Vec<u8> = raw_buf.drain(..=pos).collect();
                            let s = String::from_utf8_lossy(&chunk)
                                .trim_end_matches(['\r', '\n'])
                                .to_string();
                            let s = strip_ansi(&s).trim().to_string();
                            if !s.is_empty() {
                                lines.push(s);
                            }
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
        // Flush any remaining bytes that didn't end with `\n`
        if !raw_buf.is_empty() {
            let s = String::from_utf8_lossy(&raw_buf).trim().to_string();
            let s = strip_ansi(&s).trim().to_string();
            if !s.is_empty() {
                lines.push(s);
            }
        }
        lines
    }

    /// Shorthand: read response with the default [`IDLE_TIMEOUT`].
    fn read_response(&mut self) -> Vec<String> {
        self.read_response_idle(IDLE_TIMEOUT)
    }

    /// Send `auth-read` and return `(uuid, authkey)` or `None`.
    fn auth_read(&mut self) -> Option<(String, String)> {
        self.send_cmd("auth-read").ok()?;
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
    /// Uses a longer idle timeout (2 s) because some firmware versions reboot
    /// after writing auth — we want to capture the full reboot banner.
    /// Callers must verify success via [`Self::auth_read`] rather than
    /// inspecting the returned lines, since not all firmware versions print
    /// `"Authorization write succeeds."` before rebooting.
    fn auth_write(&mut self, uuid: &str, authkey: &str) -> Vec<String> {
        if self
            .send_cmd(&format!("auth {} {}", uuid, authkey))
            .is_err()
        {
            return vec![];
        }
        self.read_response_idle(Duration::from_millis(2000))
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

// ── Batch auth types ──────────────────────────────────────────────────────

/// Outcome of a single batch-auth UART session.
#[derive(Debug)]
pub enum BatchAuthSlotResult {
    /// Auth written and verified successfully.
    Done { mac: String },
    /// Device already had the exact credentials — nothing written.
    AlreadyDone { mac: String },
    /// Auth on device didn't match but conflict_policy=Skip — nothing written.
    Skipped { mac: String },
    /// Operation was cancelled.
    Cancelled,
}

/// What to do when device already has conflicting auth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Skip,
    Overwrite,
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
    let mut sess = AuthSession::open(&job.port)?;

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Step 2: Drain stale boot output ──────────────────────────────
    log::info!("flash.log.auth.drainBootOutput");
    sess.drain_boot_output();

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Step 3: Hardware reset ────────────────────────────────────────
    log::info!("flash.log.auth.resetDevice");
    sess.hardware_reset()?;

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Step 4: Wait for boot ─────────────────────────────────────────
    log::info!(
        "flash.log.auth.waitBoot: seconds={}",
        POST_RESET_WAIT.as_secs()
    );
    let wait_end = Instant::now() + POST_RESET_WAIT;
    while Instant::now() < wait_end {
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    sess.drain_boot_output();

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // Send a few Enter keypresses to ensure the TuyaOpen CLI shell is fully
    // interactive before we issue auth commands (prevents auth-read returning
    // None when the shell is still printing its boot banner).
    log::info!("flash.log.auth.waitShell");
    sess.wake_shell();

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    if write_mode {
        // ── Step 5: Optional read — skip UART write if device already matches ──
        log::info!("flash.log.auth.readDeviceAuth");
        let mut existing_auth: Option<(String, String)> = None;
        for _attempt in 1..=5u32 {
            if cancel.load(Ordering::Relaxed) {
                return Err(FlashError::Cancelled);
            }
            existing_auth = sess.auth_read();
            if existing_auth.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(800));
        }

        if let Some((ref u, ref k)) = existing_auth {
            if !u.is_empty()
                && !k.is_empty()
                && *u != PLACEHOLDER_UUID
                && u == &uuid
                && k == &authkey
            {
                log::info!("flash.log.auth.alreadySame");
                return Ok(());
            }
        }

        // ── Step 6: Write auth ────────────────────────────────────────
        // Send the auth command once. Some firmware versions print
        // "Authorization write succeeds." and stay running; others reboot
        // immediately. Either way, we verify by auth-read after settling.
        log::info!("flash.log.auth.writeStart");
        let _response_lines = sess.auth_write(&uuid, &authkey);

        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }

        // ── Step 6: Wait for device to settle after possible reboot ───
        log::info!(
            "flash.log.auth.waitSettle: seconds={}",
            POST_RESET_WAIT.as_secs()
        );
        let wait_end = Instant::now() + POST_RESET_WAIT;
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

        // ── Step 7: Verify via auth-read ──────────────────────────────
        log::info!("flash.log.auth.verify");
        match sess.auth_read() {
            Some((rb_uuid, rb_key)) if rb_uuid == uuid && rb_key == authkey => {
                log::info!("flash.log.auth.verifyOk");
                Ok(())
            }
            Some((rb_uuid, rb_key)) if rb_uuid == uuid => {
                // UUID matched but authkey differs — write was rejected by device.
                let _ = rb_key;
                Err(FlashError::Plugin(
                    "Verification failed: AuthKey mismatch (device may have rejected the write — check UUID/AuthKey length and format)".into(),
                ))
            }
            Some((rb_uuid, _rb_key)) => Err(FlashError::Plugin(format!(
                "Verification failed: UUID mismatch (wrote {}, read back {})",
                uuid, rb_uuid
            ))),
            None => Err(FlashError::Plugin(
                "Verification failed: no response from auth-read".into(),
            )),
        }
    } else {
        // ── Step 5 (read-only): auth-read ─────────────────────────────
        log::info!("flash.log.auth.readCurrent");
        match sess.auth_read() {
            Some((existing_uuid, existing_key)) => {
                if existing_uuid == PLACEHOLDER_UUID {
                    progress(FlashEvent::Milestone {
                        milestone: FlashMilestone::AuthReadEmpty,
                    });
                } else {
                    log::info!("flash.log.auth.authorized");
                    progress(FlashEvent::Milestone {
                        milestone: FlashMilestone::AuthReadComplete {
                            uuid: existing_uuid.clone(),
                            authkey: existing_key.clone(),
                        },
                    });
                }
                Ok(())
            }
            None => {
                progress(FlashEvent::Milestone {
                    milestone: FlashMilestone::AuthReadEmpty,
                });
                Ok(())
            }
        }
    }
}

/// Single-device batch authorization slot: open UART, read MAC, read/write auth, verify.
///
/// The caller pre-allocates `uuid`/`authkey` from an Excel row. On return:
/// - `Done`/`AlreadyDone` → caller should confirm the Excel row (mark USED).
/// - `Skipped`/`Err`/`Cancelled` → caller should release the Excel row.
pub fn run_batch_auth_slot<F>(
    port: &str,
    uuid: &str,
    authkey: &str,
    conflict_policy: ConflictPolicy,
    cancel: &AtomicBool,
    progress: F,
) -> Result<BatchAuthSlotResult, FlashError>
where
    F: Fn(BatchAuthStep),
{
    macro_rules! check_cancel {
        () => {
            if cancel.load(Ordering::Relaxed) {
                return Ok(BatchAuthSlotResult::Cancelled);
            }
        };
    }

    let mut sess = AuthSession::open(port)?;
    check_cancel!();

    sess.drain_boot_output();
    check_cancel!();
    sess.hardware_reset()?;
    check_cancel!();

    let wait_end = Instant::now() + POST_RESET_WAIT;
    while Instant::now() < wait_end {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
    }
    sess.drain_boot_output();
    check_cancel!();
    sess.wake_shell();
    check_cancel!();

    // Read MAC
    progress(BatchAuthStep::ReadingMac);
    let mac = {
        let mut mac_opt = None;
        for _ in 0..3u8 {
            check_cancel!();
            mac_opt = sess.read_mac();
            if mac_opt.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        mac_opt.unwrap_or_else(|| "UNKNOWN".to_string())
    };

    // Read existing auth
    progress(BatchAuthStep::ReadingAuth);
    let existing_auth = {
        let mut auth = None;
        for _ in 0..3u8 {
            check_cancel!();
            auth = sess.auth_read();
            if auth.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(800));
        }
        auth
    };

    // Check if device already has the credentials we want to write
    if let Some((ref ex_uuid, ref ex_key)) = existing_auth {
        // Factory-fresh devices carry this placeholder — treat as uninitialized
        if ex_uuid != PLACEHOLDER_UUID {
            if ex_uuid == uuid && ex_key == authkey {
                return Ok(BatchAuthSlotResult::AlreadyDone { mac });
            }
            if conflict_policy == ConflictPolicy::Skip {
                return Ok(BatchAuthSlotResult::Skipped { mac });
            }
        }
        // Overwrite (or placeholder device): fall through to write
    }

    // Write auth
    progress(BatchAuthStep::WritingAuth);
    let _lines = sess.auth_write(uuid, authkey);
    check_cancel!();

    // Wait for device to settle after possible reboot
    let wait_end = Instant::now() + POST_RESET_WAIT;
    while Instant::now() < wait_end {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
    }
    sess.drain_boot_output();
    sess.wake_shell();
    check_cancel!();

    // Verify
    progress(BatchAuthStep::Verifying);
    match sess.auth_read() {
        Some((rb_uuid, rb_key)) if rb_uuid == uuid && rb_key == authkey => {
            Ok(BatchAuthSlotResult::Done { mac })
        }
        Some((rb_uuid, rb_key)) => Err(FlashError::Plugin(format!(
            "Verification failed: wrote ({}, {}), read back ({}, {})",
            uuid, authkey, rb_uuid, rb_key
        ))),
        None => Err(FlashError::Plugin(
            "Verification failed: no response from auth-read".into(),
        )),
    }
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
    }

    impl MockAuthIo {
        fn new() -> Self {
            Self {
                responses: VecDeque::new(),
                buf: Vec::new(),
                sent: Vec::new(),
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
        fn set_dtr(&mut self, _level: bool) -> io::Result<()> {
            Ok(())
        }
        fn set_rts(&mut self, _level: bool) -> io::Result<()> {
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
        AuthSession { port: mock }
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
            sess.auth_read(),
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
            sess.auth_read(),
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
            sess.auth_read(),
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
        assert_eq!(sess.auth_read(), None);
    }

    #[test]
    fn auth_read_returns_none_for_empty_response() {
        let mut mock = MockAuthIo::new();
        mock.add_response("");
        let mut sess = session(mock);
        assert_eq!(sess.auth_read(), None);
    }

    #[test]
    fn auth_read_returns_none_when_only_logs_and_prompt() {
        let mut mock = MockAuthIo::new();
        mock.add_response("auth-read\r\n[04-24 10:30:00] [INFO] only logs\r\ntuya>\r\n");
        let mut sess = session(mock);
        assert_eq!(sess.auth_read(), None);
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
            sess.auth_read(),
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
        let _ = sess.auth_write("myuuid", "mykey");
        assert!(sess.port.sent_str().contains("auth myuuid mykey\r\n"));
    }

    // ── command sequencing: drain → reset → wake → read ────────────────

    #[test]
    fn wake_shell_sends_three_newlines() {
        let mock = MockAuthIo::new();
        let mut sess = session(mock);
        sess.wake_shell();
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
        // No control-line errors from the mock → Ok.
        assert!(sess.hardware_reset().is_ok());
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
}
