//! Live serial monitor — stream device output to the terminal.
//!
//! Modeled on TuyaOpen `tools/cli_command/cli_monitor.py`: raw device output
//! is passed through to stdout (optionally teed to a log file with `-l`), and
//! keystrokes are forwarded to the device so the TuyaOpen shell can be driven
//! interactively (the device echoes typed characters, like miniterm). Reuses
//! `SerialDebugSession` from tyutool-core for the port reader thread and
//! disconnect detection. Quit with Ctrl+] (miniterm-compatible) or Ctrl+C.
//!
//! When stdin is not a terminal (pipe/CI), keys cannot be read raw; input is
//! forwarded line-by-line as `line\r\n` instead, and Ctrl+C (SIGINT) remains
//! the only quit path.

use std::io::{IsTerminal as _, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use console::{Key, Term};
use tyutool_core::{DataBits, DebugChunk, DebugConfig, Parity, SerialDebugSession, StopBits};

/// Ctrl+] — miniterm's exit character (`cli_monitor.py` uses chr(0x1d)).
const CTRL_RBRACKET: char = '\u{1d}';

/// What to do with one key read from the interactive terminal.
#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    /// Stop the monitor (Ctrl+] or Ctrl+C).
    Quit,
    /// Forward these bytes to the serial port.
    Send(Vec<u8>),
    /// Key has no serial mapping (arrows, function keys, …) — drop it.
    Ignore,
}

fn key_action(key: Key) -> KeyAction {
    match key {
        Key::CtrlC | Key::Char(CTRL_RBRACKET) => KeyAction::Quit,
        Key::Enter => KeyAction::Send(b"\r\n".to_vec()),
        Key::Tab => KeyAction::Send(b"\t".to_vec()),
        Key::Backspace => KeyAction::Send(b"\x08".to_vec()),
        Key::Char(c) => {
            let mut buf = [0u8; 4];
            KeyAction::Send(c.encode_utf8(&mut buf).as_bytes().to_vec())
        }
        _ => KeyAction::Ignore,
    }
}

pub fn run_monitor(
    port: &str,
    baud: u32,
    log_path: Option<&str>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let log_file = match log_path {
        Some(p) => {
            let f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .map_err(|e| format!("cannot open log file '{}': {}", p, e))?;
            Some(Arc::new(Mutex::new(f)))
        }
        None => None,
    };

    let disconnect_reason: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let cfg = DebugConfig {
        port: port.to_string(),
        baud_rate: baud,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    };

    let log_for_chunk = log_file.clone();
    let on_chunk = Box::new(move |chunk: DebugChunk| {
        let mut out = std::io::stdout();
        let _ = out.write_all(&chunk.bytes);
        let _ = out.flush();
        if let Some(ref f) = log_for_chunk {
            if let Ok(mut f) = f.lock() {
                let _ = f.write_all(&chunk.bytes);
                let _ = f.flush();
            }
        }
    });

    let disconnect_for_cb = Arc::clone(&disconnect_reason);
    let on_disconnect = Box::new(move |reason: String| {
        if let Ok(mut slot) = disconnect_for_cb.lock() {
            slot.get_or_insert(reason);
        }
    });

    let session = SerialDebugSession::open(cfg, on_chunk, on_disconnect)
        .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;

    eprintln!(
        "--- Monitor {} {} baud --- Quit: Ctrl+] or Ctrl+C ---",
        port, baud
    );
    if let Some(p) = log_path {
        eprintln!("--- Log file: {} ---", p);
    }

    // stdin → serial. Interactive terminal: read raw keys so Ctrl+] can be
    // caught, forwarding each key's bytes immediately (device echoes them).
    // Non-terminal stdin (pipe/CI): forward complete lines as `line\r\n`;
    // on EOF the thread exits and the monitor continues read-only.
    //
    // The thread parks on stdin and is torn down with the process on quit.
    // Like miniterm, a disconnect-initiated exit can leave one raw-mode key
    // read pending; the console crate restores the terminal per read, so only
    // that final pending read is affected.
    let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
    if std::io::stdin().is_terminal() {
        let cancel_for_stdin = Arc::clone(cancel);
        std::thread::spawn(move || {
            let term = Term::stdout();
            loop {
                if cancel_for_stdin.load(Ordering::Relaxed) {
                    break;
                }
                // read_key_raw returns Key::CtrlC instead of raising SIGINT,
                // so both quit keys funnel through KeyAction::Quit.
                match term.read_key_raw() {
                    Ok(key) => match key_action(key) {
                        KeyAction::Quit => {
                            cancel_for_stdin.store(true, Ordering::SeqCst);
                            break;
                        }
                        KeyAction::Send(bytes) => {
                            if input_tx.send(bytes).is_err() {
                                break;
                            }
                        }
                        KeyAction::Ignore => {}
                    },
                    Err(_) => break,
                }
            }
        });
    } else {
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            loop {
                line.clear();
                match stdin.read_line(&mut line) {
                    Ok(0) | Err(_) => break, // EOF or read error
                    Ok(_) => {
                        let trimmed = line.trim_end_matches(['\r', '\n']);
                        let bytes = format!("{}\r\n", trimmed).into_bytes();
                        if input_tx.send(bytes).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let reason = disconnect_reason.lock().ok().and_then(|mut s| s.take());
        if let Some(reason) = reason {
            // Mirror cli_monitor.py: report the disconnect and exit cleanly.
            eprintln!();
            eprintln!(
                "--- Monitor stopped: serial port {} disconnected or unavailable. ---",
                port
            );
            let detail = reason.trim();
            if !detail.is_empty() {
                eprintln!("--- Detail: {} ---", detail);
            }
            return Ok(());
        }
        while let Ok(bytes) = input_rx.try_recv() {
            if let Err(e) = session.write(&bytes) {
                log::warn!("[monitor] stdin forward failed: {}", e);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    session.close();
    eprintln!();
    eprintln!("--- Monitor stopped ---");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_rbracket_and_ctrl_c_quit() {
        assert_eq!(key_action(Key::Char(CTRL_RBRACKET)), KeyAction::Quit);
        assert_eq!(key_action(Key::CtrlC), KeyAction::Quit);
    }

    #[test]
    fn enter_sends_crlf() {
        assert_eq!(key_action(Key::Enter), KeyAction::Send(b"\r\n".to_vec()));
    }

    #[test]
    fn printable_chars_send_utf8_bytes() {
        assert_eq!(key_action(Key::Char('a')), KeyAction::Send(b"a".to_vec()));
        assert_eq!(
            key_action(Key::Char('中')),
            KeyAction::Send("中".as_bytes().to_vec())
        );
    }

    #[test]
    fn tab_and_backspace_send_control_bytes() {
        assert_eq!(key_action(Key::Tab), KeyAction::Send(b"\t".to_vec()));
        assert_eq!(
            key_action(Key::Backspace),
            KeyAction::Send(b"\x08".to_vec())
        );
    }

    #[test]
    fn unmapped_keys_are_ignored() {
        assert_eq!(key_action(Key::ArrowUp), KeyAction::Ignore);
        assert_eq!(key_action(Key::Escape), KeyAction::Ignore);
        assert_eq!(key_action(Key::Home), KeyAction::Ignore);
    }
}
