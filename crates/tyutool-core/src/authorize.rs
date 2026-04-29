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

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::FlashError;
use crate::job::FlashJob;
use crate::progress::FlashProgress;

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

fn emit_log_key<F: Fn(FlashProgress)>(progress: &F, key: &str, pairs: &[(&str, String)]) {
    let mut params = HashMap::new();
    for (k, v) in pairs {
        params.insert((*k).to_string(), v.clone());
    }
    progress(FlashProgress::LogKey {
        key: key.to_string(),
        params,
    });
}

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

// ── Serial session ────────────────────────────────────────────────────────

struct AuthSession {
    port: Box<dyn serialport::SerialPort>,
}

impl AuthSession {
    fn open(port_name: &str) -> Result<Self, FlashError> {
        let mut port = serialport::new(port_name, BAUD)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| FlashError::Plugin(format!("cannot open {}: {}", port_name, e)))?;
        // De-assert control lines — avoid triggering download mode on open.
        let _ = port.write_data_terminal_ready(false);
        let _ = port.write_request_to_send(false);
        Ok(Self { port })
    }

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
                    if let Ok(read) = io::Read::read(&mut self.port, &mut buf[..to_read]) {
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
            .write_data_terminal_ready(false)
            .map_err(|e| FlashError::Plugin(format!("DTR error: {}", e)))?;
        self.port
            .write_request_to_send(true)
            .map_err(|e| FlashError::Plugin(format!("RTS high error: {}", e)))?;
        std::thread::sleep(Duration::from_millis(100));
        self.port
            .write_request_to_send(false)
            .map_err(|e| FlashError::Plugin(format!("RTS low error: {}", e)))?;
        std::thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    /// Clear RX buffer then write `cmd\r\n`.
    fn send_cmd(&mut self, cmd: &str) -> Result<(), FlashError> {
        let _ = self.port.clear(serialport::ClearBuffer::Input);
        let data = format!("{}\r\n", cmd);
        io::Write::write_all(&mut self.port, data.as_bytes()).map_err(FlashError::Io)?;
        io::Write::flush(&mut self.port).map_err(FlashError::Io)?;
        Ok(())
    }

    /// Send a few bare `\r\n` to flush any partial input and wait until the
    /// TuyaOpen shell is ready.  Call this after the post-boot drain and
    /// before issuing real commands when the device has just booted.
    fn wake_shell(&mut self) {
        for _ in 0..3 {
            let _ = io::Write::write_all(&mut self.port, b"\r\n");
            std::thread::sleep(Duration::from_millis(300));
        }
        // Drain prompt echoes and any leftover boot output.
        let _ = self.port.clear(serialport::ClearBuffer::Input);
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
                    if let Ok(read) = io::Read::read(&mut self.port, &mut tmp[..to_read]) {
                        raw_buf.extend_from_slice(&tmp[..read]);
                        last_data = Some(Instant::now());
                        // Extract complete `\n`-terminated lines
                        while let Some(pos) = raw_buf.iter().position(|&b| b == b'\n') {
                            let chunk: Vec<u8> = raw_buf.drain(..=pos).collect();
                            let s = String::from_utf8_lossy(&chunk)
                                .trim_end_matches(|c| c == '\r' || c == '\n')
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
}

// ── Public entry point ────────────────────────────────────────────────────

/// Run the TuyaOpen UART authorization flow.
///
/// Emits [`FlashProgress`] events throughout. The caller is responsible for
/// emitting the final `Done` event (matching the pattern in `run_job`).
pub fn run_authorize<F>(job: &FlashJob, cancel: &AtomicBool, progress: F) -> Result<(), FlashError>
where
    F: Fn(FlashProgress),
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
    emit_log_key(
        &progress,
        "flash.log.auth.openingPort",
        &[("port", job.port.clone())],
    );
    let mut sess = AuthSession::open(&job.port)?;

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Step 2: Drain stale boot output ──────────────────────────────
    emit_log_key(&progress, "flash.log.auth.drainBootOutput", &[]);
    sess.drain_boot_output();

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Step 3: Hardware reset ────────────────────────────────────────
    emit_log_key(&progress, "flash.log.auth.resetDevice", &[]);
    sess.hardware_reset()?;

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Step 4: Wait for boot ─────────────────────────────────────────
    emit_log_key(
        &progress,
        "flash.log.auth.waitBoot",
        &[("seconds", POST_RESET_WAIT.as_secs().to_string())],
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
    emit_log_key(&progress, "flash.log.auth.waitShell", &[]);
    sess.wake_shell();

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    if write_mode {
        // ── Step 5: Optional read — skip UART write if device already matches ──
        emit_log_key(&progress, "flash.log.auth.readDeviceAuth", &[]);
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
                emit_log_key(&progress, "flash.log.auth.alreadySame", &[]);
                return Ok(());
            }
        }

        // ── Step 6: Write auth ────────────────────────────────────────
        // Send the auth command once. Some firmware versions print
        // "Authorization write succeeds." and stay running; others reboot
        // immediately. Either way, we verify by auth-read after settling.
        emit_log_key(&progress, "flash.log.auth.writeStart", &[]);
        let _response_lines = sess.auth_write(&uuid, &authkey);

        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }

        // ── Step 6: Wait for device to settle after possible reboot ───
        emit_log_key(
            &progress,
            "flash.log.auth.waitSettle",
            &[("seconds", POST_RESET_WAIT.as_secs().to_string())],
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
        emit_log_key(&progress, "flash.log.auth.verify", &[]);
        match sess.auth_read() {
            Some((rb_uuid, rb_key)) if rb_uuid == uuid && rb_key == authkey => {
                emit_log_key(&progress, "flash.log.auth.verifyOk", &[]);
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
        emit_log_key(&progress, "flash.log.auth.readCurrent", &[]);
        match sess.auth_read() {
            Some((existing_uuid, existing_key)) => {
                if existing_uuid == PLACEHOLDER_UUID {
                    emit_log_key(&progress, "flash.log.auth.notAuthorized", &[]);
                } else {
                    emit_log_key(&progress, "flash.log.auth.authorized", &[]);
                    emit_log_key(
                        &progress,
                        "flash.log.auth.readResult",
                        &[
                            ("uuid", existing_uuid.clone()),
                            ("authkey", existing_key.clone()),
                        ],
                    );
                }
                Ok(())
            }
            None => {
                emit_log_key(&progress, "flash.log.auth.noData", &[]);
                Ok(())
            }
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
