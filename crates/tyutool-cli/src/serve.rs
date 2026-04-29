//! WebSocket dev-serve mode for tyutool-cli.
//! Exposes serial port operations over a local WebSocket so the Vite dev
//! server (localhost:1420) can flash real devices without the Tauri shell.
//!
//! Usage: tyutool-cli serve [--port 9527]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Context;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tyutool_core::{device_reset_dtr_rts, list_serial_ports, run_job, FlashJob, SerialPortEntry};

// ── Client → Server ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
}

// ── Server → Client ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ports { ports: Vec<SerialPortEntry> },
    DeviceResetResult {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Progress { payload: serde_json::Value },
    Error { message: String },
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

    let (mut sink, mut stream) = ws.split();
    let cancel = Arc::new(AtomicBool::new(false));

    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                send_msg(
                    &mut sink,
                    &ServerMessage::Error {
                        message: e.to_string(),
                    },
                )
                .await;
                continue;
            }
        };

        match client_msg {
            ClientMessage::ListPorts => {
                let ports = list_serial_ports().unwrap_or_default();
                send_msg(&mut sink, &ServerMessage::Ports { ports }).await;
            }
            ClientMessage::DeviceReset { port, chip_id } => {
                // Run on blocking pool so serial `open()` / DTR/RTS never stalls the WS task.
                let join = tokio::task::spawn_blocking(move || device_reset_dtr_rts(&port, &chip_id));
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
                send_msg(&mut sink, &msg).await;
            }
            ClientMessage::Cancel => {
                cancel.store(true, Ordering::Relaxed);
            }
            ClientMessage::RunJob {
                mut job,
                file_content,
                file_contents,
            } => {
                cancel.store(false, Ordering::Relaxed);
                handle_run_job(&mut sink, Arc::clone(&cancel), &mut job, file_content, file_contents).await;
            }
        }
    }

    log::info!("WS connection closed");
}

// ── Run job handler ──────────────────────────────────────────────────────────

async fn handle_run_job(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        Message,
    >,
    cancel: Arc<AtomicBool>,
    job: &mut FlashJob,
    file_content: Option<String>,
    file_contents: Option<Vec<String>>,
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
                send_msg(sink, &ServerMessage::Error { message: e.to_string() }).await;
                return;
            }
        }
    }

    // Decode multiple base64 firmwares for multi-segment flashing
    if let Some(contents) = file_contents {
        if let Some(ref mut segments) = job.segments {
            if contents.len() != segments.len() {
                send_msg(sink, &ServerMessage::Error { message: "file_contents length mismatch with segments".into() }).await;
                return;
            }
            for (i, b64) in contents.iter().enumerate() {
                match decode_to_temp(b64, &format!("tyutool_seg_{i}")) {
                    Ok(p) => {
                        segments[i].firmware_path = p.clone();
                        temp_paths.push(p);
                    }
                    Err(e) => {
                        send_msg(sink, &ServerMessage::Error { message: e.to_string() }).await;
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

    let job_clone = job.clone();
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
        send_msg(sink, &ServerMessage::Progress { payload }).await;
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
        send_msg(sink, &ServerMessage::Progress { payload }).await;
    }

    if !saw_done {
        let (ok, message) = match &result {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };
        let done_payload = serde_json::json!({ "kind": "done", "ok": ok, "message": message });
        send_msg(
            sink,
            &ServerMessage::Progress {
                payload: done_payload,
            },
        )
        .await;
    }

    // Clean up temp files
    for p in temp_paths {
        let _ = std::fs::remove_file(p);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn send_msg(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        Message,
    >,
    msg: &ServerMessage,
) {
    if let Ok(json) = serde_json::to_string(msg) {
        let _ = sink.send(Message::Text(json.into())).await;
    }
}

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
            r#"{"type":"device_reset","port":"/dev/ttyUSB0","chip_id":"T5"}"#,
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
        assert!(s.contains(r#""usbVid":6790"#), "usb VID metadata missing: {s}");
        assert!(s.contains(r#""usbPid":21970"#), "usb PID metadata missing: {s}");
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
                "chipId": "T5",
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
}
