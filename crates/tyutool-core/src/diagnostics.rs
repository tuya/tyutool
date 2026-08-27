//! Shared session diagnostics for CLI and GUI: the startup/session banner
//! (identical, issue-report-friendly block: version, OS, session id) and
//! `.log`/`.trace` file retention, selection, and reading.

use std::path::{Path, PathBuf};
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

/// Bounded growth for `.trace` files (plaintext batch-auth interaction data).
/// Independent from `.log` limits — `.trace` is never collected into any
/// export/archive zip (it has no `tyutool-` prefix and a non-`.log` extension).
const MAX_TRACE_FILES: usize = 20;

/// Delete the oldest `batch-auth-*.trace` files until at most `MAX_TRACE_FILES`
/// remain. Independent from `prune_log_files` (different prefix/extension).
pub fn prune_trace_files(log_dir: &Path) {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(log_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().map(|x| x == "trace").unwrap_or(false)
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.starts_with("batch-auth-"))
                        .unwrap_or(false)
            })
            .collect(),
        Err(_) => return,
    };
    // Timestamped filenames are lexicographically chronological; oldest first.
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    while files.len() > MAX_TRACE_FILES.saturating_sub(1) {
        let removed = files.remove(0);
        let _ = std::fs::remove_file(removed);
    }
}

/// Return the most recently modified `*.log` file in `dir`.
fn pick_active_log(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "log").unwrap_or(false))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Read the last `max_bytes` bytes of `path` as UTF-8 (lossy).
fn tail_bytes(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let len = std::fs::metadata(path)?.len();
    let start = len.saturating_sub(max_bytes);
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Select the active log in `dir` and return its last `max_bytes` bytes.
/// Pure (no AppHandle): pick + tail folded into one testable unit.
pub fn read_log_tail_impl(dir: &Path, max_bytes: u64) -> std::io::Result<String> {
    let path = pick_active_log(dir)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no log file found"))?;
    tail_bytes(&path, max_bytes)
}

/// Validate a user-supplied log filename against the same gate used by the
/// log viewer / opener: no path separators, must end in `.log`, and must carry
/// the `tyutool` prefix. This keeps the read path from ever returning the
/// plaintext `.trace` credential files (which share `app_log_dir`).
fn validate_log_filename(filename: &str) -> std::io::Result<()> {
    if filename.contains('/') || filename.contains('\\') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "filename must not contain path separators",
        ));
    }
    if !filename.ends_with(".log") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "only .log files can be opened",
        ));
    }
    if !filename.starts_with("tyutool") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "only tyutool log files can be opened",
        ));
    }
    Ok(())
}

pub fn read_named_log_impl(dir: &Path, filename: &str, max_bytes: u64) -> std::io::Result<String> {
    validate_log_filename(filename)?;
    tail_bytes(&dir.join(filename), max_bytes)
}

pub fn resolve_log_open_path(dir: &Path, filename: &str) -> std::io::Result<PathBuf> {
    validate_log_filename(filename)?;
    let path = dir.join(filename);
    if !path.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "log file not found",
        ));
    }
    Ok(path)
}

pub fn collect_log_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "log").unwrap_or(false))
        .collect()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileInfo {
    name: String,
    size_bytes: u64,
    modified_ms: i64,
}

