//! tyutool-bridge binary entry — two modes over the same WS server:
//!
//! * default: resident tray shell (menu bar status item) with the server on a
//!   background tokio runtime, so the user can see the bridge is alive and quit
//!   it deliberately;
//! * `--headless`: the pre-B6 behaviour (serve until killed, stderr logging,
//!   `exit(1)` when the port is taken) — what CI and the smoke scripts drive.
//!
//! A hand-rolled flag check instead of clap: the binary has exactly one option
//! and no subcommands, so an argument parser would be the larger surface.

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tyutool_bridge::status::{self, StatsSnapshot};
use tyutool_bridge::{bind, DEFAULT_PORT};

/// Own version, shown in the tray status line.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Login-item / LaunchAgent label for the autostart registration.
const AUTOSTART_APP_NAME: &str = "tyutool-bridge";

/// Tray menu targets.
///
/// TODO(联调期确认): both are placeholders — swap for the real Cobuilder entry
/// point and the bridge download/landing page once product confirms them.
const COBUILDER_URL: &str = "https://iot.tuya.com";
const LATEST_VERSION_URL: &str = "https://iot.tuya.com";

/// Session log retention for this binary, mirroring `prune_log_files` in
/// tyutool-cli (same "delete oldest until inside the limits" rule, smaller
/// budget: the bridge is a resident process, not an interactive tool).
const MAX_LOG_FILES: usize = 20;
const MAX_LOG_BYTES_TOTAL: u64 = 50 * 1024 * 1024; // 50 MB
/// Log file prefix; `prune_log_files` only ever touches files matching it.
const LOG_FILE_PREFIX: &str = "tyutool-bridge-";

fn main() {
    let headless = std::env::args().skip(1).any(|arg| arg == "--headless");
    init_logging(headless);
    // Single shared helper, never a locally inlined banner (repo logging
    // contract): this is what makes bridge bug reports comparable to CLI/GUI.
    tyutool_core::diagnostics::log_session_banner("tyutool-bridge", "BRIDGE", VERSION, None);

    if headless {
        run_headless();
    } else {
        run_tray();
    }
}

// ── Logging ──────────────────────────────────────────────────────────────────

/// stderr (developer diagnostics, kept from B1) plus a per-session log file, so
/// a tray-mode user who never sees stderr can still attach logs to an issue.
///
/// Never fatal: a missing/unwritable data dir degrades to stderr only.
fn init_logging(headless: bool) {
    let (log_path, file_chain) = match open_session_log() {
        Ok((path, file)) => {
            if headless {
                eprintln!("[log] Writing to: {}", path.display());
            }
            (Some(path), Some(file))
        }
        Err(e) => {
            eprintln!("tyutool-bridge: file logging disabled: {e:#}");
            (None, None)
        }
    };

    let mut dispatch = fern::Dispatch::new()
        .level(log::LevelFilter::Info)
        // The 1s discovery poller makes core's per-enumeration INFO line an
        // unbounded 1 Hz stream; keep only its warnings/errors — on both sinks.
        .level_for("tyutool_core::serial", log::LevelFilter::Warn)
        .chain(
            fern::Dispatch::new()
                .format(|out, message, record| {
                    out.finish(format_args!(
                        "[{}][{}] {}",
                        record.level(),
                        record.target(),
                        message
                    ))
                })
                .chain(std::io::stderr()),
        );

    if let Some(file) = file_chain {
        dispatch = dispatch.chain(
            fern::Dispatch::new()
                .format(|out, message, record| {
                    out.finish(format_args!(
                        "[{} {} {}] {}",
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
                        record.level(),
                        record.target(),
                        message
                    ))
                })
                .chain(file),
        );
    }

    if let Err(e) = dispatch.apply() {
        eprintln!("tyutool-bridge: logger init failed: {e}");
        return;
    }
    // Recorded in the log itself so a tray-mode user asked for "the log" can
    // find it without knowing the platform's data directory.
    if let Some(path) = log_path {
        log::info!("bridge session log: {}", path.display());
    }
}

/// Create `{data_dir}/tyutool-bridge/tyutool-bridge-<UTC timestamp>.log` and
/// prune older sessions. The name follows the CLI's `tyutool-<timestamp>.log`
/// scheme so "newest `*.log` by mtime" stays a valid way to find the live file.
///
/// TODO: no in-session size rollover yet (the CLI's `SessionLogWriter` caps a
/// single file at 10 MB); add it when the bridge grows chatty enough to matter.
fn open_session_log() -> anyhow::Result<(std::path::PathBuf, std::fs::File)> {
    let dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("no platform data directory"))?
        .join("tyutool-bridge");
    std::fs::create_dir_all(&dir).map_err(|e| anyhow::anyhow!("create {}: {e}", dir.display()))?;
    prune_log_files(&dir);

    let path = dir.join(format!(
        "{LOG_FILE_PREFIX}{}.log",
        chrono::Utc::now().format("%Y%m%d-%H%M%SZ")
    ));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))?;
    Ok((path, file))
}

