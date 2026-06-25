//! WebSocket dev-serve mode for tyutool-cli.
//! Exposes serial port operations over a local WebSocket so the Vite dev
//! server (localhost:1420) can flash real devices without the Tauri shell.
//!
//! Usage: tyutool-cli serve [--port 9527]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Context;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tyutool_core::{
    device_reset_dtr_rts, list_serial_ports, run_job, DebugChunk, DebugConfig, FlashJob,
    SerialDebugSession, SerialPortEntry,
};

// ── Client → Server ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum ClientMessage {
    ListPorts,
    DeviceReset {
        port: String,
        chip_id: String,
    },
    RunJob {
        job: FlashJob,
        #[serde(default)]
        file_content: Option<String>,
        #[serde(default)]
        file_contents: Option<Vec<String>>,
    },
    Cancel,
    SerialDebugOpen {
        cfg: DebugConfig,
    },
    SerialDebugClose,
    SerialDebugSend {
        bytes: Vec<u8>,
    },
    SerialDebugState,
    /// Frontend response to `flash_progress` `auth_conflict` milestone.
    AuthorizeConfirm {
        confirmed: bool,
    },
}

// ── Server → Client ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ports {
        ports: Vec<SerialPortEntry>,
    },
    DeviceResetResult {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Progress {
        payload: serde_json::Value,
    },
    Error {
        message: String,
    },
    SerialDebugChunk {
        chunk: DebugChunk,
    },
    SerialDebugOpened,
    SerialDebugClosed,
    SerialDebugStateInfo {
        open: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        cfg: Option<DebugConfig>,
    },
    SerialDebugDisconnected {
        reason: String,
    },
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub async fn run_serve(port: u16) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    println!("tyutool-cli serve listening on ws://{addr}");
    println!("Press Ctrl+C to stop.");

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                log::info!("WS connection from {peer}");
                tokio::spawn(handle_connection(stream));
            }
            Err(e) => log::warn!("accept error: {e}"),
        }
    }
}

// ── Per-connection handler ───────────────────────────────────────────────────

