//! Tray-facing runtime status: connection/device counters surfaced in the
//! status bar menu, and startup error diagnosis (single-instance detection).

use crate::lang::Lang;

/// Snapshot of the bridge's observable runtime state, published on a watch
/// channel for the tray shell (and tests) to consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatsSnapshot {
    /// Active WS connections (post-handshake).
    pub connections: usize,
    /// Discovered devices, allowlisted VIDs only (matches the web UI's
    /// "已发现 N 个设备" semantics — non-allowlisted ports are not devices a
    /// user can flash).
    pub devices: usize,
}

/// Status line shown in the tray menu (updated on every stats change).
pub fn status_line(version: &str, snapshot: &StatsSnapshot, lang: Lang) -> String {
    let StatsSnapshot {
        connections,
        devices,
    } = *snapshot;
    match lang {
        Lang::Zh => format!("v{version} · 连接 {connections} · 设备 {devices}"),
        Lang::En => format!("v{version} · Connections {connections} · Devices {devices}"),
    }
}

/// Why the bridge failed to start listening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupDiagnosis {
    /// The fixed port is taken — by another bridge instance (single-instance
    /// rule) or a foreign process. Either way the resident shell shows the
    /// "already running / port occupied" error state.
    AlreadyRunning,
    /// Anything else (permissions, unexpected I/O failure, ...).
    Other,
}

/// Classify a `bind` failure by walking the error chain for `AddrInUse`.
///
/// The chain rather than the outermost error: `bind` wraps the raw `io::Error`
/// in a `.context("failed to bind …")`, and further layers may be added by
/// callers, so the I/O kind can sit at any depth.
pub fn diagnose_bind_error(error: &anyhow::Error) -> StartupDiagnosis {
    for cause in error.chain() {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            if io.kind() == std::io::ErrorKind::AddrInUse {
                return StartupDiagnosis::AlreadyRunning;
            }
        }
    }
    StartupDiagnosis::Other
}

/// Error state rendered in the status line when the bridge could not start.
///
/// Trade-off (technical design): the resident tray shell stays alive with this
/// text instead of exiting, so the user can see *why* nothing works and quit
/// deliberately; `--headless` keeps `exit(1)` because a supervisor / CI script
/// needs the non-zero status.
///
/// The same text is also pushed as a system notification, so a user who never
/// opens the menu still finds out (the tray shell fires it on
/// `UserEvent::StartupFailed`). Kept out of here on purpose: this function stays
/// pure so it can be unit-tested, and firing a notification is the shell's job.
pub fn startup_error_line(
    diagnosis: StartupDiagnosis,
    error: &anyhow::Error,
    lang: Lang,
) -> String {
    match diagnosis {
        StartupDiagnosis::AlreadyRunning => match lang {
            Lang::Zh => "已有实例在运行 / 端口被占用".to_string(),
            Lang::En => "Already running / port in use".to_string(),
        },
        StartupDiagnosis::Other => startup_failed_line(error, lang),
    }
}

/// Status line for a bridge that never came up, from any cause the tray shell
/// discovers outside `bind` (no async runtime, no server thread).
///
/// Shared with [`startup_error_line`]'s catch-all arm rather than inlined at
/// each site: three call sites used to format this sentence themselves, which is
/// exactly how one of them gets left behind when the wording changes.
pub fn startup_failed_line(detail: impl std::fmt::Display, lang: Lang) -> String {
    match lang {
        Lang::Zh => format!("启动失败：{detail}"),
        Lang::En => format!("Startup failed: {detail}"),
    }
}

/// Status line for a server that *had* come up and then stopped — a different
/// statement from [`startup_failed_line`], because the port was reachable.
pub fn server_stopped_line(detail: impl std::fmt::Display, lang: Lang) -> String {
    match lang {
        Lang::Zh => format!("服务已停止：{detail}"),
        Lang::En => format!("Server stopped: {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUSY: StatsSnapshot = StatsSnapshot {
        connections: 2,
        devices: 1,
    };
    const IDLE: StatsSnapshot = StatsSnapshot {
        connections: 0,
        devices: 0,
    };

    #[test]
    fn status_line_shows_version_connections_and_devices() {
        assert_eq!(
            status_line("0.1.0", &BUSY, Lang::Zh),
            "v0.1.0 · 连接 2 · 设备 1"
        );
    }

    #[test]
    fn status_line_idle_state() {
        assert_eq!(
            status_line("0.1.0", &IDLE, Lang::Zh),
            "v0.1.0 · 连接 0 · 设备 0"
        );
    }

    /// The tray menu's only always-visible text, so it is also the first thing
    /// that gives away an unlocalized build.
    #[test]
    fn the_english_status_line_shows_version_connections_and_devices() {
        assert_eq!(
            status_line("0.1.0", &BUSY, Lang::En),
            "v0.1.0 · Connections 2 · Devices 1"
        );
        assert_eq!(
            status_line("0.1.0", &IDLE, Lang::En),
            "v0.1.0 · Connections 0 · Devices 0"
        );
    }

    /// All four failure lines, in both languages, in one place: the tray shell
    /// reaches these through three separate paths (bind failure, no runtime, the
    /// server stopping), and each used to format its own sentence.
    #[test]
    fn every_failure_status_line_follows_the_system_language() {
        let io = std::io::Error::new(std::io::ErrorKind::AddrInUse, "address in use");
        let taken = anyhow::Error::from(io).context("failed to bind 127.0.0.1:18730");

        assert_eq!(
            startup_error_line(StartupDiagnosis::AlreadyRunning, &taken, Lang::Zh),
            "已有实例在运行 / 端口被占用"
        );
        assert_eq!(
            startup_error_line(StartupDiagnosis::AlreadyRunning, &taken, Lang::En),
            "Already running / port in use"
        );

        assert_eq!(startup_failed_line("boom", Lang::Zh), "启动失败：boom");
        assert_eq!(
            startup_failed_line("boom", Lang::En),
            "Startup failed: boom"
        );
        // The catch-all arm is the same sentence, so it must not drift from it.
        assert_eq!(
            startup_error_line(StartupDiagnosis::Other, &anyhow::anyhow!("boom"), Lang::En),
            startup_failed_line("boom", Lang::En)
        );

        // A server that stopped is a different statement from one that never
        // started: the port was reachable, so the two must not share wording.
        assert_eq!(server_stopped_line("boom", Lang::Zh), "服务已停止：boom");
        assert_eq!(
            server_stopped_line("boom", Lang::En),
            "Server stopped: boom"
        );
    }

    #[test]
    fn addr_in_use_diagnoses_as_already_running() {
        let io = std::io::Error::new(std::io::ErrorKind::AddrInUse, "address in use");
        let err = anyhow::Error::from(io).context("failed to bind 127.0.0.1:18730");
        assert_eq!(diagnose_bind_error(&err), StartupDiagnosis::AlreadyRunning);
    }

    #[test]
    fn other_io_errors_diagnose_as_other() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = anyhow::Error::from(io).context("failed to bind 127.0.0.1:18730");
        assert_eq!(diagnose_bind_error(&err), StartupDiagnosis::Other);
    }
}
