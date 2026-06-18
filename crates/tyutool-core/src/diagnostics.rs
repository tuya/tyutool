//! Shared startup/session banner for CLI and GUI so both emit an identical,
//! issue-report-friendly block (version, OS, session id) from one source.

use std::time::{SystemTime, UNIX_EPOCH};

/// Build the session banner lines and a session id. Pure (no logging) so it is testable.
/// `app_type` is "CLI" or "GUI"; `install_type`/`exe` are included only when `Some`.
pub fn build_session_banner(
    name: &str,
    app_type: &str,
    version: &str,
    install_type: Option<&str>,
    exe: Option<&str>,
) -> (String, Vec<String>) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let session_id = format!("{:x}-{:x}", secs, std::process::id());

    let mut lines = vec![
        "========================================".to_string(),
        format!("===== SESSION {session_id} ====="),
        format!("[App] {name} v{version} starting"),
        format!("[App] Type: {app_type}"),
        format!(
            "[App] OS: {}, Arch: {}, Family: {}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::consts::FAMILY
        ),
    ];
    if let Some(install) = install_type {
        lines.push(format!("[App] Install: {install}"));
    }
    if let Some(exe) = exe {
        lines.push(format!("[App] Exe: {exe}"));
    }
    lines.push("========================================".to_string());
    (session_id, lines)
}

/// Emit the session banner via `log::info!` and return the session id.
pub fn log_session_banner(
    name: &str,
    app_type: &str,
    version: &str,
    install_type: Option<&str>,
) -> String {
    let exe = std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string());
    let (session_id, lines) =
        build_session_banner(name, app_type, version, install_type, exe.as_deref());
    for line in &lines {
        log::info!("{line}");
    }
    session_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_includes_session_and_metadata() {
        let (sid, lines) =
            build_session_banner("tyutool", "CLI", "3.0.11", None, Some("/usr/bin/tyutool"));
        assert!(!sid.is_empty(), "session id must be non-empty");
        assert!(lines.iter().any(|l| l.contains(&format!("SESSION {sid}"))));
        assert!(lines.iter().any(|l| l.contains("v3.0.11")));
        assert!(lines.iter().any(|l| l.contains("Type: CLI")));
        assert!(lines.iter().any(|l| l.contains("/usr/bin/tyutool")));
        assert!(lines.iter().all(|l| !l.contains("Install")));
    }

    #[test]
    fn banner_includes_install_when_present() {
        let (_sid, lines) =
            build_session_banner("tyutool", "GUI", "3.0.11", Some("AppImage"), None);
        assert!(lines.iter().any(|l| l.contains("Install: AppImage")));
    }
}