/// Delete the oldest session logs until the directory is within both limits.
/// Always keeps at least one file; only touches `LOG_FILE_PREFIX` files.
fn prune_log_files(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::path::PathBuf, u64)> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "log")
                && path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem.starts_with(LOG_FILE_PREFIX))
        })
        .map(|path| {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            (path, size)
        })
        .collect();

    // The timestamped names sort chronologically, so name order is age order.
    files.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));

    let mut count = files.len();
    let mut total: u64 = files.iter().map(|(_, size)| size).sum();
    for (path, size) in &files {
        if count <= 1 || (count <= MAX_LOG_FILES && total <= MAX_LOG_BYTES_TOTAL) {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            count -= 1;
            total = total.saturating_sub(*size);
        } else {
            // Locked by another instance: leave it and keep going.
            count -= 1;
        }
    }
}

// ── Headless mode ────────────────────────────────────────────────────────────

/// Serve until killed. Exits non-zero when the port is taken: a supervisor or
/// smoke script needs that signal, whereas the tray shell deliberately stays
/// resident and shows the error in its status line instead.
fn run_headless() {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("tyutool-bridge: failed to start the async runtime: {e}");
            std::process::exit(1);
        }
    };
    runtime.block_on(async {
        let server = match bind(DEFAULT_PORT).await {
            Ok(server) => server,
            Err(e) => {
                eprintln!("tyutool-bridge: failed to start on 127.0.0.1:{DEFAULT_PORT}: {e:#}");
                std::process::exit(1);
            }
        };
        println!("tyutool-bridge listening on ws://127.0.0.1:{DEFAULT_PORT}");
        if let Err(e) = server.run().await {
            eprintln!("tyutool-bridge: server error: {e:#}");
            std::process::exit(1);
        }
    });
}

// ── Tray mode ────────────────────────────────────────────────────────────────

/// Everything the background runtime and the menu report back to the UI thread.
#[derive(Debug)]
enum UserEvent {
    /// New counters from the server's watch channel.
    Stats(StatsSnapshot),
    /// The server could not start; the status line becomes the error state.
    StartupFailed(String),
    /// A tray menu item was activated.
    Menu(muda::MenuId),
}

fn run_tray() {
    // macOS pins the whole menu-bar/NSApplication stack to the main thread, so
    // the event loop must be built here and the server pushed to a side thread
    // (not the other way round).
    // `mut` is consumed only by the macOS activation policy below.
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        // Menu-bar-only process: no Dock icon, no app switcher entry.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }
    let proxy = event_loop.create_proxy();

    // muda delivers menu activations on its own global channel; funnel them into
    // the event loop so all UI mutation happens in one place.
    let menu_proxy = proxy.clone();
    muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::Menu(event.id));
    }));

    register_autostart();

    // Detached on purpose: the tray owns the process lifetime, and quitting
    // tears the runtime down with it.
    let server_proxy = proxy.clone();
    let spawned = std::thread::Builder::new()
        .name("bridge-server".to_string())
        .spawn(move || serve_in_background(server_proxy));
    if let Err(e) = spawned {
        log::error!("bridge server thread could not be started: {e}");
        let _ = proxy.send_event(UserEvent::StartupFailed(format!("启动失败：{e}")));
    }

    let mut tray: Option<TrayShell> = None;
    let mut status_text = status::status_line(VERSION, &StatsSnapshot::default());

    event_loop.run(move |event, _target, control_flow| {
        // Purely event-driven: nothing to poll between stats pushes and clicks.
        *control_flow = ControlFlow::Wait;
        match event {
            // tao guarantees this is the first event, and on macOS the status
            // item may only be created once the app is initialized.
            Event::NewEvents(StartCause::Init) => match TrayShell::build(&status_text) {
                Ok(shell) => tray = Some(shell),
                // No icon means no menu, and no menu means no way to quit: the
                // process would keep serving from a UI loop that can never
                // receive an event, invisible and killable only from Activity
                // Monitor / taskkill. So fail loudly and name the mode that
                // works in a tray-less environment (a real case on Linux
                // desktops without a StatusNotifier host).
                //
                // Not "degrade to headless in place": `tao::EventLoop::run`
                // never returns (on macOS it exits the process), so there is no
                // after-the-loop to fall through to. Exiting non-zero also
                // makes the failure visible to whatever autostarts us, which a
                // silent resident process would not be.
                Err(e) => {
                    log::error!(
                        "bridge tray icon could not be created: {e:#}; no usable system tray \
                         in this environment — run `tyutool-bridge --headless` instead"
                    );
                    eprintln!(
                        "tyutool-bridge: no usable system tray ({e:#}); \
                         run `tyutool-bridge --headless` instead"
                    );
                    // The error line is the whole point of this exit; make sure
                    // it reached the log file before the process goes away.
                    log::logger().flush();
                    std::process::exit(1);
                }
            },
            Event::UserEvent(UserEvent::Stats(snapshot)) => {
                status_text = status::status_line(VERSION, &snapshot);
                if let Some(shell) = &tray {
                    shell.set_status(&status_text);
                }
            }
            Event::UserEvent(UserEvent::StartupFailed(text)) => {
                status_text = text;
                if let Some(shell) = &tray {
                    shell.set_status(&status_text);
                }
            }
            Event::UserEvent(UserEvent::Menu(id)) => {
                if let Some(shell) = &tray {
                    match shell.action_for(&id) {
                        Some(MenuAction::OpenCobuilder) => open_url(COBUILDER_URL),
                        Some(MenuAction::LatestVersion) => open_url(LATEST_VERSION_URL),
                        Some(MenuAction::Quit) => *control_flow = ControlFlow::Exit,
                        None => {}
                    }
                }
            }
            // Drop the status item explicitly: on macOS `run` ends the process
            // without unwinding, so relying on Drop would leave the icon behind.
            Event::LoopDestroyed => {
                tray = None;
                log::info!("bridge tray shell exiting");
            }
            _ => {}
        }
    });
}

