//! Tray-facing runtime status: connection/device counters surfaced in the
//! status bar menu, and startup error diagnosis (single-instance detection).

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
pub fn status_line(version: &str, snapshot: &StatsSnapshot) -> String {
    let StatsSnapshot {
        connections,
        devices,
    } = *snapshot;
    format!("v{version} · 连接 {connections} · 设备 {devices}")
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
/// TODO: a native system notification would surface the failure without the
/// user opening the menu, but on macOS that needs an .app bundle (or a new
/// notification dependency) — revisit in the packaging slice.
pub fn startup_error_line(diagnosis: StartupDiagnosis, error: &anyhow::Error) -> String {
    match diagnosis {
        StartupDiagnosis::AlreadyRunning => "已有实例在运行 / 端口被占用".to_string(),
        StartupDiagnosis::Other => format!("启动失败：{error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_shows_version_connections_and_devices() {
        let line = status_line(
            "0.1.0",
            &StatsSnapshot {
                connections: 2,
                devices: 1,
            },
        );
        assert_eq!(line, "v0.1.0 · 连接 2 · 设备 1");
    }

    #[test]
    fn status_line_idle_state() {
        let line = status_line(
            "0.1.0",
            &StatsSnapshot {
                connections: 0,
                devices: 0,
            },
        );
        assert_eq!(line, "v0.1.0 · 连接 0 · 设备 0");
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