async fn handle_connection(stream: tokio::net::TcpStream) {
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            log::warn!("WS handshake failed: {e}");
            return;
        }
    };

    let (sink, mut stream) = ws.split();
    let cancel = Arc::new(AtomicBool::new(false));
    let pending_confirm: Arc<Mutex<Option<std::sync::mpsc::Sender<bool>>>> =
        Arc::new(Mutex::new(None));

    // ── mpsc sink pump ───────────────────────────────────────────────────────
    // Background tasks (progress callbacks, serial-debug reader thread) need to
    // push ServerMessage values from contexts that don't own the WS sink. Wrap
    // the sink in a single drainer task fed by an unbounded mpsc.
    use tokio::sync::mpsc;
    let (sink_tx, mut sink_rx) = mpsc::unbounded_channel::<ServerMessage>();

    let mut sink_moved = sink;
    let pump = tokio::spawn(async move {
        while let Some(msg) = sink_rx.recv().await {
            let text = serde_json::to_string(&msg).unwrap_or_else(|e| {
                serde_json::to_string(&ServerMessage::Error {
                    message: e.to_string(),
                })
                .unwrap_or_else(|_| "{\"type\":\"error\",\"message\":\"serialize failed\"}".into())
            });
            if sink_moved.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    // Per-connection serial-debug session state.
    let debug_session: Arc<Mutex<Option<SerialDebugSession>>> = Arc::new(Mutex::new(None));

    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = sink_tx.send(ServerMessage::Error {
                    message: e.to_string(),
                });
                continue;
            }
        };

        match client_msg {
            ClientMessage::ListPorts => {
                let ports = list_serial_ports().unwrap_or_default();
                let _ = sink_tx.send(ServerMessage::Ports { ports });
            }
            ClientMessage::DeviceReset { port, chip_id } => {
                // Run on blocking pool so serial `open()` / DTR/RTS never stalls the WS task.
                let join =
                    tokio::task::spawn_blocking(move || device_reset_dtr_rts(&port, &chip_id));
                let outcome = tokio::time::timeout(Duration::from_secs(15), join).await;
                let msg = match outcome {
                    Ok(Ok(Ok(()))) => ServerMessage::DeviceResetResult {
                        ok: true,
                        error: None,
                    },
                    Ok(Ok(Err(e))) => ServerMessage::DeviceResetResult {
                        ok: false,
                        error: Some(e.to_string()),
                    },
                    Ok(Err(join_e)) => ServerMessage::DeviceResetResult {
                        ok: false,
                        error: Some(format!("reset task join: {join_e}")),
                    },
                    Err(_elapsed) => ServerMessage::DeviceResetResult {
                        ok: false,
                        error: Some(
                            "device reset timed out (serial port blocked or driver hung)".into(),
                        ),
                    },
                };
                let _ = sink_tx.send(msg);
            }
            ClientMessage::Cancel => {
                cancel.store(true, Ordering::Relaxed);
                // Wake any thread blocked in confirm_overwrite so it can return Cancelled.
                if let Ok(mut sender_guard) = pending_confirm.lock() {
                    if let Some(tx) = sender_guard.take() {
                        let _ = tx.send(false);
                    }
                }
            }
            ClientMessage::RunJob {
                mut job,
                file_content,
                file_contents,
            } => {
                cancel.store(false, Ordering::Relaxed);
                handle_run_job(
                    &sink_tx,
                    Arc::clone(&cancel),
                    &mut job,
                    file_content,
                    file_contents,
                    Arc::clone(&pending_confirm),
                )
                .await;
            }
            ClientMessage::SerialDebugOpen { cfg } => {
                let mut guard = debug_session.lock().unwrap();
                if guard.is_some() {
                    let _ = sink_tx.send(ServerMessage::Error {
                        message: "already open".into(),
                    });
                    continue;
                }
                let sink_for_chunk = sink_tx.clone();
                let sink_for_disc = sink_tx.clone();
                let result = SerialDebugSession::open(
                    cfg,
                    Box::new(move |chunk| {
                        let _ = sink_for_chunk.send(ServerMessage::SerialDebugChunk { chunk });
                    }),
                    Box::new(move |reason| {
                        let _ =
                            sink_for_disc.send(ServerMessage::SerialDebugDisconnected { reason });
                    }),
                );
                match result {
                    Ok(s) => {
                        *guard = Some(s);
                        let _ = sink_tx.send(ServerMessage::SerialDebugOpened);
                    }
                    Err(e) => {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
            ClientMessage::SerialDebugClose => {
                let mut guard = debug_session.lock().unwrap();
                if let Some(s) = guard.take() {
                    s.close();
                }
                let _ = sink_tx.send(ServerMessage::SerialDebugClosed);
            }
            ClientMessage::SerialDebugSend { bytes } => {
                let guard = debug_session.lock().unwrap();
                if let Some(s) = guard.as_ref() {
                    if let Err(e) = s.write(&bytes) {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message: e.to_string(),
                        });
                        continue;
                    }
                    let ts_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let _ = sink_tx.send(ServerMessage::SerialDebugChunk {
                        chunk: DebugChunk {
                            direction: tyutool_core::Direction::Tx,
                            ts_ms,
                            bytes,
                        },
                    });
                } else {
                    let _ = sink_tx.send(ServerMessage::Error {
                        message: "serial debug not open".into(),
                    });
                }
            }
            ClientMessage::SerialDebugState => {
                let guard = debug_session.lock().unwrap();
                let (open, cfg) = match guard.as_ref() {
                    Some(s) => (true, Some(s.config().clone())),
                    None => (false, None),
                };
                let _ = sink_tx.send(ServerMessage::SerialDebugStateInfo { open, cfg });
            }
            ClientMessage::AuthorizeConfirm { confirmed } => {
                let mut guard = pending_confirm.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(tx) = guard.take() {
                    let _ = tx.send(confirmed);
                }
            }
        }
    }

    // Clean up any open serial-debug session before dropping the sink.
    if let Ok(mut guard) = debug_session.lock() {
        if let Some(s) = guard.take() {
            s.close();
        }
    }

    drop(sink_tx);
    let _ = pump.await;

    log::info!("WS connection closed");
}

// ── Run job handler ──────────────────────────────────────────────────────────

