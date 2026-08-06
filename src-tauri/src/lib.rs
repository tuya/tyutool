mod batch_auth;
mod logs;
mod serial_debug;
mod updater;
mod window;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::thread::JoinHandle;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};
use tyutool_core::SerialDebugGeneration;

/// Set once at startup; included in exported issue-report metadata.
static SESSION_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();

struct FlashState {
    /// Cancel signal for the **current** operation. Wrapped in a Mutex so
    /// flash_run can atomically swap in a fresh Arc for the new operation while
    /// leaving the old Arc signalled — preventing the old thread's cancel from
    /// being cleared when a new operation starts.
    cancel: StdMutex<Arc<AtomicBool>>,
    /// Handle to the running flash thread, if any. Joined (with 3 s timeout)
    /// before a new operation spawns, ensuring the serial port is fully
    /// released. On timeout the new operation is rejected rather than racing.
    thread: StdMutex<Option<JoinHandle<()>>>,
}

struct BatchSlot {
    cancel: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

struct BatchFlashState {
    /// key = port name (OS-native format, as received from frontend)
    slots: StdMutex<HashMap<String, BatchSlot>>,
}

struct BatchAuthState {
    slots: StdMutex<HashMap<String, BatchSlot>>,
    session: Arc<StdMutex<AllocatorSession>>,
}

/// Excel allocator + count of slot threads using it, guarded by ONE mutex so
/// acquire (batch_auth_start) and release (last SlotSessionGuard drop) can
/// never interleave. Invariant: `alloc.is_some() ⇒ active > 0` — the file
/// lock is held exactly while slots run, so the sheet can be edited between
/// batches and every new batch re-reads it from disk.
struct AllocatorSession {
    alloc: Option<std::sync::Arc<batch_auth::ExcelRowAllocator>>,
    active: usize,
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

/// Bridges an in-progress `run_authorize` blocking thread with the frontend's
/// overwrite-confirmation dialog. The sender is set by `flash_run` before
/// blocking; `authorize_confirm_cmd` resolves it with the user's choice.
struct ConfirmState {
    sender: Arc<StdMutex<Option<std::sync::mpsc::Sender<bool>>>>,
}

/// Detect the installation type at runtime based on the executable's path.
///
/// Returns a human-readable string like `"nsis"`, `"msi"`, `"portable"`,
/// `"deb/rpm"`, `"AppImage"`, `"dmg (.app bundle)"`, etc.
fn detect_install_type() -> String {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return "unknown (current_exe failed)".into(),
    };
    let exe_str = exe.to_string_lossy();

    #[cfg(target_os = "linux")]
    {
        // AppImage injects $APPIMAGE at runtime — definitive signal
        if std::env::var("APPIMAGE").is_ok() {
            return "AppImage".into();
        }
        // deb/rpm install to /usr/... or /opt/...
        if exe_str.starts_with("/usr/") || exe_str.starts_with("/opt/") {
            return "deb/rpm (installed)".into();
        }
        format!("portable ({})", exe_str)
    }

    #[cfg(target_os = "macos")]
    {
        // .app bundle: .../Foo.app/Contents/MacOS/binary
        if exe_str.contains(".app/Contents/MacOS/") {
            if exe_str.starts_with("/Applications/") {
                return "dmg (.app, /Applications)".into();
            }
            return format!("dmg (.app, {})", exe.parent().unwrap_or(&exe).display());
        }
        format!("portable ({})", exe_str)
    }

    #[cfg(target_os = "windows")]
    {
        // Normalize a Windows path string: strip \\?\ extended-length prefix,
        // convert forward slashes to backslashes, and lowercase.
        fn normalize_win_path(s: &str) -> String {
            s.to_lowercase()
                .replace('/', "\\")
                .trim_start_matches("\\\\?\\")
                .to_string()
        }

        let exe_norm = normalize_win_path(&exe_str);
        log::debug!("[InstallType] exe_norm = {}", exe_norm);

        // 1. MSI per-user: %LOCALAPPDATA%\Programs\{AppName}\
        //    Must be checked BEFORE the generic LOCALAPPDATA check below,
        //    because Tauri per-user MSI installs under Programs\ subdirectory.
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let local_norm = normalize_win_path(&local);
            log::debug!("[InstallType] LOCALAPPDATA = {}", local_norm);
            let msi_peruser_prefix = format!("{}\\programs\\", local_norm);
            if exe_norm.starts_with(&msi_peruser_prefix) {
                return "msi (installed, per-user)".into();
            }
            // 2. NSIS default: %LOCALAPPDATA%\{AppName}\
            if exe_norm.starts_with(&local_norm) {
                return "nsis (installed)".into();
            }
        }

