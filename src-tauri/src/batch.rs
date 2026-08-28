//! Batch flash and batch authorize: one worker thread per serial port.
//!
//! The Excel allocator is shared by every slot in a run and guarded by a single
//! mutex (`AllocatorSession`) so acquiring it in `batch_auth_start` and
//! releasing it from the last `SlotSessionGuard` drop cannot interleave. The
//! invariant `alloc.is_some() => active > 0` means the sheet is locked exactly
//! while slots run, so it can be edited between batches.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread::JoinHandle;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};

use tyutool_core::batch_auth;
use tyutool_core::batch_slot::{run_batch_slot, AllocatorSession, BatchAuthStartConfig};

pub(crate) struct BatchSlot {
    pub cancel: Arc<AtomicBool>,
    pub thread: JoinHandle<()>,
}

pub(crate) struct BatchFlashState {
    /// key = port name (OS-native format, as received from frontend)
    pub slots: StdMutex<HashMap<String, BatchSlot>>,
}

pub(crate) struct BatchAuthState {
    pub slots: StdMutex<HashMap<String, BatchSlot>>,
    pub session: Arc<StdMutex<AllocatorSession>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchFlashStartConfig {
    chip_id: String,
    baud_rate: u32,
    firmware_path: String,
    flash_start_hex: Option<String>,
    flash_end_hex: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchAuthReadConfig {
    chip_id: String,
    baud_rate: u32,
    auth_storage: Option<String>,
}

#[tauri::command]
pub(crate) fn batch_flash_start(
    app: AppHandle,
    state: State<'_, BatchFlashState>,
    config: BatchFlashStartConfig,
    ports: Vec<String>,
) -> Result<(), String> {
    log::info!(
        "[batch-flash] start chip={} ports_count={} ports={:?}",
        config.chip_id,
        ports.len(),
        ports
    );
    for port in ports {
        // Step 1: Remove old slot under lock (brief)
        let old_slot = {
            let mut slots = state.slots.lock().map_err(|e| e.to_string())?;
            slots.remove(&port)
        };

        // Step 2: Wait for old thread OUTSIDE the lock
        if let Some(old) = old_slot {
            old.cancel.store(true, Ordering::SeqCst);
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                let _ = old.thread.join();
                let _ = tx.send(());
            });
            if rx.recv_timeout(Duration::from_secs(3)).is_err() {
                return Err(format!(
                    "port {} previous operation not stopped; retry in a few seconds",
                    port
                ));
            }
        }

        // Step 3: Spawn thread (outside lock)
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel.clone();
        let app_clone = app.clone();
        let port_clone = port.clone();
        let config_clone = config.clone();

        let handle = std::thread::spawn(move || {
            log::info!("[batch-flash] slot begin  port={}", port_clone);
            if !std::path::Path::new(&config_clone.firmware_path).exists() {
                let _ = app_clone.emit(
                    "batch-flash-progress",
                    serde_json::json!({
                        "port": port_clone,
                        "event": { "kind": "done", "result": { "err": { "message": "firmware file not found", "elapsed_secs": 0 } } }
                    }),
                );
                log::warn!(
                    "[batch-flash] slot failed  port={} reason=firmware_not_found path={}",
                    port_clone,
                    config_clone.firmware_path
                );
                return;
            }

            let job = tyutool_core::FlashJob {
                firmware_path: Some(config_clone.firmware_path.clone()),
                flash_start_hex: config_clone.flash_start_hex.clone(),
                flash_end_hex: config_clone.flash_end_hex.clone(),
                ..tyutool_core::FlashJob::new(
                    tyutool_core::FlashMode::Flash,
                    config_clone.chip_id.clone(),
                    port_clone.clone(),
                    config_clone.baud_rate,
                )
            };

            let result = tyutool_core::run_job(&job, &cancel_clone, |p| {
                let _ = app_clone.emit(
                    "batch-flash-progress",
                    serde_json::json!({ "port": port_clone, "event": p }),
                );
            });
            match &result {
                Ok(()) => log::info!("[batch-flash] slot done    port={}", port_clone),
                Err(tyutool_core::FlashError::Cancelled) => {
                    log::info!("[batch-flash] slot cancelled port={}", port_clone)
                }
                Err(e) => log::warn!(
                    "[batch-flash] slot failed   port={} error={}",
                    port_clone,
                    e
                ),
            }
        });

        // Step 4: Insert under lock (brief)
        {
            let mut slots = state.slots.lock().map_err(|e| e.to_string())?;
            slots.insert(
                port,
                BatchSlot {
                    cancel,
                    thread: handle,
                },
            );
        }
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn batch_flash_cancel_port(
    state: State<'_, BatchFlashState>,
    port: String,
) -> Result<(), String> {
    log::info!("[batch-flash] cancel port={}", port);
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    if let Some(slot) = slots.get(&port) {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn batch_flash_cancel_all(state: State<'_, BatchFlashState>) -> Result<(), String> {
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    let count = slots.len();
    for slot in slots.values() {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    log::info!("[batch-flash] cancel all count={}", count);
    Ok(())
}

/// Validate that `path` exists and has an `.xlsx` extension, returning the
/// borrowed path on success. Pure: the existence + extension branches are the
/// testable part; the happy path delegates to the already-tested loader.
fn validate_excel_file(path: &str) -> Result<&std::path::Path, String> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err("excel.fileNotFound".into());
    }
    if p.extension().and_then(|e| e.to_str()) != Some("xlsx") {
        return Err("excel.notXlsxFormat".into());
    }
    Ok(p)
}

#[tauri::command]
pub(crate) fn validate_excel_cmd(path: String) -> Result<batch_auth::ExcelStats, String> {
    let p = validate_excel_file(&path)?;
    let alloc = batch_auth::ExcelRowAllocator::load(p)?;
    let stats = alloc.stats();
    log::info!(
        "[batch-auth] excel validated: path={} total={} used={} remaining={}",
        path,
        stats.total,
        stats.used,
        stats.remaining,
    );
    Ok(stats)
}

#[tauri::command]
pub(crate) fn batch_auth_start(
    app: AppHandle,
    state: State<'_, BatchAuthState>,
    config: BatchAuthStartConfig,
    ports: Vec<String>,
) -> Result<(), String> {
    let conflict_policy = match config.conflict_policy.as_str() {
        "overwrite" => tyutool_core::ConflictPolicy::Overwrite,
        _ => tyutool_core::ConflictPolicy::Skip,
    };
    let auth_storage = match config.auth_storage.as_deref() {
        Some("otp") => tyutool_core::AuthStorage::Otp,
        _ => tyutool_core::AuthStorage::Kv,
    };

    // A flash-only batch (authorization off) with no firmware would do nothing.
    if !config.authorize_enabled && config.firmware_path.as_deref().is_none_or(|p| p.is_empty()) {
        return Err("authorization disabled and no firmware to flash".into());
    }

    log::info!(
        "[batch-auth] batch-start: chip={} excel={} firmware={} slots={} storage={:?} conflict={} authorize={}",
        config.chip_id,
        config.excel_path,
        config.firmware_path.as_deref().unwrap_or("(none)"),
        ports.len(),
        auth_storage,
        config.conflict_policy,
        config.authorize_enabled,
    );

    // Phase 1: cancel all old slots simultaneously, collect their join receivers.
    let mut old_join_rxs: Vec<(String, std::sync::mpsc::Receiver<()>)> = Vec::new();
    for port in &ports {
        let old_slot = {
            let mut slots = state.slots.lock().map_err(|e| e.to_string())?;
            slots.remove(port)
        };
        if let Some(old) = old_slot {
            old.cancel.store(true, Ordering::SeqCst);
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                let _ = old.thread.join();
                let _ = tx.send(());
            });
            old_join_rxs.push((port.clone(), rx));
        }
    }

    // Phase 2: wait for all old threads with a shared 3-second deadline.
    if !old_join_rxs.is_empty() {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        for (port, rx) in &old_join_rxs {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if rx.recv_timeout(remaining).is_err() {
                return Err(format!("port {} not stopped; retry in a few seconds", port));
            }
        }
    }

    // Acquire the Excel session AFTER the old slots are fully joined (their
    // guards decrement the counter) and with nothing fallible left before
    // spawn (an increment must never leak). Single lock over {alloc, active}.
    // A flash-only batch never touches the sheet: no session, no file lock.
    let allocator = if !config.authorize_enabled {
        None
    } else {
        let path = std::path::Path::new(&config.excel_path);
        let mut session = state.session.lock().map_err(|e| e.to_string())?;
        let alloc = match &session.alloc {
            // Slots are still running: the file has been locked the whole
            // time, so the in-memory state cannot be stale — reuse it.
            Some(a) if a.path_matches(path) => a.clone(),
            // Running slots write to the OLD file; swapping allocators
            // mid-run would corrupt the release invariant. Refuse.
            Some(_) => return Err("excel.changedWhileRunning".into()),
            // Idle: fresh locked load, picking up any manual edits made
            // since the previous batch. Fails fast with "excel.locked" if
            // another program holds the file open for writing.
            None => {
                let a = std::sync::Arc::new(batch_auth::ExcelRowAllocator::load_locked(path)?);
                session.alloc = Some(a.clone());
                a
            }
        };
        session.active += ports.len();
        Some(alloc)
    };

    // Phase 3: spawn threads — auth code allocation happens lazily inside each thread,
    // after reading the device's existing auth status.
    //
    // One shared .trace writer per batch run captures plaintext verify data
    // (UUID/AuthKey comparison values) for local diagnosis. The file is
    // `batch-auth-<ts>.trace` — never collected into any export/archive zip.
    let trace_writer = match app.path().app_log_dir() {
        Ok(log_dir) => {
            let ts = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
            match tyutool_core::BatchAuthTraceWriter::open(&log_dir, &ts) {
                Ok(w) => Some(std::sync::Arc::new(std::sync::Mutex::new(w))),
                Err(e) => {
                    log::warn!("[batch-auth] trace writer unavailable: {e}");
                    None
                }
            }
        }
        Err(e) => {
            log::warn!("[batch-auth] log dir unavailable, trace disabled: {e}");
            None
        }
    };
    // Trim old .trace files alongside the run (bounded growth, independent of .log pruning).
    if let Some(log_dir) = trace_writer
        .as_ref()
        .and_then(|_| app.path().app_log_dir().ok())
    {
        tyutool_core::prune_trace_files(&log_dir);
    }
    for port in ports {
        // Set up cancel + spawn thread
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_clone = cancel.clone();
        let app_clone = app.clone();
        let port_clone = port.clone();
        let config_clone = config.clone();
        let alloc_clone = allocator.clone();
        let session_clone = state.session.clone();
        let trace_clone = trace_writer.clone();

        let handle = std::thread::spawn(move || {
            run_batch_slot(
                port_clone,
                config_clone,
                cancel_clone,
                alloc_clone,
                session_clone,
                trace_clone,
                conflict_policy,
                auth_storage,
                &|payload| {
                    let _ = app_clone.emit("batch-auth-progress", payload);
                },
            )
        });

        // 5. Insert new slot (under lock, briefly)
        {
            let mut slots = state.slots.lock().map_err(|e| e.to_string())?;
            slots.insert(
                port,
                BatchSlot {
                    cancel,
                    thread: handle,
                },
            );
        }
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn batch_auth_cancel_port(
    state: State<'_, BatchAuthState>,
    port: String,
) -> Result<(), String> {
    log::info!("[batch-auth] cancel port={}", port);
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    if let Some(slot) = slots.get(&port) {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn batch_auth_cancel_all(state: State<'_, BatchAuthState>) -> Result<(), String> {
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    let count = slots.len();
    for slot in slots.values() {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    log::info!("[batch-auth] cancel all count={}", count);
    Ok(())
}

#[tauri::command]
pub(crate) fn batch_auth_read_ports(
    app: AppHandle,
    state: State<'_, BatchAuthState>,
    config: BatchAuthReadConfig,
    ports: Vec<String>,
) -> Result<(), String> {
    let auth_storage = match config.auth_storage.as_deref() {
        Some("otp") => tyutool_core::AuthStorage::Otp,
        _ => tyutool_core::AuthStorage::Kv,
    };

    // Phase 1: cancel any running slot on each port and collect join receivers.
    let mut old_join_rxs: Vec<(String, std::sync::mpsc::Receiver<()>)> = Vec::new();
    for port in &ports {
        let old_slot = {
            let mut slots = state.slots.lock().map_err(|e| e.to_string())?;
            slots.remove(port)
        };
        if let Some(old) = old_slot {
            old.cancel.store(true, Ordering::SeqCst);
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                let _ = old.thread.join();
                let _ = tx.send(());
            });
            old_join_rxs.push((port.clone(), rx));
        }
    }

    // Phase 2: wait for old threads with a shared 3-second deadline.
    if !old_join_rxs.is_empty() {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        for (port, rx) in &old_join_rxs {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if rx.recv_timeout(remaining).is_err() {
                return Err(format!("port {} not stopped; retry in a few seconds", port));
            }
        }
    }

    // Phase 3: spawn one thread per port.
    for port in ports {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel.clone();
        let app_clone = app.clone();
        let port_clone = port.clone();
        let config_clone = config.clone();

        let handle = std::thread::spawn(move || {
            log::info!("[batch-auth-read] slot begin port={}", port_clone);
            let result = tyutool_core::read_auth_probe(
                &port_clone,
                &config_clone.chip_id,
                config_clone.baud_rate,
                auth_storage,
                &cancel_clone,
            );
            match result {
                Ok(r) => {
                    let _ = app_clone.emit(
                        "batch-auth-read-progress",
                        serde_json::json!({
                            "port": port_clone,
                            "step": "done",
                            "mac":  r.mac,
                            "uuid": r.uuid,
                        }),
                    );
                    log::info!(
                        "[batch-auth-read] done port={} mac={}",
                        port_clone,
                        r.mac.as_deref().unwrap_or("")
                    );
                }
                Err(tyutool_core::FlashError::Cancelled) => {
                    let _ = app_clone.emit(
                        "batch-auth-read-progress",
                        serde_json::json!({ "port": port_clone, "step": "cancelled" }),
                    );
                    log::info!("[batch-auth-read] cancelled port={}", port_clone);
                }
                Err(e) => {
                    let _ = app_clone.emit(
                        "batch-auth-read-progress",
                        serde_json::json!({
                            "port": port_clone,
                            "step": "failed",
                            "error": e.to_string(),
                        }),
                    );
                    log::warn!("[batch-auth-read] failed port={} error={}", port_clone, e);
                }
            }
        });

        let mut slots = state.slots.lock().map_err(|e| e.to_string())?;
        slots.insert(
            port,
            BatchSlot {
                cancel,
                thread: handle,
            },
        );
    }

    Ok(())
}

#[cfg(test)]
mod validate_excel_tests {
    use super::*;

    #[test]
    fn validate_excel_file_rejects_missing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.xlsx");
        let err = validate_excel_file(missing.to_str().unwrap()).unwrap_err();
        assert_eq!(err, "excel.fileNotFound");
    }

    #[test]
    fn validate_excel_file_rejects_wrong_extension() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("data.csv");
        std::fs::write(&p, b"x").unwrap();
        let err = validate_excel_file(p.to_str().unwrap()).unwrap_err();
        assert_eq!(err, "excel.notXlsxFormat");
    }

    #[test]
    fn validate_excel_file_accepts_existing_xlsx() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("book.xlsx");
        std::fs::write(&p, b"x").unwrap();
        let ok = validate_excel_file(p.to_str().unwrap()).unwrap();
        assert_eq!(ok, p.as_path());
    }
}