async fn handle_run_job(
    sink_tx: &tokio::sync::mpsc::UnboundedSender<ServerMessage>,
    cancel: Arc<AtomicBool>,
    job: &mut FlashJob,
    file_content: Option<String>,
    file_contents: Option<Vec<String>>,
    pending_confirm: Arc<Mutex<Option<std::sync::mpsc::Sender<bool>>>>,
) {
    let mut temp_paths: Vec<String> = Vec::new();

    // Decode single base64 firmware (legacy/read mode source)
    if let Some(b64) = file_content {
        match decode_to_temp(&b64, "tyutool_fw") {
            Ok(p) => {
                job.firmware_path = Some(p.clone());
                temp_paths.push(p);
            }
            Err(e) => {
                let _ = sink_tx.send(ServerMessage::Error {
                    message: e.to_string(),
                });
                return;
            }
        }
    }

    // Decode multiple base64 firmwares for multi-segment flashing
    if let Some(contents) = file_contents {
        if let Some(ref mut segments) = job.segments {
            if contents.len() != segments.len() {
                let _ = sink_tx.send(ServerMessage::Error {
                    message: "file_contents length mismatch with segments".into(),
                });
                return;
            }
            for (i, b64) in contents.iter().enumerate() {
                match decode_to_temp(b64, &format!("tyutool_seg_{i}")) {
                    Ok(p) => {
                        segments[i].firmware_path = p.clone();
                        temp_paths.push(p);
                    }
                    Err(e) => {
                        let _ = sink_tx.send(ServerMessage::Error {
                            message: e.to_string(),
                        });
                        // cleanup already created files? temp_paths will be cleaned at end
                        return;
                    }
                }
            }
        }
    }

    // For read mode: use temp path when client path is empty/absent
    let is_read = matches!(job.mode, tyutool_core::FlashMode::Read);
    if is_read && job.read_file_path.as_deref().unwrap_or("").is_empty() {
        job.read_file_path = Some(temp_path("tyutool_read", "bin"));
    }
    let read_path = if is_read {
        job.read_file_path.clone()
    } else {
        None
    };

    let mut job_clone = job.clone();

    // Inject confirm_overwrite AFTER cloning because Clone resets it to None
    // (see impl Clone for FlashJob in crates/tyutool-core/src/job.rs).
    let sink_for_confirm = sink_tx.clone();
    let confirm_store = Arc::clone(&pending_confirm);
    job_clone.confirm_overwrite = Some(Box::new(move |existing_uuid, existing_authkey| {
        use tyutool_core::{FlashEvent, FlashMilestone};
        if let Ok(v) = serde_json::to_value(FlashEvent::Milestone {
            milestone: FlashMilestone::AuthConflict {
                existing_uuid,
                existing_authkey,
            },
        }) {
            let _ = sink_for_confirm.send(ServerMessage::Progress { payload: v });
        }
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        {
            let mut guard = confirm_store.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(tx);
        }
        rx.recv().unwrap_or(false)
    }));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();

    // Run job in a blocking thread; collect file output there too.
    let handle = tokio::task::spawn_blocking(move || {
        let result = run_job(&job_clone, &cancel, |p| {
            if let Ok(v) = serde_json::to_value(&p) {
                let _ = tx.send(v);
            }
        });

        // Read output file inside the blocking thread before returning
        let file_b64 = if let Some(ref path) = read_path {
            let b64 = std::fs::read(path)
                .ok()
                .map(|b| base64::engine::general_purpose::STANDARD.encode(&b));
            let _ = std::fs::remove_file(path);
            b64
        } else {
            None
        };

        (result, file_b64)
    });

    // Forward all progress from `run_job` (including `Done`). Only synthesize `Done` below if
    // the job never emitted one (e.g. serialization skip) — otherwise Authorize + `Ok(())`
    // after `run_job` returned `Ok(())` would incorrectly send `{ ok: true }`.
    let mut saw_done = false;
    while let Some(payload) = rx.recv().await {
        if payload
            .get("kind")
            .and_then(|k| k.as_str())
            .is_some_and(|k| k == "done")
        {
            saw_done = true;
        }
        let _ = sink_tx.send(ServerMessage::Progress { payload });
    }
    // Channel closed → blocking task has finished

    let (result, file_b64) = handle.await.unwrap_or((
        Err(tyutool_core::FlashError::Plugin(
            "task panicked".to_string(),
        )),
        None,
    ));

    // For read mode: send file_content message before Done
    if let Some(b64) = file_b64 {
        let path_str = job.read_file_path.as_deref().unwrap_or("read.bin");
        let name = std::path::Path::new(path_str)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("read.bin");
        let payload = serde_json::json!({ "kind": "file_content", "name": name, "content": b64 });
        let _ = sink_tx.send(ServerMessage::Progress { payload });
    }

    if !saw_done {
        let (ok, message) = match &result {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };
        let done_payload = serde_json::json!({ "kind": "done", "ok": ok, "message": message });
        let _ = sink_tx.send(ServerMessage::Progress {
            payload: done_payload,
        });
    }

    // Clean up temp files
    for p in temp_paths {
        let _ = std::fs::remove_file(p);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn decode_to_temp(b64: &str, prefix: &str) -> anyhow::Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("base64 decode failed")?;
    let path = temp_path(prefix, "bin");
    std::fs::write(&path, &bytes).context("write temp file")?;
    Ok(path)
}

fn temp_path(prefix: &str, ext: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir()
        .join(format!("{prefix}_{ts}.{ext}"))
        .to_string_lossy()
        .to_string()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_list_ports() {
        let msg: ClientMessage = serde_json::from_str(r#"{"type":"list_ports"}"#).unwrap();
        assert!(matches!(msg, ClientMessage::ListPorts));
    }

    #[test]
    fn deserialize_device_reset() {
        let msg: ClientMessage = serde_json::from_str(
            r#"{"type":"device_reset","port":"/dev/ttyUSB0","chip_id":"T5AI"}"#,
        )
        .unwrap();
        assert!(matches!(msg, ClientMessage::DeviceReset { .. }));
    }

    #[test]
    fn device_reset_result_wire_type_is_snake_case() {
        let msg = ServerMessage::DeviceResetResult {
            ok: true,
            error: None,
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(
            s.contains(r#""type":"device_reset_result""#),
            "unexpected JSON (client listens for type device_reset_result): {s}"
        );
    }

    #[test]
    fn ports_message_keeps_usb_metadata() {
        let msg = ServerMessage::Ports {
            ports: vec![SerialPortEntry {
                path: "/dev/ttyACM0".into(),
                name: Some("USB Enhanced Serial".into()),
                usb_vid: Some(0x1a86),
                usb_pid: Some(0x55d2),
                usb_serial: None,
                usb_interface: None,
                port_role: None,
            }],
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""type":"ports""#), "unexpected JSON: {s}");
        assert!(
            s.contains(r#""usbVid":6790"#),
            "usb VID metadata missing: {s}"
        );
        assert!(
            s.contains(r#""usbPid":21970"#),
            "usb PID metadata missing: {s}"
        );
    }

    #[test]
    fn deserialize_cancel() {
        let msg: ClientMessage = serde_json::from_str(r#"{"type":"cancel"}"#).unwrap();
        assert!(matches!(msg, ClientMessage::Cancel));
    }

    #[test]
    fn deserialize_run_job_minimal() {
        let json = r#"{
            "type": "run_job",
            "job": {
                "mode": "erase",
                "chipId": "T5AI",
                "port": "/dev/ttyUSB0",
                "baudRate": 921600,
                "eraseStartHex": "0x00000000",
                "eraseEndHex": "0x00200000"
            }
        }"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::RunJob { .. }));
    }

    #[test]
    fn decode_to_temp_roundtrip() {
        let original = b"hello firmware";
        let b64 = base64::engine::general_purpose::STANDARD.encode(original);
        let path = decode_to_temp(&b64, "test_fw").unwrap();
        let read_back = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_back, original);
    }

    #[test]
    fn deserialize_serial_debug_open_with_config() {
        let json = r#"{
            "type": "serial_debug_open",
            "cfg": {
                "port": "/dev/ttyUSB0",
                "baudRate": 115200,
                "dataBits": "eight",
                "parity": "none",
                "stopBits": "one"
            }
        }"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::SerialDebugOpen { cfg } => {
                assert_eq!(cfg.port, "/dev/ttyUSB0");
                assert_eq!(cfg.baud_rate, 115200);
            }
            other => panic!("expected SerialDebugOpen, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_serial_debug_close_and_state() {
        let close: ClientMessage =
            serde_json::from_str(r#"{"type":"serial_debug_close"}"#).unwrap();
        assert!(matches!(close, ClientMessage::SerialDebugClose));
        let state: ClientMessage =
            serde_json::from_str(r#"{"type":"serial_debug_state"}"#).unwrap();
        assert!(matches!(state, ClientMessage::SerialDebugState));
    }

    #[test]
    fn deserialize_serial_debug_send_bytes() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"serial_debug_send","bytes":[1,2,255]}"#).unwrap();
        match msg {
            ClientMessage::SerialDebugSend { bytes } => assert_eq!(bytes, vec![1, 2, 255]),
            other => panic!("expected SerialDebugSend, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_authorize_confirm() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"authorize_confirm","confirmed":true}"#).unwrap();
        match msg {
            ClientMessage::AuthorizeConfirm { confirmed } => assert!(confirmed),
            other => panic!("expected AuthorizeConfirm, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_run_job_with_multiple_segments_and_file_contents() {
        let json = r#"{
            "type": "run_job",
            "job": {
                "mode": "flash",
                "chipId": "T5AI",
                "port": "/dev/ttyUSB0",
                "baudRate": 921600,
                "segments": [
                    {"firmwarePath": "", "startAddr": "0x00000000", "endAddr": "0x00100000"},
                    {"firmwarePath": "", "startAddr": "0x00100000", "endAddr": "0x00200000"}
                ]
            },
            "file_contents": ["YWFh", "YmJi"]
        }"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::RunJob {
                job,
                file_content,
                file_contents,
            } => {
                assert!(file_content.is_none());
                let segments = job.segments.expect("segments present");
                assert_eq!(segments.len(), 2);
                assert_eq!(segments[1].start_addr, "0x00100000");
                assert_eq!(file_contents.unwrap().len(), 2);
            }
            other => panic!("expected RunJob, got {other:?}"),
        }
    }

    #[test]
    fn serialize_serial_debug_lifecycle_messages_use_snake_case() {
        assert!(serde_json::to_string(&ServerMessage::SerialDebugOpened)
            .unwrap()
            .contains(r#""type":"serial_debug_opened""#));
        assert!(serde_json::to_string(&ServerMessage::SerialDebugClosed)
            .unwrap()
            .contains(r#""type":"serial_debug_closed""#));
        let disc = serde_json::to_string(&ServerMessage::SerialDebugDisconnected {
            reason: "device removed".into(),
        })
        .unwrap();
        assert!(disc.contains(r#""type":"serial_debug_disconnected""#));
        assert!(disc.contains(r#""reason":"device removed""#));
    }

    #[test]
    fn serialize_serial_debug_state_info_omits_cfg_when_closed() {
        let closed = serde_json::to_string(&ServerMessage::SerialDebugStateInfo {
            open: false,
            cfg: None,
        })
        .unwrap();
        assert!(closed.contains(r#""type":"serial_debug_state_info""#));
        assert!(closed.contains(r#""open":false"#));
        // skip_serializing_if = Option::is_none → cfg must be absent.
        assert!(
            !closed.contains("cfg"),
            "cfg should be omitted when None: {closed}"
        );

        let open = serde_json::to_string(&ServerMessage::SerialDebugStateInfo {
            open: true,
            cfg: Some(DebugConfig {
                port: "/dev/ttyUSB0".into(),
                baud_rate: 115200,
                data_bits: tyutool_core::DataBits::Eight,
                parity: tyutool_core::Parity::None,
                stop_bits: tyutool_core::StopBits::One,
            }),
        })
        .unwrap();
        assert!(open.contains(r#""open":true"#));
        assert!(
            open.contains(r#""baudRate":115200"#),
            "cfg should be present: {open}"
        );
    }

    #[test]
    fn serialize_error_and_chunk_messages() {
        let err = serde_json::to_string(&ServerMessage::Error {
            message: "boom".into(),
        })
        .unwrap();
        assert!(err.contains(r#""type":"error""#));
        assert!(err.contains(r#""message":"boom""#));

        let chunk = serde_json::to_string(&ServerMessage::SerialDebugChunk {
            chunk: DebugChunk {
                direction: tyutool_core::Direction::Tx,
                ts_ms: 42,
                bytes: vec![7, 8],
            },
        })
        .unwrap();
        assert!(chunk.contains(r#""type":"serial_debug_chunk""#));
        assert!(chunk.contains(r#""direction":"tx""#));
    }
}
