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

use crate::batch_auth;
use crate::logs;

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

/// Excel allocator + count of slot threads using it, guarded by ONE mutex so
/// acquire (batch_auth_start) and release (last SlotSessionGuard drop) can
/// never interleave. Invariant: `alloc.is_some() ⇒ active > 0` — the file
/// lock is held exactly while slots run, so the sheet can be edited between
/// batches and every new batch re-reads it from disk.
pub(crate) struct AllocatorSession {
    pub alloc: Option<std::sync::Arc<batch_auth::ExcelRowAllocator>>,
    pub active: usize,
}

/// Decrements the session's active count when a slot thread exits (any path,
/// including early returns and panics); the last one out releases the file
/// lock and drops the allocator.
struct SlotSessionGuard(Arc<StdMutex<AllocatorSession>>);

impl Drop for SlotSessionGuard {
    fn drop(&mut self) {
        if let Ok(mut session) = self.0.lock() {
            session.active = session.active.saturating_sub(1);
            if session.active == 0 {
                if let Some(alloc) = session.alloc.take() {
                    alloc.release_lock();
                    log::info!("[batch-auth] last slot finished; excel session closed");
                }
            }
        }
    }
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
pub(crate) struct BatchAuthStartConfig {
    chip_id: String,
    baud_rate: u32,
    auth_baud_rate: u32,
    firmware_path: Option<String>,
    flash_start_hex: Option<String>,
    flash_end_hex: Option<String>,
    excel_path: String,
    conflict_policy: String,
    auth_storage: Option<String>,
    /// false ⇒ flash-only batch: skip the Excel session and the auth step.
    #[serde(default = "default_authorize_enabled")]
    authorize_enabled: bool,
}

fn default_authorize_enabled() -> bool {
    true
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
                mode: tyutool_core::FlashMode::Flash,
                chip_id: config_clone.chip_id.clone(),
                port: port_clone.clone(),
                baud_rate: config_clone.baud_rate,
                firmware_path: Some(config_clone.firmware_path.clone()),
                segments: None,
                flash_start_hex: config_clone.flash_start_hex.clone(),
                flash_end_hex: config_clone.flash_end_hex.clone(),
                erase_start_hex: None,
                erase_end_hex: None,
                read_start_hex: None,
                read_end_hex: None,
                read_file_path: None,
                authorize_uuid: None,
                authorize_key: None,
                authorize_storage: None,
                confirm_overwrite: None,
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

/// One batch-auth slot: optional firmware flash, then the authorize pipeline,
/// for a single port. Runs on its own thread.
///
/// `emit` receives every `batch-auth-progress` payload. Taking a callback rather
/// than an `AppHandle` is what makes this callable from a test: the production
/// caller forwards to `app.emit`, while a test can record the payloads and
/// assert the slot's step sequence.
#[allow(clippy::too_many_arguments)]
fn run_batch_auth_slot(
    port: String,
    config: BatchAuthStartConfig,
    cancel: Arc<AtomicBool>,
    allocator: Option<Arc<batch_auth::ExcelRowAllocator>>,
    session: Arc<StdMutex<AllocatorSession>>,
    trace: Option<Arc<StdMutex<logs::BatchAuthTraceWriter>>>,
    conflict_policy: tyutool_core::ConflictPolicy,
    auth_storage: tyutool_core::AuthStorage,
    emit: &dyn Fn(serde_json::Value),
) {
    // Must be the first statement: every exit path (early returns,
    // panics) has to decrement the session count. Flash-only batches
    // never incremented it, so they get no guard.
    let _session_guard = allocator.is_some().then(|| SlotSessionGuard(session));
    // Last Excel write failure for this slot; attached to the final
    // progress emit so the operator sees the sheet was NOT updated.
    let excel_err: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));
    log::info!(
        "[batch-auth] slot begin  port={port} chip={}",
        config.chip_id
    );
    if let Some(ref fw_path) = config.firmware_path {
        if !fw_path.is_empty() {
            let job = tyutool_core::FlashJob {
                mode: tyutool_core::FlashMode::Flash,
                chip_id: config.chip_id.clone(),
                port: port.clone(),
                baud_rate: config.baud_rate,
                firmware_path: Some(fw_path.clone()),
                segments: None,
                flash_start_hex: config.flash_start_hex.clone(),
                flash_end_hex: config.flash_end_hex.clone(),
                erase_start_hex: None,
                erase_end_hex: None,
                read_start_hex: None,
                read_end_hex: None,
                read_file_path: None,
                authorize_uuid: None,
                authorize_key: None,
                authorize_storage: None,
                confirm_overwrite: None,
            };
            log::info!(
                "[batch-auth] flash start  port={port} chip={} firmware={fw_path}",
                config.chip_id
            );
            let port2 = port.clone();
            let flash_result = tyutool_core::run_job(&job, &cancel, |p| {
                emit(serde_json::json!({
                    "port": port2,
                    "step": "flashing",
                    "event": p
                }));
            });
            if !config.authorize_enabled {
                // Flash-only: a user cancel is not a failure — mirror
                // the auth pipeline's "cancelled" step so the slot
                // returns to idle without polluting the cumulative
                // stats. On Ok, a late cancel changes nothing: the
                // run_job Done event forwarded above is already the
                // terminal signal, so emitting a second outcome here
                // would double-count the slot.
                match flash_result {
                    Err(tyutool_core::FlashError::Cancelled) => {
                        log::info!("[batch-auth] flash cancelled  port={port}");
                        emit(serde_json::json!({
                            "port": port,
                            "step": "cancelled"
                        }));
                        return;
                    }
                    Err(e) => {
                        log::warn!("[batch-auth] flash failed  port={port} error={e}");
                        emit(serde_json::json!({
                            "port": port,
                            "step": "failed",
                            "error": e.to_string()
                        }));
                        return;
                    }
                    Ok(()) => {}
                }
            } else if flash_result.is_err() || cancel.load(Ordering::Relaxed) {
                let error = flash_result
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "cancelled".into());
                log::warn!("[batch-auth] flash failed  port={port} error={error}");
                emit(serde_json::json!({
                    "port": port,
                    "step": "failed",
                    "error": error
                }));
                return;
            }
            log::info!("[batch-auth] flash done  port={port}");
            if config.authorize_enabled {
                // Wait for the device to boot naturally after flash before the auth
                // slot issues a hardware reset. Non-fatal: times out after 3 s max.
                tyutool_core::wait_after_firmware_flash(
                    &port,
                    config.auth_baud_rate,
                    &config.chip_id,
                    &cancel,
                );
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
            }
        }
    }

    // Flash-only batch: run_job's Done event (forwarded above under
    // step "flashing") is the terminal signal for the frontend —
    // nothing to authorize, no Excel to update.
    let Some(allocator) = allocator else {
        log::info!("[batch-auth] slot done (flash-only)  port={port}");
        return;
    };

    // find_by_mac: look up Excel row by device MAC address
    let alloc_find = allocator.clone();
    let find_by_mac =
        move |mac: &str| -> Option<(usize, String, String)> { alloc_find.find_by_mac(mac) };

    // allocate_row: claim a new unused row
    let alloc_alloc = allocator.clone();
    let allocate_row = move || -> Option<(usize, String, String)> {
        match alloc_alloc.allocate_row() {
            Ok(row) => Some((row.row_idx, row.uuid, row.authkey)),
            Err(e) => {
                log::warn!("[batch-auth] allocate_row error: {e}");
                None
            }
        }
    };

    // update_row: translate BatchAuthRowUpdate → RowStatus + step_name, then write to Excel
    let alloc_update = allocator.clone();
    let port_for_update = port.clone();
    let excel_err_update = excel_err.clone();
    let update_row = move |row_idx: usize, mac: &str, update: tyutool_core::BatchAuthRowUpdate| {
        use crate::batch_auth::RowStatus;
        use tyutool_core::BatchAuthRowUpdate as U;
        let (status, step_name, error): (RowStatus, Option<&str>, Option<String>) = match update {
            U::MacRead => (RowStatus::MacRead, Some("mac_read"), None),
            U::AuthWritten => (RowStatus::AuthWritten, Some("auth_written"), None),
            U::AuthVerified => (RowStatus::AuthVerified, Some("auth_verified"), None),
            // Done: keep last step in Excel (STATUS=DONE is sufficient)
            U::Done => (RowStatus::Done, None, None),
            U::StepFailed { step, error } => {
                let status = if step == "auth_write" {
                    RowStatus::MacRead
                } else {
                    RowStatus::AuthWritten
                };
                (status, Some(step), Some(error))
            }
        };
        if let Err(e) =
            alloc_update.update_row_state(row_idx, mac, status, step_name, error.as_deref())
        {
            log::error!(
                "[batch-auth] excel-update-failed  port={port_for_update} row={row_idx} err={e}"
            );
            if let Ok(mut slot) = excel_err_update.lock() {
                *slot = Some(e);
            }
        }
    };

    let slot_config = tyutool_core::BatchAuthSlotConfig {
        auth_baud_rate: config.auth_baud_rate,
        conflict_policy,
        auth_storage,
    };
    let result = tyutool_core::run_batch_auth_slot(
        &port,
        &config.chip_id,
        &slot_config,
        find_by_mac,
        allocate_row,
        update_row,
        &cancel,
        |step| {
            let step_str = match step {
                tyutool_core::BatchAuthStep::ReadingMac => "reading_mac",
                tyutool_core::BatchAuthStep::ReadingAuth => "reading_auth",
                tyutool_core::BatchAuthStep::WritingAuth => "writing_auth",
                tyutool_core::BatchAuthStep::Verifying => "verifying",
            };
            emit(serde_json::json!({ "port": port, "step": step_str }));
        },
        |line: &str| {
            if let Some(tw) = &trace {
                if let Ok(mut w) = tw.lock() {
                    w.writeln(line);
                }
            }
        },
    );

    // Attach the captured Excel write failure (if any) to a final
    // emit — the frontend renders it as a per-slot warning.
    let emit_final = |mut payload: serde_json::Value| {
        if let Some(e) = excel_err.lock().ok().and_then(|g| g.clone()) {
            payload["excelError"] = serde_json::Value::String(e);
        }
        emit(payload);
    };

    match result {
        // Done — state already written to Excel by update_row(Done)
        Ok(tyutool_core::BatchAuthSlotResult::Done { mac }) => {
            log::info!("[batch-auth] slot done  port={port} mac={mac}");
            emit_final(serde_json::json!({ "port": port, "step": "done", "mac": mac }));
        }
        // AlreadyDone — state already written to Excel by update_row(Done) in authorize.rs
        Ok(tyutool_core::BatchAuthSlotResult::AlreadyDone { mac }) => {
            log::info!("[batch-auth] slot already-done  port={port} mac={mac}");
            emit_final(serde_json::json!({ "port": port, "step": "done", "mac": mac }));
        }
        // InsufficientCodes — no row was allocated, nothing to write
        Ok(tyutool_core::BatchAuthSlotResult::InsufficientCodes { mac }) => {
            log::info!("[batch-auth] slot no-code  port={port} mac={mac}");
            emit(serde_json::json!({ "port": port, "step": "no_code", "mac": mac }));
        }
        // Skipped — device already carries auth we didn't (knowingly) write.
        // If that UUID is still an unclaimed row in our sheet, claim it for this
        // MAC so the same code can't later be handed to a different device.
        Ok(tyutool_core::BatchAuthSlotResult::Skipped { mac, existing_uuid }) => {
            log::info!(
                "[batch-auth] slot skipped  port={port} mac={mac} existing_uuid={existing_uuid}"
            );
            if let Err(e) = allocator.confirm_existing_uuid(&existing_uuid, &mac) {
                log::error!("[batch-auth] excel-confirm-skipped-failed  port={port} existing_uuid={existing_uuid} err={e}");
                if let Ok(mut slot) = excel_err.lock() {
                    *slot = Some(e);
                }
            }
            emit_final(
                serde_json::json!({ "port": port, "step": "skipped", "mac": mac, "existingUuid": existing_uuid }),
            );
        }
        // Cancelled — pre-write cancel; if MacRead was written, row stays in MacRead state (recoverable)
        Ok(tyutool_core::BatchAuthSlotResult::Cancelled) => {
            log::info!("[batch-auth] slot cancelled  port={port}");
            emit(serde_json::json!({ "port": port, "step": "cancelled" }));
        }
        // CancelledAfterWrite — state already written to Excel by update_row(AuthWritten)
        Ok(tyutool_core::BatchAuthSlotResult::CancelledAfterWrite { mac, uuid }) => {
            log::warn!(
                "[batch-auth] slot cancelled AFTER auth_write  port={port} mac={mac} uuid={uuid}"
            );
            emit_final(
                serde_json::json!({ "port": port, "step": "cancelled_after_write", "mac": mac, "uuid": uuid }),
            );
        }
        // DefaultMac — T5/T5AI factory default MAC; no row allocation
        Ok(tyutool_core::BatchAuthSlotResult::DefaultMac { mac }) => {
            log::warn!("[batch-auth] slot default-mac  port={port} mac={mac}");
            emit(serde_json::json!({ "port": port, "step": "default_mac", "mac": mac }));
        }
        // Err — state already written by update_row(StepFailed) inside authorize.rs
        Err(e) => {
            log::warn!("[batch-auth] slot failed  port={port} error={e}");
            emit_final(
                serde_json::json!({ "port": port, "step": "failed", "error": e.to_string() }),
            );
        }
    }
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
            match logs::BatchAuthTraceWriter::open(&log_dir, &ts) {
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
        logs::prune_trace_files(&log_dir);
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
            run_batch_auth_slot(
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
mod tests {
    use super::*;

    /// The point of `run_batch_auth_slot` taking an `emit` callback instead of an
    /// `AppHandle`: the slot body can be driven from a test. This covers the
    /// flash-only-with-nothing-to-do path, which touches no serial port.
    #[test]
    fn flash_only_slot_without_firmware_or_allocator_emits_nothing() {
        let seen: Arc<StdMutex<Vec<serde_json::Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let sink = seen.clone();

        run_batch_auth_slot(
            "COM_TEST".to_string(),
            BatchAuthStartConfig {
                chip_id: "T5AI".into(),
                baud_rate: 921_600,
                auth_baud_rate: 115_200,
                firmware_path: None,
                flash_start_hex: None,
                flash_end_hex: None,
                excel_path: String::new(),
                conflict_policy: "skip".into(),
                auth_storage: None,
                authorize_enabled: false,
            },
            Arc::new(AtomicBool::new(false)),
            None,
            Arc::new(StdMutex::new(AllocatorSession {
                alloc: None,
                active: 0,
            })),
            None,
            tyutool_core::ConflictPolicy::Skip,
            tyutool_core::AuthStorage::Kv,
            &move |payload| sink.lock().unwrap().push(payload),
        );

        assert!(
            seen.lock().unwrap().is_empty(),
            "a flash-only slot with no firmware has nothing to report"
        );
    }

    #[test]
    fn batch_auth_start_config_defaults_authorize_enabled_to_true() {
        let cfg: BatchAuthStartConfig = serde_json::from_str(
            r#"{"chipId":"esp32","baudRate":921600,"authBaudRate":115200,
                "excelPath":"codes.xlsx","conflictPolicy":"skip"}"#,
        )
        .unwrap();
        assert!(cfg.authorize_enabled);

        let cfg: BatchAuthStartConfig = serde_json::from_str(
            r#"{"chipId":"esp32","baudRate":921600,"authBaudRate":115200,
                "excelPath":"","conflictPolicy":"skip","authorizeEnabled":false}"#,
        )
        .unwrap();
        assert!(!cfg.authorize_enabled);
    }
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