/// Background runtime: bind, publish stats to the UI thread, serve forever.
fn serve_in_background(proxy: EventLoopProxy<UserEvent>) {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            log::error!("bridge async runtime could not be created: {e}");
            let _ = proxy.send_event(UserEvent::StartupFailed(format!("启动失败：{e}")));
            return;
        }
    };

    runtime.block_on(async move {
        let server = match bind(DEFAULT_PORT).await {
            Ok(server) => server,
            Err(e) => {
                // Resident on failure (unlike --headless): the whole point of
                // the tray is that the user finds out *why* nothing works.
                let diagnosis = status::diagnose_bind_error(&e);
                let line = status::startup_error_line(diagnosis, &e);
                // The status-line copy lands in the log too, so a bug report
                // shows exactly what the user was reading in the tray.
                log::error!(
                    "bridge failed to start on 127.0.0.1:{DEFAULT_PORT} ({diagnosis:?}): {e:#}; \
                     tray status line: {line}"
                );
                let _ = proxy.send_event(UserEvent::StartupFailed(line));
                return;
            }
        };
        log::info!("bridge listening on ws://127.0.0.1:{DEFAULT_PORT}");

        let (stats_tx, mut stats_rx) = tokio::sync::watch::channel(StatsSnapshot::default());
        let stats_proxy = proxy.clone();
        tokio::spawn(async move {
            while stats_rx.changed().await.is_ok() {
                let snapshot = *stats_rx.borrow_and_update();
                if stats_proxy.send_event(UserEvent::Stats(snapshot)).is_err() {
                    // The event loop is gone: the process is on its way out.
                    return;
                }
            }
        });

        if let Err(e) = server.run_with_stats(stats_tx).await {
            log::error!("bridge server stopped: {e:#}");
            let _ = proxy.send_event(UserEvent::StartupFailed(format!("服务已停止：{e}")));
        }
    });
}

/// What a tray menu item does. Kept separate from the muda ids so the event
/// handling above reads as behaviour rather than id comparisons.
enum MenuAction {
    OpenCobuilder,
    LatestVersion,
    Quit,
}

/// The live status item: icon, menu, and the ids needed to route clicks.
///
/// Held by the UI thread only — muda/tray-icon handles are not `Send`.
struct TrayShell {
    // Both handles must stay alive for the item to remain in the menu bar.
    _icon: tray_icon::TrayIcon,
    _menu: muda::Menu,
    status_item: muda::MenuItem,
    open_cobuilder: muda::MenuId,
    latest_version: muda::MenuId,
    quit: muda::MenuId,
}

