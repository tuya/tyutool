//! Shared startup/session banner for CLI and GUI so both emit an identical,
//! issue-report-friendly block (version, OS, session id) from one source.

use std::path::Path;
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

/// Retention policy for a family of per-session `.log` files: how many files
/// to keep and how many total bytes they may occupy, and the filename prefix
/// (matched against `file_stem`) that identifies files this policy owns.
///
/// Each caller (CLI, GUI, bridge) has its own budget and prefix — see
/// `prune_log_files` for why the numbers must not be unified.
///
/// ⚠ The prefix match is a plain `starts_with`, not an exact segment match, so
/// if two policies ever point at the **same directory**, neither prefix may be
/// a prefix of the other (e.g. `"tyutool-"` is a prefix of
/// `"tyutool-bridge-"`). Otherwise the broader policy's prune pass will also
/// delete files that belong to the narrower one. CLI/GUI (`"tyutool-"`) and
/// bridge (`"tyutool-bridge-"`) are safe today only because they write to
/// different directories (`data_dir/tyutool/` vs. `data_dir/tyutool-bridge/`),
/// not because their prefixes are disjoint — don't point a new policy at either
/// of those directories without checking this.
pub struct LogRetention {
    pub prefix: &'static str,
    pub max_files: usize,
    pub max_bytes_total: u64,
}

/// Delete the oldest `.log` files under `dir` whose stem starts with
/// `policy.prefix`, until both `policy.max_files` and `policy.max_bytes_total`
/// are satisfied. Always keeps at least one file. Timestamped filenames sort
/// chronologically, so lexicographic order is age order. A stem that doesn't
/// start with `policy.prefix` at all — e.g. legacy `tyutool.log` under the
/// `"tyutool-"` policy — is left untouched, not just left alone once inside
/// the limits.
///
/// A file is only counted as removed when `remove_file` actually succeeds —
/// on failure (e.g. locked by another process) it is left in place and still
/// counts against both limits, so the loop keeps trying the next-oldest file
/// rather than under-counting a file that is still on disk.
pub fn prune_log_files(dir: &Path, policy: &LogRetention) {
    let mut files: Vec<(std::path::PathBuf, u64)> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|ext| ext == "log")
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| s.starts_with(policy.prefix))
            })
            .map(|p| {
                let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                (p, size)
            })
            .collect(),
        Err(_) => return,
    };

    files.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));

    let mut count = files.len();
    let mut total: u64 = files.iter().map(|(_, s)| s).sum();

    for (path, size) in &files {
        if count <= 1 || (count <= policy.max_files && total <= policy.max_bytes_total) {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            count -= 1;
            total = total.saturating_sub(*size);
        }
    }
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

    fn touch(dir: &Path, name: &str, size: u64) {
        let path = dir.join(name);
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(size).unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        v.sort();
        v
    }

    const TEST_POLICY: LogRetention = LogRetention {
        prefix: "tyutool-",
        max_files: 5,
        max_bytes_total: 50 * 1024 * 1024,
    };

    #[test]
    fn prune_removes_oldest_when_over_count_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 1..=(TEST_POLICY.max_files + 2) {
            touch(dir.path(), &format!("tyutool-{i:06}.log"), 1024);
        }
        prune_log_files(dir.path(), &TEST_POLICY);
        assert_eq!(names(dir.path()).len(), TEST_POLICY.max_files);
        assert!(!dir.path().join("tyutool-000001.log").exists());
        assert!(!dir.path().join("tyutool-000002.log").exists());
        assert!(dir.path().join("tyutool-000007.log").exists());
    }

    #[test]
    fn prune_removes_oldest_when_over_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        // 5 files each 15 MB = 75 MB, over the 50 MB TEST_POLICY budget.
        for i in 1..=5 {
            touch(dir.path(), &format!("tyutool-{i:06}.log"), 15 * 1024 * 1024);
        }
        prune_log_files(dir.path(), &TEST_POLICY);
        let remaining = names(dir.path());
        let total: u64 = remaining
            .iter()
            .map(|n| std::fs::metadata(dir.path().join(n)).unwrap().len())
            .sum();
        assert!(total <= TEST_POLICY.max_bytes_total);
        // Oldest file must be the one removed.
        assert!(!dir.path().join("tyutool-000001.log").exists());
    }

    #[test]
    fn prune_always_keeps_at_least_one_file() {
        let dir = tempfile::tempdir().unwrap();
        touch(
            dir.path(),
            "tyutool-000001.log",
            TEST_POLICY.max_bytes_total + 1,
        );
        prune_log_files(dir.path(), &TEST_POLICY);
        assert_eq!(names(dir.path()).len(), 1);
    }

    #[test]
    fn prune_ignores_files_with_a_different_prefix_or_extension() {
        let dir = tempfile::tempdir().unwrap();
        for i in 1..=(TEST_POLICY.max_files + 3) {
            touch(dir.path(), &format!("tyutool-{i:06}.log"), 1024);
        }
        // Neither the trace file nor the differently-prefixed log file matches
        // TEST_POLICY's prefix/extension filter, so both must survive.
        touch(dir.path(), "batch-auth-000001.trace", 1024);
        touch(dir.path(), "other-000001.log", 1024);
        prune_log_files(dir.path(), &TEST_POLICY);
        assert!(dir.path().join("batch-auth-000001.trace").exists());
        assert!(dir.path().join("other-000001.log").exists());
    }

    #[test]
    fn different_policies_do_not_interfere_in_the_same_directory() {
        // Prefixes are deliberately non-overlapping (neither is a prefix of the
        // other), unlike the real "tyutool-" / "tyutool-bridge-" pair, so this
        // test isolates cross-policy interference from the substring-prefix
        // matching rule itself (covered separately above).
        let dir = tempfile::tempdir().unwrap();
        let other_policy = LogRetention {
            prefix: "other-app-",
            max_files: 2,
            max_bytes_total: 10 * 1024 * 1024,
        };
        for i in 1..=7 {
            touch(dir.path(), &format!("tyutool-{i:06}.log"), 1024);
        }
        for i in 1..=4 {
            touch(dir.path(), &format!("other-app-{i:06}.log"), 1024);
        }
        prune_log_files(dir.path(), &TEST_POLICY);
        prune_log_files(dir.path(), &other_policy);

        let remaining = names(dir.path());
        let plain_count = remaining
            .iter()
            .filter(|n| n.starts_with("tyutool-"))
            .count();
        let other_count = remaining
            .iter()
            .filter(|n| n.starts_with("other-app-"))
            .count();
        assert_eq!(plain_count, TEST_POLICY.max_files);
        assert_eq!(other_count, other_policy.max_files);
    }
}
