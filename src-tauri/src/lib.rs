mod batch_auth;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, RunEvent, State};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};
use tyutool_core::{DebugChunk, DebugConfig, SerialDebugSession};

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

struct DebugState {
    session: StdMutex<Option<SerialDebugSession>>,
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
    allocator: StdMutex<Option<std::sync::Arc<batch_auth::ExcelRowAllocator>>>,
}

#[derive(Clone, serde::Serialize)]
struct DisconnectPayload {
    reason: String,
}

const DEFAULT_MAIN_WINDOW_WIDTH: f64 = 1280.0;
const DEFAULT_MAIN_WINDOW_HEIGHT: f64 = 800.0;
const MIN_MAIN_WINDOW_WIDTH: f64 = 1024.0;
const MIN_MAIN_WINDOW_HEIGHT: f64 = 680.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalWindowSize {
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalWindowPosition {
    x: i32,
    y: i32,
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
        return format!("portable ({})", exe_str);
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
        return format!("portable ({})", exe_str);
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

        return format!("portable ({})", exe_str);
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
    job: tyutool_core::FlashJob,
) -> Result<(), String> {
    log::info!(
        "[Flash] Starting operation: mode={:?}, chip={}, port={}, baud={}",
        job.mode,
        job.chip_id,
        job.port,
        job.baud_rate
    );

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
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchAuthStartConfig {
    chip_id: String,
    baud_rate: u32,
    firmware_path: Option<String>,
    excel_path: String,
    conflict_policy: String,
}

#[tauri::command]
fn batch_flash_start(
    app: AppHandle,
    state: State<'_, BatchFlashState>,
    config: BatchFlashStartConfig,
    ports: Vec<String>,
) -> Result<(), String> {
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
            if !std::path::Path::new(&config_clone.firmware_path).exists() {
                let _ = app_clone.emit(
                    "batch-flash-progress",
                    serde_json::json!({
                        "port": port_clone,
                        "event": { "kind": "done", "result": { "err": { "message": "firmware file not found", "elapsed_secs": 0 } } }
                    }),
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
                flash_start_hex: None,
                flash_end_hex: None,
                erase_start_hex: None,
                erase_end_hex: None,
                read_start_hex: None,
                read_end_hex: None,
                read_file_path: None,
                authorize_uuid: None,
                authorize_key: None,
            };

            let _ = tyutool_core::run_job(&job, &cancel_clone, |p| {
                let _ = app_clone.emit(
                    "batch-flash-progress",
                    serde_json::json!({ "port": port_clone, "event": p }),
                );
            });
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
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    if let Some(slot) = slots.get(&port) {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn batch_flash_cancel_all(state: State<'_, BatchFlashState>) -> Result<(), String> {
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    for slot in slots.values() {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn validate_excel_cmd(path: String) -> Result<batch_auth::ExcelStats, String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err("文件不存在".into());
    }
    if p.extension().and_then(|e| e.to_str()) != Some("xlsx") {
        return Err("请选择 .xlsx 格式文件".into());
    }
    let alloc = batch_auth::ExcelRowAllocator::load(p)?;
    Ok(alloc.stats())
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

    let allocator = {
        let path = std::path::Path::new(&config.excel_path);
        let mut alloc_guard = state.allocator.lock().map_err(|e| e.to_string())?;
        // Reuse existing allocator if present; otherwise load from disk
        if let Some(ref existing) = *alloc_guard {
            existing.clone()
        } else {
            let alloc = std::sync::Arc::new(batch_auth::ExcelRowAllocator::load(path)?);
            *alloc_guard = Some(alloc.clone());
            alloc
        }
    };

    for port in ports {
        // 1. Remove old slot and wait for it (under lock, but quickly)
        let old_slot = {
            let mut slots = state.slots.lock().map_err(|e| e.to_string())?;
            slots.remove(&port)
        };

        // 2. Wait for old thread OUTSIDE the lock
        if let Some(old) = old_slot {
            old.cancel.store(true, Ordering::SeqCst);
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                let _ = old.thread.join();
                let _ = tx.send(());
            });
            if rx.recv_timeout(Duration::from_secs(3)).is_err() {
                return Err(format!("port {} not stopped; retry in a few seconds", port));
            }
        }

        // 3. Allocate row (outside lock)
        let row = match allocator.allocate_row() {
            Ok(r) => r,
            Err(e) => {
                let _ = app.emit(
                    "batch-auth-progress",
                    serde_json::json!({
                        "port": port,
                        "step": "failed",
                        "error": e
                    }),
                );
                continue;
            }
        };

        // 4. Set up cancel + spawn thread
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_clone = cancel.clone();
        let app_clone = app.clone();
        let port_clone = port.clone();
        let config_clone = config.clone();
        let alloc_clone = allocator.clone();
        let row_idx = row.row_idx;
        let uuid = row.uuid.clone();
        let authkey = row.authkey.clone();

        let handle = std::thread::spawn(move || {
            if let Some(ref fw_path) = config_clone.firmware_path {
                if !fw_path.is_empty() {
                    let job = tyutool_core::FlashJob {
                        mode: tyutool_core::FlashMode::Flash,
                        chip_id: config_clone.chip_id.clone(),
                        port: port_clone.clone(),
                        baud_rate: config_clone.baud_rate,
                        firmware_path: Some(fw_path.clone()),
                        segments: None,
                        flash_start_hex: None,
                        flash_end_hex: None,
                        erase_start_hex: None,
                        erase_end_hex: None,
                        read_start_hex: None,
                        read_end_hex: None,
                        read_file_path: None,
                        authorize_uuid: None,
                        authorize_key: None,
                    };
                    let app2 = app_clone.clone();
                    let port2 = port_clone.clone();
                    let flash_result = tyutool_core::run_job(&job, &cancel_clone, |p| {
                        let _ = app2.emit(
                            "batch-auth-progress",
                            serde_json::json!({
                                "port": port2,
                                "step": "flashing",
                                "event": p
                            }),
                        );
                    });
                    if flash_result.is_err() || cancel_clone.load(Ordering::Relaxed) {
                        alloc_clone.release_row(row_idx);
                        let _ = app_clone.emit(
                            "batch-auth-progress",
                            serde_json::json!({
                                "port": port_clone,
                                "step": "failed",
                                "error": flash_result.err().map(|e| e.to_string())
                                    .unwrap_or_else(|| "cancelled".into())
                            }),
                        );
                        return;
                    }
                }
            }

            let result = tyutool_core::run_batch_auth_slot(
                &port_clone,
                &uuid,
                &authkey,
                conflict_policy,
                &cancel_clone,
                |step| {
                    let step_str = match step {
                        tyutool_core::BatchAuthStep::ReadingMac => "reading_mac",
                        tyutool_core::BatchAuthStep::ReadingAuth => "reading_auth",
                        tyutool_core::BatchAuthStep::WritingAuth => "writing_auth",
                        tyutool_core::BatchAuthStep::Verifying => "verifying",
                    };
                    let _ = app_clone.emit(
                        "batch-auth-progress",
                        serde_json::json!({ "port": port_clone, "step": step_str }),
                    );
                },
            );

            match result {
                Ok(tyutool_core::BatchAuthSlotResult::Done { mac }) => {
                    let _ = alloc_clone.confirm_row(row_idx, mac.clone());
                    let _ = app_clone.emit(
                        "batch-auth-progress",
                        serde_json::json!({ "port": port_clone, "step": "done", "mac": mac }),
                    );
                }
                Ok(tyutool_core::BatchAuthSlotResult::AlreadyDone { mac }) => {
                    let _ = alloc_clone.confirm_row(row_idx, mac.clone());
                    let _ = app_clone.emit(
                        "batch-auth-progress",
                        serde_json::json!({ "port": port_clone, "step": "done", "mac": mac }),
                    );
                }
                Ok(tyutool_core::BatchAuthSlotResult::Skipped { mac }) => {
                    alloc_clone.release_row(row_idx);
                    let _ = app_clone.emit(
                        "batch-auth-progress",
                        serde_json::json!({ "port": port_clone, "step": "skipped", "mac": mac }),
                    );
                }
                Ok(tyutool_core::BatchAuthSlotResult::Cancelled) => {
                    alloc_clone.release_row(row_idx);
                }
                Err(e) => {
                    alloc_clone.release_row(row_idx);
                    let _ = app_clone.emit(
                        "batch-auth-progress",
                        serde_json::json!({
                            "port": port_clone,
                            "step": "failed",
                            "error": e.to_string()
                        }),
                    );
                }
            }
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
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    if let Some(slot) = slots.get(&port) {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn batch_auth_cancel_all(state: State<'_, BatchAuthState>) -> Result<(), String> {
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    for slot in slots.values() {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

/// Read current UART authorization (for GUI overwrite prompt). Does not emit `flash-progress`.
#[tauri::command]
fn authorize_probe_cmd(port: String) -> Result<Option<tyutool_core::DeviceAuthorization>, String> {
    let cancel = AtomicBool::new(false);
    tyutool_core::probe_device_authorization(&port, &cancel).map_err(|e| e.to_string())
}

#[tauri::command]
fn flash_cancel(state: State<'_, FlashState>) {
    log::info!("[Flash] User cancelled operation");
    if let Ok(guard) = state.cancel.lock() {
        guard.store(true, Ordering::SeqCst);
    }
}

#[tauri::command]
async fn serial_debug_open(
    app: AppHandle,
    state: State<'_, DebugState>,
    cfg: DebugConfig,
) -> Result<(), String> {
    {
        let guard = state
            .session
            .lock()
            .map_err(|_| "debug state poisoned".to_string())?;
        if guard.is_some() {
            return Err("already open".into());
        }
    }
    let app_for_chunk = app.clone();
    let app_for_disc = app.clone();
    // Run the blocking serialport::open() off the main thread.
    let session = tauri::async_runtime::spawn_blocking(move || {
        SerialDebugSession::open(
            cfg,
            Box::new(move |chunk: DebugChunk| {
                let _ = app_for_chunk.emit("serial-debug-chunk", &chunk);
            }),
            Box::new(move |reason: String| {
                let _ =
                    app_for_disc.emit("serial-debug-disconnected", &DisconnectPayload { reason });
            }),
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let mut guard = state
        .session
        .lock()
        .map_err(|_| "debug state poisoned".to_string())?;
    if guard.is_some() {
        // Another open won the race while we were in spawn_blocking; discard this session.
        session.close();
        return Err("already open".into());
    }
    *guard = Some(session);
    Ok(())
}

#[tauri::command]
async fn serial_debug_close(state: State<'_, DebugState>) -> Result<(), String> {
    let session = {
        let mut guard = state
            .session
            .lock()
            .map_err(|_| "debug state poisoned".to_string())?;
        guard.take()
    };
    if let Some(session) = session {
        // h.join() blocks; run it off the async runtime thread.
        tauri::async_runtime::spawn_blocking(move || session.close())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn serial_debug_send(
    app: AppHandle,
    state: State<'_, DebugState>,
    bytes: Vec<u8>,
) -> Result<(), String> {
    {
        let guard = state
            .session
            .lock()
            .map_err(|_| "debug state poisoned".to_string())?;
        let session = guard
            .as_ref()
            .ok_or_else(|| "serial debug not open".to_string())?;
        session.write(&bytes).map_err(|e| e.to_string())?;
    } // DebugState lock dropped here — emit happens unlocked
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let chunk = tyutool_core::DebugChunk {
        direction: tyutool_core::Direction::Tx,
        ts_ms,
        bytes,
    };
    let _ = app.emit("serial-debug-chunk", &chunk);
    Ok(())
}

#[tauri::command]
fn serial_debug_state(state: State<'_, DebugState>) -> Result<Option<DebugConfig>, String> {
    let guard = state
        .session
        .lock()
        .map_err(|_| "debug state poisoned".to_string())?;
    Ok(guard.as_ref().map(|s| s.config().clone()))
}

#[tauri::command]
fn get_file_size(path: String) -> Result<u64, String> {
    let size = std::fs::metadata(&path)
        .map(|m| m.len())
        .map_err(|e| format!("cannot stat '{}': {}", path, e))?;
    log::debug!("[File] get_file_size: path={}, size={}", path, size);
    Ok(size)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceResetArgs {
    port: String,
    chip_id: String,
}

#[tauri::command]
fn device_reset_cmd(args: DeviceResetArgs) -> Result<(), String> {
    log::info!(
        "[Serial] Device reset (DTR/RTS): port={}, chip_id={}",
        args.port,
        args.chip_id
    );
    tyutool_core::device_reset_dtr_rts(&args.port, &args.chip_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn check_port_available_cmd(port: String) -> tyutool_core::PortCheckResult {
    let result = tyutool_core::check_port_available(&port);
    log::debug!(
        "[Serial] check_port_available: port={}, available={}",
        port,
        result.available
    );
    result
}

#[tauri::command]
fn check_file_exists(path: String) -> bool {
    let exists = std::path::Path::new(&path).exists();
    log::debug!("[File] check_file_exists: path={}, exists={}", path, exists);
    exists
}

/// Fetch a URL and return body as string. Used by the frontend update checker
/// to bypass WebView CSP restrictions on cross-origin fetch.
#[tauri::command]
async fn fetch_url(url: String, timeout_ms: u64) -> Result<String, String> {
    log::info!("[Update] fetch_url: url={}, timeout_ms={}", url, timeout_ms);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| {
            log::error!("[Update] fetch_url: failed to build client: {}", e);
            e.to_string()
        })?;
    let resp = client.get(&url).send().await.map_err(|e| {
        log::error!("[Update] fetch_url: request failed for {}: {}", url, e);
        e.to_string()
    })?;
    let status = resp.status();
    log::info!("[Update] fetch_url: response status={}", status);
    if !status.is_success() {
        log::error!("[Update] fetch_url: HTTP error {}", status);
        return Err(format!("HTTP {}", status));
    }
    let body = resp.text().await.map_err(|e| {
        log::error!("[Update] fetch_url: failed to read body: {}", e);
        e.to_string()
    })?;
    log::info!("[Update] fetch_url: body length={}", body.len());
    Ok(body)
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

fn fit_logical_dimension(default: f64, min: f64, available: f64) -> f64 {
    if !available.is_finite() || available <= 0.0 {
        return default;
    }
    if available >= default {
        default
    } else if available >= min {
        available
    } else {
        available
    }
}

fn default_main_window_logical_size(
    work_area: PhysicalRect,
    scale_factor: f64,
) -> LogicalSize<f64> {
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let available_width = f64::from(work_area.width) / scale_factor;
    let available_height = f64::from(work_area.height) / scale_factor;

    LogicalSize::new(
        fit_logical_dimension(
            DEFAULT_MAIN_WINDOW_WIDTH,
            MIN_MAIN_WINDOW_WIDTH,
            available_width,
        ),
        fit_logical_dimension(
            DEFAULT_MAIN_WINDOW_HEIGHT,
            MIN_MAIN_WINDOW_HEIGHT,
            available_height,
        ),
    )
}

fn clamp_axis(position: i32, size: u32, work_start: i32, work_extent: u32) -> i32 {
    if size >= work_extent {
        return work_start;
    }

    let min = i64::from(work_start);
    let max = min + i64::from(work_extent) - i64::from(size);
    i64::from(position).clamp(min, max) as i32
}

fn clamp_outer_position_to_work_area(
    x: i32,
    y: i32,
    outer_size: PhysicalWindowSize,
    work_area: PhysicalRect,
) -> PhysicalWindowPosition {
    PhysicalWindowPosition {
        x: clamp_axis(x, outer_size.width, work_area.x, work_area.width),
        y: clamp_axis(y, outer_size.height, work_area.y, work_area.height),
    }
}

fn physical_rect_from_tauri(rect: &tauri::PhysicalRect<i32, u32>) -> PhysicalRect {
    PhysicalRect {
        x: rect.position.x,
        y: rect.position.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

/// Default main window size + safe visible placement (matches `tauri.conf.json` when it fits).
fn apply_default_main_window_layout(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        let monitor = win
            .current_monitor()
            .map_err(|e| e.to_string())?
            .or_else(|| win.primary_monitor().ok().flatten())
            .or_else(|| {
                win.available_monitors()
                    .ok()
                    .and_then(|monitors| monitors.into_iter().next())
            });

        if let Some(monitor) = monitor {
            let work_area = physical_rect_from_tauri(monitor.work_area());
            let size = default_main_window_logical_size(work_area, monitor.scale_factor());
            win.set_size(size).map_err(|e| e.to_string())?;
            win.center().map_err(|e| e.to_string())?;

            let outer_position = win.outer_position().map_err(|e| e.to_string())?;
            let outer_size = win.outer_size().map_err(|e| e.to_string())?;
            let clamped = clamp_outer_position_to_work_area(
                outer_position.x,
                outer_position.y,
                PhysicalWindowSize {
                    width: outer_size.width,
                    height: outer_size.height,
                },
                work_area,
            );

            if clamped.x != outer_position.x || clamped.y != outer_position.y {
                win.set_position(PhysicalPosition::new(clamped.x, clamped.y))
                    .map_err(|e| e.to_string())?;
            }
        } else {
            win.set_size(LogicalSize::new(
                DEFAULT_MAIN_WINDOW_WIDTH,
                DEFAULT_MAIN_WINDOW_HEIGHT,
            ))
            .map_err(|e| e.to_string())?;
            win.center().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
fn reset_main_window_layout(app: AppHandle) -> Result<(), String> {
    apply_default_main_window_layout(&app)
}

#[tauri::command]
fn write_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
fn append_text_file(path: String, content: String) -> Result<(), String> {
    use std::io::Write;
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::LogDir { file_name: None }),
                    Target::new(TargetKind::Stdout),
                ])
                .rotation_strategy(RotationStrategy::KeepAll)
                .max_file_size(5 * 1024 * 1024) // 5MB
                .timezone_strategy(TimezoneStrategy::UseLocal)
                .level(log::LevelFilter::Info)
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
        .manage(DebugState {
            session: StdMutex::new(None),
        })
        .manage(BatchFlashState {
            slots: StdMutex::new(HashMap::new()),
        })
        .manage(BatchAuthState {
            slots: StdMutex::new(HashMap::new()),
            allocator: StdMutex::new(None),
        })
        .setup(|app| {
            let version = app.package_info().version.to_string();
            let name = &app.package_info().name;
            let install_type = detect_install_type();
            log::info!("========================================");
            log::info!("[App] {} v{} starting", name, version);
            log::info!("[App] Type: GUI");
            log::info!(
                "[App] OS: {}, Arch: {}, Family: {}",
                std::env::consts::OS,
                std::env::consts::ARCH,
                std::env::consts::FAMILY
            );
            log::info!("[App] Install: {}", install_type);
            if let Ok(exe) = std::env::current_exe() {
                log::info!("[App] Exe: {}", exe.display());
            }
            log::info!("========================================");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_serial_ports_cmd,
            flash_run,
            authorize_probe_cmd,
            flash_cancel,
            batch_flash_start,
            batch_flash_cancel_port,
            batch_flash_cancel_all,
            validate_excel_cmd,
            batch_auth_start,
            batch_auth_cancel_port,
            batch_auth_cancel_all,
            device_reset_cmd,
            get_file_size,
            check_port_available_cmd,
            check_file_exists,
            fetch_url,
            get_install_type,
            set_log_level,
            reset_main_window_layout,
            serial_debug_open,
            serial_debug_close,
            serial_debug_send,
            serial_debug_state,
            write_text_file,
            append_text_file,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                RunEvent::Ready => {
                    // After the event loop is ready: layout, then show (window starts `visible: false`
                    // so the compositor / session restore does not paint a wrong geometry first).
                    let _ = apply_default_main_window_layout(&app_handle);
                    if let Some(win) = app_handle.get_webview_window("main") {
                        let _ = win.show();
                    }
                    // Some desktops re-apply saved geometry shortly after map; re-layout once shortly after.
                    let h = app_handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(280));
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || {
                            let _ = apply_default_main_window_layout(&h2);
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
    fn default_layout_shrinks_to_fit_high_dpi_work_area() {
        let work_area = PhysicalRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };

        let size = default_main_window_logical_size(work_area, 1.5);

        assert_eq!(size.width, 1280.0);
        assert!(size.height < DEFAULT_MAIN_WINDOW_HEIGHT);
        assert!(size.height <= 1040.0 / 1.5);
    }

    #[test]
    fn clamp_outer_position_moves_window_below_work_area_top() {
        let work_area = PhysicalRect {
            x: 0,
            y: 40,
            width: 1920,
            height: 1040,
        };
        let outer_size = PhysicalWindowSize {
            width: 1200,
            height: 800,
        };

        let pos = clamp_outer_position_to_work_area(-100, -200, outer_size, work_area);

        assert_eq!(pos.x, 0);
        assert_eq!(pos.y, 40);
    }

    #[test]
    fn clamp_outer_position_keeps_title_bar_visible_when_window_is_taller_than_work_area() {
        let work_area = PhysicalRect {
            x: 100,
            y: 100,
            width: 800,
            height: 500,
        };
        let outer_size = PhysicalWindowSize {
            width: 900,
            height: 700,
        };

        let pos = clamp_outer_position_to_work_area(20, 20, outer_size, work_area);

        assert_eq!(pos.x, 100);
        assert_eq!(pos.y, 100);
    }
}