        // 3. MSI per-machine: %PROGRAMFILES%\, %PROGRAMW6432%\, or %PROGRAMFILES(X86)%\
        //    PROGRAMW6432 always points to the native 64-bit Program Files folder,
        //    even when running inside a 32-bit (WOW64) process.
        for var in &["PROGRAMW6432", "PROGRAMFILES", "PROGRAMFILES(X86)"] {
            if let Ok(pf) = std::env::var(var) {
                let pf_norm = normalize_win_path(&pf);
                log::debug!("[InstallType] {} = {}", var, pf_norm);
                if !pf_norm.is_empty() && exe_norm.starts_with(&pf_norm) {
                    return "msi (installed)".into();
                }
            }
        }

        format!("portable ({})", exe_str)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        format!("unknown ({})", exe_str)
    }
}

#[tauri::command]
fn list_serial_ports_cmd() -> Result<Vec<tyutool_core::SerialPortEntry>, String> {
    let ports = tyutool_core::list_serial_ports().map_err(|e| {
        log::error!("[Serial] Failed to enumerate ports: {}", e);
        e.to_string()
    })?;
    log::info!(
        "[Serial] Scan found {} port(s): [{}]",
        ports.len(),
        ports
            .iter()
            .map(|p| p.path.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(ports)
}

#[tauri::command]
fn flash_run(
    app: AppHandle,
    state: State<'_, FlashState>,
    confirm_state: State<'_, ConfirmState>,
    mut job: tyutool_core::FlashJob,
) -> Result<(), String> {
    log::info!(
        "[Flash] Starting operation: mode={:?}, chip={}, port={}, baud={}",
        job.mode,
        job.chip_id,
        job.port,
        job.baud_rate
    );

    // Clear any stale confirm sender from a previous run that exited abnormally.
    if let Ok(mut g) = confirm_state.sender.lock() {
        *g = None;
    }

    // Create a fresh cancel flag for this operation and signal the old one.
    // Swapping atomically under the mutex ensures the old Arc stays `true`
    // while the new Arc starts at `false` — the two operations never share a
    // cancel flag, so starting a new job cannot un-cancel the previous one.
    let new_cancel = Arc::new(AtomicBool::new(false));
    let old_cancel = {
        let mut guard = state.cancel.lock().map_err(|e| e.to_string())?;
        std::mem::replace(&mut *guard, new_cancel.clone())
    };
    old_cancel.store(true, Ordering::SeqCst);

    // Wait up to 3 seconds for the previous thread to exit.  If it hasn't
    // finished — e.g. blocked on a serial read with a long timeout — reject
    // the new request rather than racing on the same port.
    {
        let mut guard = state.thread.lock().map_err(|e| e.to_string())?;
        if let Some(prev) = guard.take() {
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                let _ = prev.join();
                let _ = tx.send(());
            });
            if rx.recv_timeout(Duration::from_secs(3)).is_err() {
                return Err(
                    "previous flash operation has not stopped yet; wait a few seconds and retry"
                        .into(),
                );
            }
        }
    }

    let cancel = new_cancel;

    // Inject confirm callback: blocks until the frontend calls authorize_confirm_cmd.
    // AuthConflict is already emitted by core's progress callback; no need to re-emit here.
    let confirm_sender = Arc::clone(&confirm_state.inner().sender);
    job.confirm_overwrite = Some(Box::new(move |_existing_uuid, _existing_authkey| {
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        {
            let mut guard = confirm_sender.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(tx);
        }
        rx.recv().unwrap_or(false)
    }));

    let app = app.clone();
    let handle = std::thread::spawn(move || {
        let _ = tyutool_core::run_job(&job, &cancel, |p| {
            let _ = app.emit("flash-progress", &p);
        });
    });

    *state.thread.lock().map_err(|e| e.to_string())? = Some(handle);
    Ok(())
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchFlashStartConfig {
    chip_id: String,
    baud_rate: u32,
    firmware_path: String,
    flash_start_hex: Option<String>,
    flash_end_hex: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchAuthStartConfig {
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
struct BatchAuthReadConfig {
    chip_id: String,
    baud_rate: u32,
    auth_storage: Option<String>,
}

#[tauri::command]
fn batch_flash_start(
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
fn batch_flash_cancel_port(state: State<'_, BatchFlashState>, port: String) -> Result<(), String> {
    log::info!("[batch-flash] cancel port={}", port);
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    if let Some(slot) = slots.get(&port) {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn batch_flash_cancel_all(state: State<'_, BatchFlashState>) -> Result<(), String> {
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
fn validate_excel_cmd(path: String) -> Result<batch_auth::ExcelStats, String> {
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
fn batch_auth_start(
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
fn batch_auth_cancel_port(state: State<'_, BatchAuthState>, port: String) -> Result<(), String> {
    log::info!("[batch-auth] cancel port={}", port);
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    if let Some(slot) = slots.get(&port) {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn batch_auth_cancel_all(state: State<'_, BatchAuthState>) -> Result<(), String> {
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    let count = slots.len();
    for slot in slots.values() {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    log::info!("[batch-auth] cancel all count={}", count);
    Ok(())
}

#[tauri::command]
fn batch_auth_read_ports(
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

#[tauri::command]
fn flash_cancel(state: State<'_, FlashState>, confirm_state: State<'_, ConfirmState>) {
    log::info!("[Flash] User cancelled operation");
    if let Ok(guard) = state.cancel.lock() {
        guard.store(true, Ordering::SeqCst);
    }
    // Wake any thread blocked in confirm_overwrite so it can return Cancelled.
    if let Ok(mut sender_guard) = confirm_state.sender.lock() {
        if let Some(tx) = sender_guard.take() {
            let _ = tx.send(false);
        }
    }
}

/// Resolve a pending overwrite-confirmation from `run_authorize`.
/// Called by the frontend after the user responds to the AuthConflict dialog.
#[tauri::command]
fn authorize_confirm_cmd(state: State<'_, ConfirmState>, confirmed: bool) -> Result<(), String> {
    let mut guard = state.sender.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = guard.take() {
        let _ = tx.send(confirmed);
        log::info!("[auth] confirm resolved: {}", confirmed);
        Ok(())
    } else {
        log::warn!("[auth] confirm called with no pending authorization");
        Err("no pending authorization confirmation".into())
    }
}

#[tauri::command]
fn get_file_size(path: String) -> Result<u64, String> {
    let size = std::fs::metadata(&path)
        .map(|m| m.len())
        .map_err(|e| format!("cannot stat '{}': {}", path, e))?;
    log::debug!("[File] get_file_size: path={}, size={}", path, size);
    Ok(size)
}

#[tauri::command]
async fn check_port_available_cmd(port: String) -> tyutool_core::PortCheckResult {
    match tauri::async_runtime::spawn_blocking(move || tyutool_core::check_port_available(&port))
        .await
    {
        Ok(result) => {
            log::debug!(
                "[Serial] check_port_available: available={}",
                result.available
            );
            result
        }
        Err(_) => tyutool_core::PortCheckResult {
            available: false,
            error_message: Some("Port check task panicked".to_string()),
            process_info: None,
            kill_hint: None,
        },
    }
}

#[tauri::command]
fn check_file_exists(path: String) -> bool {
    let exists = std::path::Path::new(&path).exists();
    log::debug!("[File] check_file_exists: path={}, exists={}", path, exists);
    exists
}

#[tauri::command]
fn get_install_type() -> String {
    detect_install_type()
}

#[tauri::command]
fn set_log_level(level: String) -> Result<(), String> {
    let filter = match level.as_str() {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        "off" => log::LevelFilter::Off,
        _ => return Err(format!("Invalid log level: {}", level)),
    };
    log::set_max_level(filter);
    log::info!("Log level changed to: {}", level);
    Ok(())
}

/// Shrink `default` to fit `available`, never growing past it. No lower floor is
/// applied on purpose: the window's `minWidth`/`minHeight` in tauri.conf.json
/// already enforces one, and forcing a minimum here would push the window off a
/// work area smaller than that minimum.
#[tauri::command]
fn reset_main_window_layout(app: AppHandle) -> Result<(), String> {
    window::apply_default_main_window_layout(&app)
}

/// Lowercase hex SHA-256. Shared by the batch archive (records the flashed
/// firmware's digest) and `updater::download_auth_firmware` (verifies it).
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// The archive folder is created inside a user-chosen directory; reject names
/// that could escape it (separators, traversal, leading dots).
fn validate_archive_folder_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err("invalid archive folder name".to_string())
    }
}

/// Write the AppHandle-free part of a batch archive into `dest`: a copy of
/// the auth Excel sheet, the optional firmware binary, batch-summary.json
/// (with `environment` and firmware sha256/size merged in) and
/// batch-slots.csv (UTF-8 BOM so Excel renders it correctly). logs.zip is
/// added by the caller.
fn write_batch_archive(
    dest: &std::path::Path,
    excel_src: Option<&std::path::Path>,
    firmware_src: Option<&std::path::Path>,
    summary_json: &str,
    slots_csv: &str,
    environment: serde_json::Value,
) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("create archive dir: {e}"))?;

    // Flash-only batches never touch a sheet, so there is nothing to copy.
    if let Some(excel_src) = excel_src {
        let excel_name = excel_src
            .file_name()
            .ok_or_else(|| "invalid excel path".to_string())?;
        std::fs::copy(excel_src, dest.join(excel_name))
            .map_err(|e| format!("copy auth sheet: {e}"))?;
    }

    let mut summary: serde_json::Value =
        serde_json::from_str(summary_json).map_err(|e| format!("invalid summary json: {e}"))?;

    if let Some(fw) = firmware_src {
        let fw_name = fw
            .file_name()
            .ok_or_else(|| "invalid firmware path".to_string())?;
        let bytes = std::fs::read(fw).map_err(|e| format!("read firmware: {e}"))?;
        std::fs::write(dest.join(fw_name), &bytes).map_err(|e| format!("copy firmware: {e}"))?;
        if let Some(obj) = summary.get_mut("firmware").and_then(|v| v.as_object_mut()) {
            obj.insert("sha256".into(), serde_json::json!(sha256_hex(&bytes)));
            obj.insert("sizeBytes".into(), serde_json::json!(bytes.len()));
            obj.insert(
                "archivedFileName".into(),
                serde_json::json!(fw_name.to_string_lossy()),
            );
        }
    }

    if let Some(obj) = summary.as_object_mut() {
        obj.insert("environment".into(), environment);
    }
    let pretty = serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?;
    std::fs::write(dest.join("batch-summary.json"), pretty)
        .map_err(|e| format!("write summary: {e}"))?;

    let mut csv = Vec::with_capacity(slots_csv.len() + 3);
    csv.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    csv.extend_from_slice(slots_csv.as_bytes());
    std::fs::write(dest.join("batch-slots.csv"), csv).map_err(|e| format!("write csv: {e}"))?;
    Ok(())
}

/// Archive a finished batch run into `<dest_dir>/<folder_name>`: auth-sheet
/// copy, optional firmware copy, batch-summary.json, batch-slots.csv and a
/// logs.zip of the current session logs. Returns the created folder path.
#[tauri::command]
fn archive_batch_cmd(
    app: AppHandle,
    dest_dir: String,
    folder_name: String,
    excel_path: String,
    firmware_path: Option<String>,
    summary_json: String,
    slots_csv: String,
) -> Result<String, String> {
    validate_archive_folder_name(&folder_name)?;
    let dest = std::path::Path::new(&dest_dir).join(&folder_name);
    let environment = serde_json::json!({
        "app": app.package_info().name,
        "version": app.package_info().version.to_string(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "install": detect_install_type(),
        "sessionId": SESSION_ID.get().map(String::as_str).unwrap_or(""),
    });
    write_batch_archive(
        &dest,
        Some(excel_path.as_str())
            .filter(|s| !s.is_empty())
            .map(std::path::Path::new),
        firmware_path
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(std::path::Path::new),
        &summary_json,
        &slots_csv,
        environment,
    )?;
    let log_dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    logs::gather_and_write_logs_zip(
        &log_dir,
        &app.package_info().name,
        &app.package_info().version.to_string(),
        &detect_install_type(),
        SESSION_ID.get().map(String::as_str).unwrap_or(""),
        &dest.join("logs.zip"),
        // Archive = the operator's local troubleshooting bundle: keep plaintext.
        false,
    )?;
    log::info!("[BatchAuth] archived batch to {}", dest.display());
    Ok(dest.to_string_lossy().into_owned())
}

/// Open an external URL in the system browser.
///
/// On Linux this must NOT go through the opener plugin's detached spawn: when
/// the app runs from an AppImage, `AppRun` prepends `$APPDIR/usr/bin` to `PATH`
/// (so `xdg-open` resolves to the *bundled* copy) and sets `LD_LIBRARY_PATH` /
/// `GTK_PATH` / `XDG_DATA_DIRS` / … into the mount. A browser spawned with that
/// environment loads the AppImage's bundled libraries and silently fails to
/// start — and a detached spawn reports success regardless. Here we invoke the
/// *system* `xdg-open`, strip the AppImage-injected vars, and capture the exit
/// status so failures surface instead of vanishing.
#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        open_external_url_linux(&url)
    }
    #[cfg(not(target_os = "linux"))]
    {
        tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
    }
}

/// AppImage-injected environment variables that point a spawned process at the
/// bundled libraries/modules. Stripped before launching the system browser.
#[cfg(target_os = "linux")]
const APPIMAGE_INJECTED_VARS: &[&str] = &[
    "LD_LIBRARY_PATH",
    "GTK_PATH",
    "GTK_EXE_PREFIX",
    "GTK_DATA_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GDK_PIXBUF_MODULE_FILE",
    "GIO_EXTRA_MODULES",
    "GSETTINGS_SCHEMA_DIR",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "QT_PLUGIN_PATH",
    "PERLLIB",
    "PYTHONPATH",
    "PYTHONHOME",
];

/// The command + environment changes needed to launch `xdg-open` cleanly.
/// Returned (rather than spawned) so the sanitization decisions are testable.
#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq)]
struct XdgCommandSpec {
    program: String,
    arg: String,
    env_remove: Vec<&'static str>,
    /// (key, value) pairs to set; empty when not inside an AppImage.
    env_set: Vec<(&'static str, String)>,
}

/// Build the launch spec for opening `url` via `xdg`. When `appdir` is `Some`
/// (running inside an AppImage), strip the injected vars, reset PATH, and drop
/// `$APPDIR` entries from `xdg_data_dirs`. When `None`, no sanitization.
#[cfg(target_os = "linux")]
fn build_xdg_command_spec(
    url: &str,
    xdg: &str,
    appdir: Option<&str>,
    xdg_data_dirs: Option<&str>,
) -> XdgCommandSpec {
    let mut env_remove = Vec::new();
    let mut env_set = Vec::new();

    if let Some(appdir) = appdir {
        env_remove.extend_from_slice(APPIMAGE_INJECTED_VARS);
        // Reset PATH so the browser and its helpers resolve to system binaries.
        env_set.push((
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string(),
        ));
        // Drop the $APPDIR entries from XDG_DATA_DIRS (handler/.desktop lookup).
        if let Some(dirs) = xdg_data_dirs {
            let cleaned: Vec<&str> = dirs
                .split(':')
                .filter(|d| !d.is_empty() && !d.starts_with(appdir))
                .collect();
            env_set.push((
                "XDG_DATA_DIRS",
                if cleaned.is_empty() {
                    "/usr/local/share:/usr/share".to_string()
                } else {
                    cleaned.join(":")
                },
            ));
        }
    }

    XdgCommandSpec {
        program: xdg.to_string(),
        arg: url.to_string(),
        env_remove,
        env_set,
    }
}

#[cfg(target_os = "linux")]
fn open_external_url_linux(url: &str) -> Result<(), String> {
    use std::process::Command;

    // Prefer the system xdg-open, bypassing any AppImage-bundled copy on PATH.
    let xdg = ["/usr/bin/xdg-open", "/bin/xdg-open"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .unwrap_or("xdg-open");

    let appdir = std::env::var("APPDIR").ok();
    log::info!(
        "[OpenUrl] linux: appimage={}, opener={}, url_len={}",
        appdir.is_some(),
        xdg,
        url.len()
    );

    let xdg_data_dirs = std::env::var("XDG_DATA_DIRS").ok();
    let spec = build_xdg_command_spec(url, xdg, appdir.as_deref(), xdg_data_dirs.as_deref());

    let mut cmd = Command::new(&spec.program);
    cmd.arg(&spec.arg);
    for var in &spec.env_remove {
        cmd.env_remove(var);
    }
    for (key, value) in &spec.env_set {
        cmd.env(key, value);
    }

    // status() waits only for xdg-open (which returns once the browser is
    // launched), not for the browser itself — so this does not block the UI.
    match cmd.status() {
        Ok(s) if s.success() => {
            log::info!("[OpenUrl] {xdg} exited 0 (handed off to browser)");
            Ok(())
        }
        Ok(s) => {
            log::error!("[OpenUrl] {xdg} exited with {s}");
            Err(format!("xdg-open exited with status {s}"))
        }
        Err(e) => {
            log::error!("[OpenUrl] failed to spawn {xdg}: {e}");
            Err(format!("failed to spawn {xdg}: {e}"))
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let session_log_name = format!("tyutool-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::LogDir {
                        file_name: Some(session_log_name),
                    }),
                    Target::new(TargetKind::Stdout),
                ])
                // Cap each session log at 10 MB; once exceeded the plugin rolls
                // the file over (KeepAll keeps the rolled `tyutool-<ts>_<date>.log`,
                // which prune_log_files / list_log_files still pick up). This bounds
                // within-session growth; prune trims old sessions at startup.
                .max_file_size(10 * 1024 * 1024)
                .rotation_strategy(RotationStrategy::KeepAll)
                .timezone_strategy(TimezoneStrategy::UseLocal)
                .level(log::LevelFilter::Debug)
                // espflash dumps every protocol command's full payload as hex at DEBUG
                // (~10 MB per ESP flash), drowning the session log. Cap it at INFO;
                // use the CLI with --verbose to capture ESP protocol frames instead.
                .level_for("espflash", log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(FlashState {
            cancel: StdMutex::new(Arc::new(AtomicBool::new(false))),
            thread: StdMutex::new(None),
        })
        .manage({
            let (archive, filters) = serial_debug::create_serial_debug_archive_resilient();
            serial_debug::DebugState {
                session: Arc::new(StdMutex::new(None)),
                archive: Arc::new(StdMutex::new(archive)),
                filters: Arc::new(StdMutex::new(filters)),
                chunk_bridge: Arc::new(StdMutex::new(None)),
                generation: Arc::new(SerialDebugGeneration::default()),
            }
        })
        .manage(BatchFlashState {
            slots: StdMutex::new(HashMap::new()),
        })
        .manage(BatchAuthState {
            slots: StdMutex::new(HashMap::new()),
            session: Arc::new(StdMutex::new(AllocatorSession {
                alloc: None,
                active: 0,
            })),
        })
        .manage(ConfirmState {
            sender: Arc::new(StdMutex::new(None)),
        })
        .manage(updater::UpdateState {
            pending: StdMutex::new(None),
        })
        .manage(logs::DialogPathRegistry::new())
        .setup(|app| {
            let version = app.package_info().version.to_string();
            let install_type = detect_install_type();
            let session_id = tyutool_core::diagnostics::log_session_banner(
                &app.package_info().name,
                "GUI",
                &version,
                Some(&install_type),
            );
            let _ = SESSION_ID.set(session_id);
            if let Ok(log_dir) = app.path().app_log_dir() {
                logs::prune_log_files(&log_dir);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_serial_ports_cmd,
            flash_run,
            authorize_confirm_cmd,
            flash_cancel,
            batch_flash_start,
            batch_flash_cancel_port,
            batch_flash_cancel_all,
            validate_excel_cmd,
            batch_auth_start,
            batch_auth_cancel_port,
            batch_auth_cancel_all,
            batch_auth_read_ports,
            serial_debug::device_reset_cmd,
            serial_debug::serial_debug_device_reset_cmd,
            get_file_size,
            check_port_available_cmd,
            check_file_exists,
            updater::fetch_url,
            updater::update_check,
            updater::update_download,
            updater::update_install,
            updater::download_auth_firmware,
            get_install_type,
            set_log_level,
            reset_main_window_layout,
            serial_debug::serial_debug_open,
            serial_debug::serial_debug_close,
            serial_debug::serial_debug_send,
            serial_debug::serial_debug_state,
            serial_debug::serial_debug_session_clear,
            serial_debug::serial_debug_append_sys_line,
            serial_debug::serial_debug_filter_add,
            serial_debug::serial_debug_filter_remove,
            serial_debug::serial_debug_filter_read_matches,
            serial_debug::serial_debug_session_read_page,
            logs::write_text_file,
            logs::append_text_file,
            logs::register_dialog_path,
            logs::list_log_files,
            logs::list_log_file_openers,
            logs::read_log_tail,
            logs::open_log_file_in_editor,
            logs::export_logs_zip,
            archive_batch_cmd,
            open_external_url,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                RunEvent::Ready => {
                    // After the event loop is ready: layout, then show (window starts `visible: false`
                    // so the compositor / session restore does not paint a wrong geometry first).
                    let _ = window::apply_default_main_window_layout(app_handle);
                    if let Some(win) = app_handle.get_webview_window("main") {
                        let _ = win.show();
                    }
                    // Some desktops re-apply saved geometry shortly after map; re-layout once shortly after.
                    let h = app_handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(280));
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || {
                            let _ = window::apply_default_main_window_layout(&h2);
                        });
                    });
                }
                RunEvent::ExitRequested { .. } => {
                    // Signal cancel for all batch flash threads and collect handles
                    let flash_threads: Vec<JoinHandle<()>> = {
                        let mut v = Vec::new();
                        if let Some(batch_state) = app_handle.try_state::<BatchFlashState>() {
                            if let Ok(mut slots) = batch_state.slots.lock() {
                                for (_, slot) in slots.drain() {
                                    slot.cancel.store(true, Ordering::SeqCst);
                                    v.push(slot.thread);
                                }
                            }
                        }
                        v
                    };
                    let auth_threads: Vec<JoinHandle<()>> = {
                        let mut v = Vec::new();
                        if let Some(auth_state) = app_handle.try_state::<BatchAuthState>() {
                            if let Ok(mut slots) = auth_state.slots.lock() {
                                for (_, slot) in slots.drain() {
                                    slot.cancel.store(true, Ordering::SeqCst);
                                    v.push(slot.thread);
                                }
                            }
                        }
                        v
                    };
                    // Join with 5s total timeout — gives threads time to release serial ports
                    // and finish Excel writes before the process exits
                    let deadline = std::time::Instant::now() + Duration::from_secs(5);
                    for t in flash_threads.into_iter().chain(auth_threads) {
                        let remaining =
                            deadline.saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        let (tx, rx) = std::sync::mpsc::channel::<()>();
                        std::thread::spawn(move || {
                            let _ = t.join();
                            let _ = tx.send(());
                        });
                        let _ = rx.recv_timeout(remaining);
                    }
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_archive_folder_name_accepts_generated_names() {
        assert!(validate_archive_folder_name("batch-archive_20260717-143205_esp32").is_ok());
    }

    #[test]
    fn validate_archive_folder_name_rejects_escapes() {
        for bad in ["", "..", "a/b", "a\\b", ".hidden", "a b"] {
            assert!(validate_archive_folder_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn write_batch_archive_produces_all_files_and_merges_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let excel = dir.path().join("codes.xlsx");
        std::fs::write(&excel, b"excel-bytes").unwrap();
        let fw = dir.path().join("auth.bin");
        std::fs::write(&fw, b"abc").unwrap();
        let dest = dir.path().join("archive");

        write_batch_archive(
            &dest,
            Some(&excel),
            Some(&fw),
            r#"{"firmware":{"source":"local"},"batch":{}}"#,
            "port,status\r\nCOM3,done\r\n",
            serde_json::json!({"os": "test"}),
        )
        .unwrap();

        assert_eq!(
            std::fs::read(dest.join("codes.xlsx")).unwrap(),
            b"excel-bytes"
        );
        assert_eq!(std::fs::read(dest.join("auth.bin")).unwrap(), b"abc");
        let csv = std::fs::read(dest.join("batch-slots.csv")).unwrap();
        assert_eq!(&csv[..3], &[0xEF, 0xBB, 0xBF]);
        let written: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dest.join("batch-summary.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(written["environment"]["os"], "test");
        assert_eq!(written["firmware"]["sizeBytes"], 3);
        assert_eq!(
            written["firmware"]["sha256"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(written["firmware"]["archivedFileName"], "auth.bin");
    }

    #[test]
    fn write_batch_archive_without_firmware_skips_firmware_files() {
        let dir = tempfile::tempdir().unwrap();
        let excel = dir.path().join("codes.xlsx");
        std::fs::write(&excel, b"excel-bytes").unwrap();
        let dest = dir.path().join("archive");

        write_batch_archive(
            &dest,
            Some(&excel),
            None,
            r#"{"firmware":null,"batch":{}}"#,
            "port,status\r\n",
            serde_json::json!({}),
        )
        .unwrap();

        assert!(dest.join("codes.xlsx").exists());
        assert!(dest.join("batch-summary.json").exists());
        assert!(!dest.join("auth.bin").exists());
    }

    #[test]
    fn write_batch_archive_without_excel_skips_sheet_copy() {
        let dir = tempfile::tempdir().unwrap();
        let fw = dir.path().join("auth.bin");
        std::fs::write(&fw, b"abc").unwrap();
        let dest = dir.path().join("archive");

        write_batch_archive(
            &dest,
            None,
            Some(&fw),
            r#"{"firmware":{"source":"local"},"batch":{}}"#,
            "port,status\r\n",
            serde_json::json!({}),
        )
        .unwrap();

        assert!(dest.join("auth.bin").exists());
        assert!(dest.join("batch-summary.json").exists());
        assert!(!dest.join("codes.xlsx").exists());
    }

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

#[cfg(test)]
#[cfg(target_os = "linux")]
mod xdg_command_tests {
    use super::*;

    #[test]
    fn no_appimage_leaves_env_untouched() {
        let spec = build_xdg_command_spec("https://example.com", "/usr/bin/xdg-open", None, None);
        assert_eq!(spec.program, "/usr/bin/xdg-open");
        assert_eq!(spec.arg, "https://example.com");
        assert!(spec.env_remove.is_empty());
        assert!(spec.env_set.is_empty());
    }

    #[test]
    fn appimage_strips_vars_and_resets_path() {
        let spec = build_xdg_command_spec(
            "https://example.com",
            "/usr/bin/xdg-open",
            Some("/tmp/.mount_app"),
            None,
        );
        assert!(spec.env_remove.contains(&"LD_LIBRARY_PATH"));
        assert!(spec.env_remove.contains(&"GTK_PATH"));
        let path = spec
            .env_set
            .iter()
            .find(|(k, _)| *k == "PATH")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            path,
            Some("/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
        );
        // No XDG_DATA_DIRS provided -> not set.
        assert!(spec.env_set.iter().all(|(k, _)| *k != "XDG_DATA_DIRS"));
    }

    #[test]
    fn appimage_drops_appdir_entries_from_xdg_data_dirs() {
        let spec = build_xdg_command_spec(
            "https://example.com",
            "/usr/bin/xdg-open",
            Some("/tmp/.mount_app"),
            Some("/tmp/.mount_app/usr/share:/usr/share:/usr/local/share"),
        );
        let dirs = spec
            .env_set
            .iter()
            .find(|(k, _)| *k == "XDG_DATA_DIRS")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert_eq!(dirs, "/usr/share:/usr/local/share");
    }

    #[test]
    fn appimage_falls_back_when_all_xdg_data_dirs_stripped() {
        let spec = build_xdg_command_spec(
            "https://example.com",
            "/usr/bin/xdg-open",
            Some("/tmp/.mount_app"),
            Some("/tmp/.mount_app/usr/share"),
        );
        let dirs = spec
            .env_set
            .iter()
            .find(|(k, _)| *k == "XDG_DATA_DIRS")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert_eq!(dirs, "/usr/local/share:/usr/share");
    }
}