pub fn list_log_files_impl(dir: &Path) -> Vec<LogFileInfo> {
    let mut files: Vec<LogFileInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|x| x == "log").unwrap_or(false)
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with("tyutool"))
                    .unwrap_or(false)
        })
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            let name = p.file_name()?.to_str()?.to_owned();
            let size_bytes = meta.len();
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            Some(LogFileInfo {
                name,
                size_bytes,
                modified_ms,
            })
        })
        .collect();
    files.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| b.name.cmp(&a.name))
    });
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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

    #[test]
    fn prune_trace_files_keeps_newest_and_ignores_logs() {
        let dir = tempfile::tempdir().unwrap();
        // 25 trace files + 1 unrelated log file.
        for i in 0..25 {
            let name = format!("batch-auth-202601{:02}-000000.trace", i + 1);
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::write(dir.path().join("tyutool-old.log"), b"x").unwrap();
        prune_trace_files(dir.path());
        let traces: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "trace").unwrap_or(false))
            .collect();
        // Keeps the newest MAX_TRACE_FILES-1 (the latest by lexicographic order).
        assert_eq!(traces.len(), MAX_TRACE_FILES - 1);
        // The log file is untouched.
        assert!(dir.path().join("tyutool-old.log").exists());
    }

    #[test]
    fn pick_active_log_returns_newest_by_mtime() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tyutool-20260618-100000.log"), b"old").unwrap();
        // Sleep so the second file has a strictly newer mtime on all platforms.
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(dir.path().join("tyutool-20260629-120000.log"), b"current").unwrap();
        let picked = pick_active_log(dir.path()).unwrap();
        assert_eq!(picked.file_name().unwrap(), "tyutool-20260629-120000.log");
    }

    #[test]
    fn tail_bytes_returns_last_n() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.log");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(b"0123456789").unwrap();
        let tail = tail_bytes(&p, 4).unwrap();
        assert_eq!(tail, "6789");
    }

    #[test]
    fn pick_active_log_falls_back_to_a_log_when_no_exact_name() {
        let dir = tempfile::tempdir().unwrap();
        // No "tyutool.log" present, only timestamped rotations + a non-log file.
        std::fs::write(dir.path().join("tyutool_old.log"), b"old").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();

        let picked = pick_active_log(dir.path()).unwrap();
        assert_eq!(picked.extension().unwrap(), "log");
    }

    #[test]
    fn pick_active_log_returns_none_for_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        assert!(pick_active_log(dir.path()).is_none());
    }

    #[test]
    fn tail_bytes_returns_whole_file_when_max_exceeds_len() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.log");
        std::fs::write(&p, b"abc").unwrap();
        let tail = tail_bytes(&p, 1000).unwrap();
        assert_eq!(tail, "abc");
    }

    #[test]
    fn collect_log_files_filters_only_logs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.log"), b"x").unwrap();
        std::fs::write(dir.path().join("b.log"), b"x").unwrap();
        std::fs::write(dir.path().join("c.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("readme"), b"x").unwrap();
        // Plaintext batch-auth interaction data lives in .trace files, which
        // must NEVER be collected into an export/archive zip.
        std::fs::write(
            dir.path().join("batch-auth-20260804-120000.trace"),
            b"secret",
        )
        .unwrap();

        let files = collect_log_files(dir.path());

        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .all(|p| p.extension().map(|x| x == "log").unwrap_or(false)));
        assert!(!files
            .iter()
            .any(|p| p.extension().map(|x| x == "trace").unwrap_or(false)));
    }

    #[test]
    fn collect_log_files_returns_empty_for_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(collect_log_files(&missing).is_empty());
    }

    #[test]
    fn batch_auth_trace_file_not_collected_by_collect_log_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tyutool.log"), b"x").unwrap();
        std::fs::write(
            dir.path().join("batch-auth-20260804-120000.trace"),
            b"secret",
        )
        .unwrap();
        let files = collect_log_files(dir.path());
        assert!(files
            .iter()
            .all(|p| { p.extension().map(|x| x == "log").unwrap_or(false) }));
        assert!(!files
            .iter()
            .any(|p| p.to_string_lossy().contains("batch-auth")));
    }

    #[test]
    fn read_log_tail_impl_reads_active_log_tail() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tyutool.log"), b"0123456789").unwrap();
        let tail = read_log_tail_impl(dir.path(), 4).unwrap();
        assert_eq!(tail, "6789");
    }

    #[test]
    fn read_log_tail_impl_errors_when_no_log() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        let err = read_log_tail_impl(dir.path(), 100).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn list_log_files_impl_returns_tyutool_logs_sorted_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        // Create two tyutool-* logs with different sizes (mtime may be identical in fast tests)
        std::fs::write(dir.path().join("tyutool-20250101-100000.log"), b"old").unwrap();
        std::fs::write(dir.path().join("tyutool-20250629-120000.log"), b"new").unwrap();
        // Non-matching file must be excluded
        std::fs::write(dir.path().join("other.log"), b"x").unwrap();

        let files = list_log_files_impl(dir.path());
        // Must include only tyutool*.log files
        assert!(files.iter().all(|f| f.name.starts_with("tyutool")));
        assert!(!files.iter().any(|f| f.name == "other.log"));
        assert_eq!(files.len(), 2);
        // Newest-first: secondary sort by name descending when mtime is equal
        assert_eq!(files[0].name, "tyutool-20250629-120000.log");
    }

    #[test]
    fn list_log_files_impl_returns_empty_for_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_log_files_impl(dir.path()).is_empty());
    }

    #[test]
    fn list_log_files_impl_size_bytes_matches_file_size() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tyutool.log"), b"hello").unwrap();
        let files = list_log_files_impl(dir.path());
        assert_eq!(files[0].size_bytes, 5);
    }

    #[test]
    fn read_named_log_impl_reads_specified_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tyutool-old.log"), b"old content").unwrap();
        std::fs::write(dir.path().join("tyutool-new.log"), b"new content").unwrap();
        let result = read_named_log_impl(dir.path(), "tyutool-old.log", 1000).unwrap();
        assert_eq!(result, "old content");
    }

    #[test]
    fn read_named_log_impl_rejects_path_traversal_forward_slash() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_named_log_impl(dir.path(), "../secret.log", 100).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn read_named_log_impl_rejects_path_traversal_backslash() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_named_log_impl(dir.path(), "..\\secret.log", 100).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn read_named_log_impl_errors_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_named_log_impl(dir.path(), "tyutool-ghost.log", 100).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn read_named_log_impl_rejects_trace_credential_files() {
        // `.trace` files (batch-auth plaintext UUID/AuthKey) share app_log_dir
        // but must never be readable via read_named_log_impl. They fail both the
        // `.log` suffix gate and the `tyutool` prefix gate.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("batch-auth-20260805-120000.trace"),
            b"secret",
        )
        .unwrap();
        let err =
            read_named_log_impl(dir.path(), "batch-auth-20260805-120000.trace", 100).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn resolve_log_open_path_accepts_tyutool_log_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tyutool-20260708.log"), b"hello").unwrap();

        let path = resolve_log_open_path(dir.path(), "tyutool-20260708.log").unwrap();

        assert_eq!(path.file_name().unwrap(), "tyutool-20260708.log");
    }

    #[test]
    fn resolve_log_open_path_rejects_non_log_extension() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_log_open_path(dir.path(), "tyutool-20260708.txt").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn resolve_log_open_path_rejects_non_tyutool_log_names() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_log_open_path(dir.path(), "notes.log").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
