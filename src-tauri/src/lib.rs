use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, RunEvent, State};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};

struct FlashState {
    cancel: Arc<AtomicBool>,
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
    state.cancel.store(false, Ordering::SeqCst);
    let cancel = state.cancel.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = tyutool_core::run_job(&job, &cancel, |p| {
            let _ = app.emit("flash-progress", &p);
        });
    });
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
    state.cancel.store(true, Ordering::SeqCst);
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
            cancel: Arc::new(AtomicBool::new(false)),
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
            device_reset_cmd,
            get_file_size,
            check_port_available_cmd,
            check_file_exists,
            fetch_url,
            get_install_type,
            set_log_level,
            reset_main_window_layout,
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