impl TrayShell {
    fn build(status_text: &str) -> anyhow::Result<Self> {
        // Disabled: a status readout, not a command.
        let status_item = muda::MenuItem::new(status_text, false, None);
        let open_cobuilder = muda::MenuItem::new("打开 Cobuilder", true, None);
        let latest_version = muda::MenuItem::new("获取最新版本", true, None);
        let quit = muda::MenuItem::new("退出", true, None);

        let menu = muda::Menu::new();
        menu.append_items(&[
            &status_item,
            &muda::PredefinedMenuItem::separator(),
            &open_cobuilder,
            &latest_version,
            &muda::PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| anyhow::anyhow!("build tray menu: {e}"))?;

        let icon = tray_icon::TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_icon(placeholder_icon()?)
            // macOS recolors a template image for the current menu bar
            // appearance, which is what keeps a black glyph visible in dark mode.
            .with_icon_as_template(true)
            .with_tooltip("Cobuilder Bridge")
            .build()
            .map_err(|e| anyhow::anyhow!("create tray icon: {e}"))?;

        Ok(Self {
            _icon: icon,
            open_cobuilder: open_cobuilder.id().clone(),
            latest_version: latest_version.id().clone(),
            quit: quit.id().clone(),
            status_item,
            _menu: menu,
        })
    }

    fn set_status(&self, text: &str) {
        self.status_item.set_text(text);
    }

    fn action_for(&self, id: &muda::MenuId) -> Option<MenuAction> {
        if *id == self.open_cobuilder {
            Some(MenuAction::OpenCobuilder)
        } else if *id == self.latest_version {
            Some(MenuAction::LatestVersion)
        } else if *id == self.quit {
            Some(MenuAction::Quit)
        } else {
            None
        }
    }
}

/// Generated ring glyph (opaque black on transparent), drawn in code so the
/// binary needs no asset pipeline yet. Black + alpha is exactly what a macOS
/// template image wants; other platforms show it as-is.
///
/// TODO: replace with the real Cobuilder Bridge artwork (proper per-platform
/// icon set, `.ico` on Windows) when the design lands.
fn placeholder_icon() -> anyhow::Result<tray_icon::Icon> {
    const SIZE: u32 = 32;
    const OUTER: f32 = 14.0;
    const INNER: f32 = 8.0;
    let center = (SIZE as f32 - 1.0) / 2.0;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance_sq = dx * dx + dy * dy;
            let on_ring = (INNER * INNER..=OUTER * OUTER).contains(&distance_sq);
            rgba.extend_from_slice(if on_ring {
                &[0x00, 0x00, 0x00, 0xFF]
            } else {
                &[0x00, 0x00, 0x00, 0x00]
            });
        }
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE)
        .map_err(|e| anyhow::anyhow!("build tray icon bitmap: {e}"))
}

/// Hand the URL to the platform's default handler. `std::process::Command`
/// rather than a helper crate: one command per platform is the whole feature.
///
/// Waited on in a throwaway thread so the launcher process is reaped without
/// the UI thread ever blocking on it.
fn open_url(url: &'static str) {
    let spawned = std::thread::Builder::new()
        .name("bridge-open-url".to_string())
        .spawn(move || {
            #[cfg(target_os = "macos")]
            let mut command = {
                let mut c = std::process::Command::new("open");
                c.arg(url);
                c
            };
            #[cfg(target_os = "windows")]
            let mut command = {
                let mut c = std::process::Command::new("cmd");
                // Empty title argument: `start` treats a lone quoted argument
                // as the window title otherwise.
                c.args(["/C", "start", "", url]);
                c
            };
            #[cfg(all(unix, not(target_os = "macos")))]
            let mut command = {
                let mut c = std::process::Command::new("xdg-open");
                c.arg(url);
                c
            };

            match command.status() {
                Ok(status) if status.success() => {}
                Ok(status) => log::warn!("bridge could not open {url}: exit {status}"),
                Err(e) => log::warn!("bridge could not open {url}: {e}"),
            }
        });
    if let Err(e) = spawned {
        log::warn!("bridge could not spawn the URL opener for {url}: {e}");
    }
}

// ── Autostart ────────────────────────────────────────────────────────────────

/// Register the bridge to start with the user's session, once. Advisory only:
/// every failure is a warning, never a reason not to run.
///
/// TODO: once the bridge ships as a macOS .app bundle, switch to
/// `SMAppService` (`MacOSLaunchMode::SMAppService`, or the objc2 API directly)
/// — that is the packaging slice's job, together with cleaning up the
/// LaunchAgent plist this leaves behind when a user disables autostart.
fn register_autostart() {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            log::warn!("bridge autostart skipped, own path unknown: {e}");
            return;
        }
    };

    let builder = auto_launch::AutoLaunchBuilder::new()
        .set_app_name(AUTOSTART_APP_NAME)
        .set_app_path(&exe.to_string_lossy())
        // A LaunchAgent plist works for a bare binary; both the AppleScript
        // login item and `SMAppService` modes want a real .app bundle.
        .set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent)
        .build();

    let launcher = match builder {
        Ok(launcher) => launcher,
        Err(e) => {
            log::warn!("bridge autostart not configured: {e}");
            return;
        }
    };

    match launcher.is_enabled() {
        Ok(true) => log::info!("bridge autostart already registered"),
        Ok(false) => match launcher.enable() {
            Ok(()) => log::info!("bridge autostart registered for {}", exe.display()),
            Err(e) => log::warn!("bridge autostart registration failed: {e}"),
        },
        Err(e) => log::warn!("bridge autostart state unknown: {e}"),
    }
}
