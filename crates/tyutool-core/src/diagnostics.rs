//! Shared session diagnostics for CLI and GUI: the startup/session banner
//! (identical, issue-report-friendly block: version, OS, session id),
//! `.log`/`.trace` file retention, selection, and reading, and the
//! report-info header / credential redaction / zip export used by the GUI's
//! export-for-report and archive-batch flows.
//!
//! `build_report_info` / `REDACT_PREFIXES` / `redact_log_content` have no
//! `zip` dependency and are always available. `write_logs_zip` and
//! `gather_and_write_logs_zip` pull in the `zip` crate and are gated behind
//! the `zip` Cargo feature, per the crate boundary rule in AGENTS.md, so
//! `tyutool-serve` and `tyutool-bridge` don't link it.
//!
//! Two guarantees documented in AGENTS.md are covered by the tests here:
//!
//! * `batch-auth-*.trace` files hold plaintext credential interaction data and
//!   must never reach an export or archive zip. `collect_log_files` and the
//!   `.trace`-reading gates in this module exclude them by extension and
//!   prefix.
//! * The export path redacts credentials (`write_logs_zip` `mask = true`); the
//!   archive path keeps plaintext because it is the operator's local bundle.

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

pub fn build_report_info(name: &str, version: &str, install: &str, session_id: &str) -> String {
    format!(
        "tyutool report-info\nname: {name}\nversion: {version}\nos: {}\narch: {}\nfamily: {}\ninstall: {install}\nsession: {session_id}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY,
    )
}

/// Credential-bearing field prefixes that tyutool itself emits into log lines
/// (see `authorize.rs` format strings). Redaction matches these prefixes and
/// masks the value that follows, without assuming any UUID/AuthKey shape —
/// device identifiers vary in length (12/16/20 chars and others).
pub const REDACT_PREFIXES: &[&str] = &["uuid=", "authkey=", "existing_uuid=", "otp_uuid="];

/// Mask a credential value that starts at `value_start` in `s`. The value runs
/// until the next whitespace, comma, or closing paren — the delimiters tyutool's
/// own format strings use. Returns the index just past the consumed value.
fn mask_value_range(s: &str, value_start: usize) -> (usize, &'static str) {
    let bytes = s.as_bytes();
    let mut end = value_start;
    while end < bytes.len() {
        let b = bytes[end];
        if b == b' ' || b == b',' || b == b')' || b == b'\n' || b == b'\r' || b == b'\t' {
            break;
        }
        end += 1;
    }
    (end, "****")
}

/// Redact known-prefix credential values in a log file's text content. Used
/// only on the export-for-report path (`mask = true`); the archive path keeps
/// plaintext for local diagnosis. Pure & string-based — no regex dependency.
pub fn redact_log_content(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Try to match a redaction prefix at the current position.
        let matched = REDACT_PREFIXES.iter().find_map(|pfx| {
            if content[i..].starts_with(pfx) {
                Some(*pfx)
            } else {
                None
            }
        });
        if let Some(pfx) = matched {
            out.push_str(pfx);
            let value_start = i + pfx.len();
            let (end, mask) = mask_value_range(content, value_start);
            out.push_str(mask);
            i = end;
        } else {
            // Push the next byte (UTF-8 safe: boundaries respected by pushing
            // one char at a time when the byte starts a non-ASCII sequence).
            let ch = content[i..].chars().next().expect("non-empty tail");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

#[cfg(feature = "zip")]
fn write_logs_zip(
    log_files: &[PathBuf],
    report_info: &str,
    dest: &Path,
    mask: bool,
) -> Result<(), String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zw.start_file("report-info.txt", opts)
        .map_err(|e| e.to_string())?;
    zw.write_all(report_info.as_bytes())
        .map_err(|e| e.to_string())?;
    for p in log_files {
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
            let bytes = if mask {
                redact_log_content(&String::from_utf8_lossy(&bytes)).into_bytes()
            } else {
                bytes
            };
            zw.start_file(name, opts).map_err(|e| e.to_string())?;
            zw.write_all(&bytes).map_err(|e| e.to_string())?;
        }
    }
    zw.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Gather `*.log` files from `dir`, build the report header, and write the zip
/// to `dest`. `mask = true` redacts credential values (export-for-report);
/// `mask = false` keeps plaintext (archive — local troubleshooting bundle).
/// Pure (no AppHandle): collect + build + write folded into one unit.
#[cfg(feature = "zip")]
pub fn gather_and_write_logs_zip(
    dir: &Path,
    name: &str,
    version: &str,
    install: &str,
    session_id: &str,
    dest: &Path,
    mask: bool,
) -> Result<(), String> {
    let files = collect_log_files(dir);
    let info = build_report_info(name, version, install, session_id);
    write_logs_zip(&files, &info, dest, mask)
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

    #[test]
    fn build_report_info_contains_expected_fields() {
        let info = build_report_info("tyutool", "3.0.11", "AppImage", "abc-123");

        assert!(info.contains("name: tyutool"));
        assert!(info.contains("version: 3.0.11"));
        assert!(info.contains("install: AppImage"));
        assert!(info.contains("session: abc-123"));
        assert!(info.contains(&format!("os: {}", std::env::consts::OS)));
        assert!(info.contains(&format!("arch: {}", std::env::consts::ARCH)));
        assert!(info.contains(&format!("family: {}", std::env::consts::FAMILY)));
    }

    // `redact_log_content` has no `zip` dependency and must stay tested even
    // when the `zip` feature is off — it backs a security contract (AGENTS.md:
    // export-for-report must redact credential values by known prefix). These
    // tests call it directly, not through `write_logs_zip`.

    /// Each of the four known credential prefixes is redacted.
    #[test]
    fn redact_log_content_masks_each_known_prefix() {
        let content = "uuid=uuidval authkey=keyval existing_uuid=existval otp_uuid=otpval\n";
        let redacted = redact_log_content(content);
        assert!(!redacted.contains("uuidval"));
        assert!(!redacted.contains("keyval"));
        assert!(!redacted.contains("existval"));
        assert!(!redacted.contains("otpval"));
        assert!(redacted.contains("uuid=****"));
        assert!(redacted.contains("authkey=****"));
        assert!(redacted.contains("existing_uuid=****"));
        assert!(redacted.contains("otp_uuid=****"));
    }

    /// UUID/AuthKey values are not a fixed length (devices return 12/16/20+
    /// chars per AGENTS.md) — redaction must be by prefix only, never by
    /// assuming a specific value length.
    #[test]
    fn redact_log_content_masks_regardless_of_value_length() {
        let content = "uuid=abcdef123456 uuid=abcdef1234567890 uuid=abcdef1234567890abcd\n";
        let redacted = redact_log_content(content);
        assert!(!redacted.contains("abcdef123456"));
        assert!(!redacted.contains("abcdef1234567890"));
        assert!(!redacted.contains("abcdef1234567890abcd"));
        assert_eq!(redacted, "uuid=**** uuid=**** uuid=****\n");
    }

    /// Content with no credential-bearing prefix is returned unchanged.
    #[test]
    fn redact_log_content_preserves_non_credential_lines() {
        let content = "[info] tyutool v3.2.8 starting\n[serial] port=COM3 opened\n";
        assert_eq!(redact_log_content(content), content);
    }

    /// Actual (not assumed) behavior: the scanner checks every byte position
    /// for a prefix match — it is not anchored to word/line boundaries, so a
    /// prefix embedded inside a larger token still triggers redaction.
    #[test]
    fn redact_log_content_matches_prefix_anywhere_not_only_at_word_start() {
        let content = "prefixuuid=embedded-value\n";
        let redacted = redact_log_content(content);
        assert!(!redacted.contains("embedded-value"));
        assert_eq!(redacted, "prefixuuid=****\n");
    }
}

/// Tests for the `zip`-gated export helpers (`write_logs_zip` /
/// `gather_and_write_logs_zip`). Split from `mod tests` above because these
/// require the `zip` feature; the rest of this module's tests must keep
/// passing when `zip` is disabled.
#[cfg(all(test, feature = "zip"))]
mod zip_export_tests {
    use super::*;

    #[test]
    fn write_logs_zip_includes_logs_and_report() {
        let dir = tempfile::tempdir().unwrap();
        let log_a = dir.path().join("tyutool.log");
        std::fs::write(&log_a, b"hello log").unwrap();
        let dest = dir.path().join("out.zip");

        write_logs_zip(&[log_a], "report-body", &dest, false).unwrap();

        let f = std::fs::File::open(&dest).unwrap();
        let mut zip = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"report-info.txt".to_string()));
        assert!(names.contains(&"tyutool.log".to_string()));
    }

    /// Helper: write `content` to a single `tyutool.log` in a temp dir, zip it
    /// via `write_logs_zip(mask)`, then return the zipped log's text content.
    fn write_and_read_zipped_log(content: &str, mask: bool) -> String {
        let dir = tempfile::tempdir().unwrap();
        let log_a = dir.path().join("tyutool.log");
        std::fs::write(&log_a, content.as_bytes()).unwrap();
        let dest = dir.path().join("out.zip");
        write_logs_zip(&[log_a], "report-body", &dest, mask).unwrap();
        let f = std::fs::File::open(&dest).unwrap();
        let mut zip = zip::ZipArchive::new(f).unwrap();
        let mut entry = zip.by_name("tyutool.log").unwrap();
        let mut out = String::new();
        use std::io::Read;
        entry.read_to_string(&mut out).unwrap();
        out
    }

    /// Archive path (mask=false): credential-bearing log lines survive verbatim.
    /// The archive is the operator's local troubleshooting bundle and must keep
    /// real UUID/AuthKey values for diagnosis.
    #[test]
    fn write_logs_zip_mask_false_preserves_plaintext_credentials() {
        let content = "[batch-auth] allocated  port=COM3 mac=AA uuid=plaintext-uuid-value\n";
        let zipped = write_and_read_zipped_log(content, false);
        assert!(zipped.contains("uuid=plaintext-uuid-value"));
    }

    /// Export-for-report path (mask=true): known-prefix credential values are
    /// redacted. UUID form is NOT assumed — any length after `uuid=` is masked.
    #[test]
    fn write_logs_zip_mask_true_redacts_uuid_and_authkey_by_known_prefix() {
        let content =
            "[batch-auth] allocated  port=COM3 uuid=plaintext-uuid-value\n\
              [batch-auth] verify-fail  reason=wrote (uuid=plaintext-uuid-value, authkey=secretkey)\n\
              [batch-auth] skipped  port=COM3 existing_uuid=another-uuid\n\
              [batch-auth] auth-write failed  otp_uuid=otp-uuid-here\n";
        let zipped = write_and_read_zipped_log(content, true);
        assert!(!zipped.contains("plaintext-uuid-value"));
        assert!(!zipped.contains("secretkey"));
        assert!(!zipped.contains("another-uuid"));
        assert!(!zipped.contains("otp-uuid-here"));
        // Prefixes themselves remain (the line is still legible as a log line).
        assert!(zipped.contains("uuid="));
        assert!(zipped.contains("authkey="));
    }

    /// UUID length is not fixed (firmware accepts 16 or 20; real devices may
    /// return other lengths). Redaction must be by prefix, not by assuming a
    /// specific UUID length.
    #[test]
    fn write_logs_zip_mask_redacts_uuid_regardless_of_length() {
        for uuid_val in ["abcdef123456", "abcdef1234567890", "abcdef1234567890abcd"] {
            let content = format!("[batch-auth] allocated  uuid={uuid_val}\n");
            let zipped = write_and_read_zipped_log(&content, true);
            assert!(
                !zipped.contains(uuid_val),
                "uuid `{uuid_val}` leaked into masked export"
            );
        }
    }

    /// Non-credential log content is untouched by masking.
    #[test]
    fn write_logs_zip_mask_preserves_non_credential_lines() {
        let content =
            "[info] tyutool v3.2.8 starting\n[serial] port=COM3 opened\n[batch-auth] done  port=COM3 mac=AA:BB\n";
        let zipped = write_and_read_zipped_log(content, true);
        assert!(zipped.contains("tyutool v3.2.8 starting"));
        assert!(zipped.contains("port=COM3 opened"));
        assert!(zipped.contains("mac=AA:BB"));
    }

    #[test]
    fn gather_and_write_logs_zip_collects_logs_and_report() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tyutool.log"), b"hello").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"skip").unwrap();
        let dest = dir.path().join("out.zip");

        gather_and_write_logs_zip(
            dir.path(),
            "tyutool",
            "3.0.11",
            "AppImage",
            "sid",
            &dest,
            false,
        )
        .unwrap();

        let f = std::fs::File::open(&dest).unwrap();
        let mut zip = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"report-info.txt".to_string()));
        assert!(names.contains(&"tyutool.log".to_string()));
        assert!(!names.contains(&"notes.txt".to_string()));
    }
}
