//! Log files: retention, redaction, export, and opening them in an editor.
//!
//! Two guarantees documented in AGENTS.md live here and are covered by the
//! tests at the bottom of this file:
//!
//! * `batch-auth-*.trace` files hold plaintext credential interaction data and
//!   must never reach an export or archive zip. `collect_log_files`,
//!   `list_log_files_impl`, `pick_active_log` and `validate_log_filename` all
//!   exclude them by extension and prefix.
//! * The export path redacts credentials (`write_logs_zip` `mask = true`); the
//!   archive path keeps plaintext because it is the operator's local bundle.
//!
//! `DialogPathRegistry` gates the renderer-reachable arbitrary-path write
//! primitives; see its doc comment.

use std::sync::Mutex as StdMutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, State};

use crate::{detect_install_type, SESSION_ID};

/// Registry of filesystem paths the renderer may write to.
///
/// `write_text_file` / `append_text_file` are arbitrary-path write primitives
/// reachable from the renderer. Without a gate, a compromised renderer / XSS
/// could overwrite arbitrary files (`~/.bashrc`, startup scripts, ...). The
/// legitimate callers all write to paths the user picked via a Tauri dialog
/// (serial-debug auto-save dir, log-export save-as). Those callers register the
/// dialog-chosen path here; the write commands then refuse any path that is not
/// the registered path itself or a descendant of a registered directory.
///
/// Entries are TTL-bounded (a write is allowed within `TTL` of registration) so
/// a leaked/old entry cannot be reused later. The map is capped to bound memory.
pub(crate) struct DialogPathRegistry {
    entries: StdMutex<Vec<(std::path::PathBuf, std::time::Instant)>>,
}

/// How long a registered path stays authorized for writes.
const DIALOG_PATH_TTL: Duration = Duration::from_secs(600); // 10 min

/// Maximum number of registered paths kept at once (LRU eviction on insert).
const DIALOG_PATH_MAX_ENTRIES: usize = 32;

impl DialogPathRegistry {
    pub(crate) fn new() -> Self {
        Self {
            entries: StdMutex::new(Vec::new()),
        }
    }

    /// Register a dialog-chosen path (file or directory). Refreshes the TTL if
    /// already present; evicts the oldest entry when at capacity.
    fn register(&self, path: &std::path::Path) {
        let now = std::time::Instant::now();
        let mut entries = self.entries.lock().expect("DialogPathRegistry poisoned");
        // Refresh TTL if already present.
        if let Some(slot) = entries.iter_mut().find(|(p, _)| p == path) {
            slot.1 = now;
            return;
        }
        // Evict oldest (and any expired) when at capacity.
        if entries.len() >= DIALOG_PATH_MAX_ENTRIES {
            Self::prune_locked(&mut entries, now);
            while entries.len() >= DIALOG_PATH_MAX_ENTRIES {
                // Remove the single oldest.
                if let Some(idx) = Self::oldest_index(&entries) {
                    entries.remove(idx);
                } else {
                    break;
                }
            }
        }
        entries.push((path.to_path_buf(), now));
    }

    /// Returns true if `path` may be written: it equals a registered path, or it
    /// lives beneath a registered directory, within the TTL. Expired entries are
    /// pruned as a side effect.
    fn is_authorized(&self, path: &std::path::Path) -> bool {
        let now = std::time::Instant::now();
        let mut entries = match self.entries.lock() {
            Ok(e) => e,
            Err(p) => p.into_inner(),
        };
        Self::prune_locked(&mut entries, now);
        entries.iter().any(|(registered, ts)| {
            if now.duration_since(*ts) > DIALOG_PATH_TTL {
                return false;
            }
            // Authorized if the write target is the registered path itself, or a
            // descendant of a registered directory.
            path == registered || path.starts_with(registered)
        })
    }

    fn prune_locked(
        entries: &mut Vec<(std::path::PathBuf, std::time::Instant)>,
        now: std::time::Instant,
    ) {
        entries.retain(|(_, ts)| now.duration_since(*ts) <= DIALOG_PATH_TTL);
    }

    fn oldest_index(entries: &[(std::path::PathBuf, std::time::Instant)]) -> Option<usize> {
        entries
            .iter()
            .enumerate()
            .min_by_key(|(_, (_, ts))| *ts)
            .map(|(i, _)| i)
    }
}

#[tauri::command]
pub(crate) fn register_dialog_path(
    path: String,
    registry: State<'_, DialogPathRegistry>,
) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    registry.register(p);
    log::debug!("[DialogPath] registered: {}", p.display());
    Ok(())
}

#[tauri::command]
pub(crate) fn write_text_file(
    path: String,
    content: String,
    registry: State<'_, DialogPathRegistry>,
) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !registry.is_authorized(p) {
        log::warn!(
            "[DialogPath] write_text_file rejected unauthorized path: {}",
            p.display()
        );
        return Err("path is not authorized for writing".into());
    }
    std::fs::write(p, content.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn append_text_file(
    path: String,
    content: String,
    registry: State<'_, DialogPathRegistry>,
) -> Result<(), String> {
    use std::io::Write;
    let p = std::path::Path::new(&path);
    if !registry.is_authorized(p) {
        log::warn!(
            "[DialogPath] append_text_file rejected unauthorized path: {}",
            p.display()
        );
        return Err("path is not authorized for writing".into());
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(p)
        .map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())
}

const MAX_LOG_FILES: usize = 100;
const MAX_LOG_BYTES_TOTAL: u64 = 100 * 1024 * 1024; // 100 MB
/// Bounded growth for `.trace` files (plaintext batch-auth interaction data).
/// Independent from `.log` limits — `.trace` is never collected into any
/// export/archive zip (it has no `tyutool-` prefix and a non-`.log` extension).
const MAX_TRACE_FILES: usize = 20;

/// Delete the oldest per-session log files until the collection is within both
/// the file-count and total-size limits. Only manages files whose stem starts
/// with "tyutool-" (per-session naming); legacy "tyutool.log" is left untouched.
/// Always retains at least one file.
pub(crate) fn prune_log_files(log_dir: &std::path::Path) {
    let mut files: Vec<(std::path::PathBuf, u64)> = std::fs::read_dir(log_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|x| x == "log").unwrap_or(false)
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with("tyutool-"))
                    .unwrap_or(false)
        })
        .map(|p| {
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            (p, size)
        })
        .collect();

    // Timestamped filenames are lexicographically chronological.
    files.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));

    let mut count = files.len();
    let mut total: u64 = files.iter().map(|(_, s)| s).sum();

    for (path, size) in &files {
        if count <= 1 || (count <= MAX_LOG_FILES && total <= MAX_LOG_BYTES_TOTAL) {
            break;
        }
        let _ = std::fs::remove_file(path);
        count -= 1;
        total = total.saturating_sub(*size);
    }
}

/// Plaintext writer for batch-auth device-interaction data (auth-read raw lines,
/// auth-write responses, verify comparison values). Lives in its own
/// `batch-auth-<ts>.trace` file — deliberately NOT a `.log` file and NOT
/// `tyutool-`-prefixed, so `collect_log_files` / `prune_log_files` /
/// `list_log_files_impl` / `pick_active_log` all ignore it. The export-for-report
/// zip therefore can never contain it; only the operator's local machine keeps it.
pub(crate) struct BatchAuthTraceWriter {
    file: std::fs::File,
}

impl BatchAuthTraceWriter {
    /// Create `<log_dir>/batch-auth-<ts>.trace` (append mode). `ts` should be a
    /// sortable timestamp stem (matching the `tyutool-<ts>.log` convention).
    pub(crate) fn open(log_dir: &std::path::Path, ts: &str) -> std::io::Result<Self> {
        let path = log_dir.join(format!("batch-auth-{ts}.trace"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self { file })
    }

    /// Append one line (trailing newline added). Errors are swallowed — trace
    /// logging is best-effort and must never break a batch run.
    pub(crate) fn writeln(&mut self, line: &str) {
        use std::io::Write;
        let _ = writeln!(self.file, "{line}");
    }
}

/// Delete the oldest `batch-auth-*.trace` files until at most `MAX_TRACE_FILES`
/// remain. Independent from `prune_log_files` (different prefix/extension).
pub(crate) fn prune_trace_files(log_dir: &std::path::Path) {
    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(log_dir) {
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
fn pick_active_log(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "log").unwrap_or(false))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Read the last `max_bytes` bytes of `path` as UTF-8 (lossy).
fn tail_bytes(path: &std::path::Path, max_bytes: u64) -> std::io::Result<String> {
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
fn read_log_tail_impl(dir: &std::path::Path, max_bytes: u64) -> std::io::Result<String> {
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

fn read_named_log_impl(
    dir: &std::path::Path,
    filename: &str,
    max_bytes: u64,
) -> std::io::Result<String> {
    validate_log_filename(filename)?;
    tail_bytes(&dir.join(filename), max_bytes)
}

fn resolve_log_open_path(
    dir: &std::path::Path,
    filename: &str,
) -> std::io::Result<std::path::PathBuf> {
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

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogFileOpener {
    id: &'static str,
    label: &'static str,
}

enum LogFileOpenerLaunch {
    SystemDefault,
    Direct(std::path::PathBuf),
    #[cfg(target_os = "macos")]
    MacApplication(std::path::PathBuf),
}

struct DetectedLogFileOpener {
    launch: LogFileOpenerLaunch,
}

fn supported_log_editor_catalog() -> Vec<LogFileOpener> {
    vec![
        LogFileOpener {
            id: "default",
            label: "System Default",
        },
        LogFileOpener {
            id: "vscode",
            label: "VS Code",
        },
        LogFileOpener {
            id: "sublime_text",
            label: "Sublime Text",
        },
        LogFileOpener {
            id: "notepad_minus_minus",
            label: "Notepad--",
        },
        LogFileOpener {
            id: "windows_notepad",
            label: "Windows Notepad",
        },
        LogFileOpener {
            id: "notepad_plus_plus",
            label: "Notepad++",
        },
    ]
}

fn existing_path(path: impl Into<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    let path = path.into();
    path.is_file().then_some(path)
}

#[cfg(target_os = "macos")]
fn existing_app_bundle(path: impl Into<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    let path = path.into();
    path.is_dir().then_some(path)
}

fn first_existing_path(
    paths: impl IntoIterator<Item = std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    paths.into_iter().find_map(existing_path)
}

#[cfg(target_os = "macos")]
fn first_existing_app_bundle(
    paths: impl IntoIterator<Item = std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    paths.into_iter().find_map(existing_app_bundle)
}

fn find_command_in_path(names: &[&str]) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let path_exts: Vec<String> = if cfg!(windows) {
        std::env::var_os("PATHEXT")
            .map(|exts| {
                exts.to_string_lossy()
                    .split(';')
                    .filter(|ext| !ext.is_empty())
                    .map(|ext| ext.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_else(|| vec![".exe".into(), ".cmd".into(), ".bat".into()])
    } else {
        vec![String::new()]
    };

    for dir in std::env::split_paths(&path_var) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
            if cfg!(windows) && std::path::Path::new(name).extension().is_none() {
                for ext in &path_exts {
                    let candidate = dir.join(format!("{name}{ext}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

fn detect_vscode() -> Option<DetectedLogFileOpener> {
    #[cfg(target_os = "windows")]
    {
        let mut candidates = Vec::new();
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(
                std::path::PathBuf::from(&local_app_data)
                    .join("Programs")
                    .join("Microsoft VS Code")
                    .join("Code.exe"),
            );
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.push(
                std::path::PathBuf::from(&program_files)
                    .join("Microsoft VS Code")
                    .join("Code.exe"),
            );
        }
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            candidates.push(
                std::path::PathBuf::from(&program_files_x86)
                    .join("Microsoft VS Code")
                    .join("Code.exe"),
            );
        }
        let program =
            first_existing_path(candidates).or_else(|| find_command_in_path(&["Code.exe"]))?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![std::path::PathBuf::from(
            "/Applications/Visual Studio Code.app",
        )];
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(
                std::path::PathBuf::from(home)
                    .join("Applications")
                    .join("Visual Studio Code.app"),
            );
        }
        if let Some(app_path) = first_existing_app_bundle(candidates) {
            return Some(DetectedLogFileOpener {
                launch: LogFileOpenerLaunch::MacApplication(app_path),
            });
        }
        let program = find_command_in_path(&["code"])?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let program = first_existing_path([
            std::path::PathBuf::from("/usr/bin/code"),
            std::path::PathBuf::from("/usr/local/bin/code"),
            std::path::PathBuf::from("/snap/bin/code"),
        ])
        .or_else(|| find_command_in_path(&["code"]))?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[allow(unreachable_code)]
    None
}

fn detect_sublime_text() -> Option<DetectedLogFileOpener> {
    #[cfg(target_os = "windows")]
    {
        let mut candidates = Vec::new();
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.push(
                std::path::PathBuf::from(&program_files)
                    .join("Sublime Text")
                    .join("sublime_text.exe"),
            );
            candidates.push(
                std::path::PathBuf::from(&program_files)
                    .join("Sublime Text 3")
                    .join("sublime_text.exe"),
            );
        }
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            candidates.push(
                std::path::PathBuf::from(&program_files_x86)
                    .join("Sublime Text")
                    .join("sublime_text.exe"),
            );
            candidates.push(
                std::path::PathBuf::from(&program_files_x86)
                    .join("Sublime Text 3")
                    .join("sublime_text.exe"),
            );
        }
        let program = first_existing_path(candidates)
            .or_else(|| find_command_in_path(&["subl.exe", "sublime_text.exe"]))?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![std::path::PathBuf::from("/Applications/Sublime Text.app")];
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(
                std::path::PathBuf::from(home)
                    .join("Applications")
                    .join("Sublime Text.app"),
            );
        }
        if let Some(app_path) = first_existing_app_bundle(candidates) {
            return Some(DetectedLogFileOpener {
                launch: LogFileOpenerLaunch::MacApplication(app_path),
            });
        }
        let program = find_command_in_path(&["subl"])?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let program = first_existing_path([
            std::path::PathBuf::from("/opt/sublime_text/sublime_text"),
            std::path::PathBuf::from("/usr/bin/subl"),
            std::path::PathBuf::from("/usr/local/bin/subl"),
            std::path::PathBuf::from("/usr/bin/sublime_text"),
        ])
        .or_else(|| find_command_in_path(&["subl", "sublime_text"]))?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[allow(unreachable_code)]
    None
}

fn detect_notepad_minus_minus() -> Option<DetectedLogFileOpener> {
    #[cfg(target_os = "windows")]
    {
        let mut candidates = Vec::new();
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.push(
                std::path::PathBuf::from(&program_files)
                    .join("Notepad--")
                    .join("Notepad--.exe"),
            );
            candidates.push(
                std::path::PathBuf::from(&program_files)
                    .join("Notepad--")
                    .join("ndd.exe"),
            );
        }
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            candidates.push(
                std::path::PathBuf::from(&program_files_x86)
                    .join("Notepad--")
                    .join("Notepad--.exe"),
            );
            candidates.push(
                std::path::PathBuf::from(&program_files_x86)
                    .join("Notepad--")
                    .join("ndd.exe"),
            );
        }
        let program = first_existing_path(candidates)
            .or_else(|| find_command_in_path(&["ndd.exe", "Notepad--.exe"]))?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![std::path::PathBuf::from("/Applications/Notepad--.app")];
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(
                std::path::PathBuf::from(home)
                    .join("Applications")
                    .join("Notepad--.app"),
            );
        }
        if let Some(app_path) = first_existing_app_bundle(candidates) {
            return Some(DetectedLogFileOpener {
                launch: LogFileOpenerLaunch::MacApplication(app_path),
            });
        }
        let program = find_command_in_path(&["ndd"])?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let program = first_existing_path([
            std::path::PathBuf::from("/usr/bin/ndd"),
            std::path::PathBuf::from("/usr/local/bin/ndd"),
            std::path::PathBuf::from("/opt/ndd/ndd"),
        ])
        .or_else(|| find_command_in_path(&["ndd"]))?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[allow(unreachable_code)]
    None
}

fn detect_windows_notepad() -> Option<DetectedLogFileOpener> {
    #[cfg(target_os = "windows")]
    {
        let system_root = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
        let program = existing_path(system_root.join("System32").join("notepad.exe"))?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[allow(unreachable_code)]
    None
}

fn detect_notepad_plus_plus() -> Option<DetectedLogFileOpener> {
    #[cfg(target_os = "windows")]
    {
        let mut candidates = Vec::new();
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.push(
                std::path::PathBuf::from(&program_files)
                    .join("Notepad++")
                    .join("notepad++.exe"),
            );
        }
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            candidates.push(
                std::path::PathBuf::from(&program_files_x86)
                    .join("Notepad++")
                    .join("notepad++.exe"),
            );
        }
        let program =
            first_existing_path(candidates).or_else(|| find_command_in_path(&["notepad++.exe"]))?;
        return Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::Direct(program),
        });
    }
    #[allow(unreachable_code)]
    None
}

fn detect_log_file_opener(editor_id: &str) -> Option<DetectedLogFileOpener> {
    match editor_id {
        "default" => Some(DetectedLogFileOpener {
            launch: LogFileOpenerLaunch::SystemDefault,
        }),
        "vscode" => detect_vscode(),
        "sublime_text" => detect_sublime_text(),
        "notepad_minus_minus" => detect_notepad_minus_minus(),
        "windows_notepad" => detect_windows_notepad(),
        "notepad_plus_plus" => detect_notepad_plus_plus(),
        _ => None,
    }
}

fn open_log_path_with_opener(
    path: &std::path::Path,
    opener: DetectedLogFileOpener,
) -> Result<(), String> {
    match opener.launch {
        LogFileOpenerLaunch::SystemDefault => {
            tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
        }
        LogFileOpenerLaunch::Direct(program) => std::process::Command::new(&program)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string()),
        #[cfg(target_os = "macos")]
        LogFileOpenerLaunch::MacApplication(app_path) => std::process::Command::new("open")
            .arg("-a")
            .arg(app_path)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string()),
    }
}

#[tauri::command]
pub(crate) fn read_log_tail(
    app: AppHandle,
    max_bytes: usize,
    filename: Option<String>,
) -> Result<String, String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    match filename {
        None => read_log_tail_impl(&dir, max_bytes as u64).map_err(|e| e.to_string()),
        Some(name) => read_named_log_impl(&dir, &name, max_bytes as u64).map_err(|e| e.to_string()),
    }
}

fn collect_log_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
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
pub(crate) struct LogFileInfo {
    name: String,
    size_bytes: u64,
    modified_ms: i64,
}

fn list_log_files_impl(dir: &std::path::Path) -> Vec<LogFileInfo> {
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
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
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

#[tauri::command]
pub(crate) fn list_log_files(app: AppHandle) -> Result<Vec<LogFileInfo>, String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    Ok(list_log_files_impl(&dir))
}

#[tauri::command]
pub(crate) fn list_log_file_openers() -> Vec<LogFileOpener> {
    supported_log_editor_catalog()
        .into_iter()
        .filter(|opener| opener.id == "default" || detect_log_file_opener(opener.id).is_some())
        .collect()
}

#[tauri::command]
pub(crate) fn open_log_file_in_editor(
    app: AppHandle,
    filename: String,
    editor_id: String,
) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    let path = resolve_log_open_path(&dir, &filename).map_err(|e| e.to_string())?;
    let opener = detect_log_file_opener(&editor_id)
        .ok_or_else(|| format!("unsupported or unavailable editor: {editor_id}"))?;
    open_log_path_with_opener(&path, opener)
}

fn build_report_info(name: &str, version: &str, install: &str, session_id: &str) -> String {
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
const REDACT_PREFIXES: &[&str] = &["uuid=", "authkey=", "existing_uuid=", "otp_uuid="];

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
fn redact_log_content(content: &str) -> String {
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

fn write_logs_zip(
    log_files: &[std::path::PathBuf],
    report_info: &str,
    dest: &std::path::Path,
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
pub(crate) fn gather_and_write_logs_zip(
    dir: &std::path::Path,
    name: &str,
    version: &str,
    install: &str,
    session_id: &str,
    dest: &std::path::Path,
    mask: bool,
) -> Result<(), String> {
    let files = collect_log_files(dir);
    let info = build_report_info(name, version, install, session_id);
    write_logs_zip(&files, &info, dest, mask)
}

#[tauri::command]
pub(crate) fn export_logs_zip(app: AppHandle, dest_path: String) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    gather_and_write_logs_zip(
        &dir,
        &app.package_info().name,
        &app.package_info().version.to_string(),
        &detect_install_type(),
        SESSION_ID.get().map(String::as_str).unwrap_or(""),
        std::path::Path::new(&dest_path),
        // Export-for-report: redact credential values, the zip may be shared
        // via a GitHub issue. The archive path keeps plaintext (mask = false).
        true,
    )
}

#[cfg(test)]
mod log_tools_tests {
    use super::*;
    use std::io::Write;

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
    fn batch_auth_trace_writer_creates_dot_trace_file_with_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = BatchAuthTraceWriter::open(dir.path(), "20260804-120000").unwrap();
        w.writeln("[verify] wrote uuid=real-uuid authkey=real-secret-key");
        drop(w);
        let path = dir.path().join("batch-auth-20260804-120000.trace");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("uuid=real-uuid"));
        assert!(content.contains("authkey=real-secret-key"));
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
    fn supported_log_editor_catalog_starts_with_system_default() {
        let editors = supported_log_editor_catalog();

        assert!(!editors.is_empty());
        assert_eq!(editors[0].id, "default");
    }
}

#[cfg(test)]
mod dialog_path_registry_tests {
    use super::DialogPathRegistry;
    use std::path::Path;

    #[test]
    fn rejects_unregistered_path() {
        let reg = DialogPathRegistry::new();
        assert!(!reg.is_authorized(Path::new("/tmp/evil.txt")));
    }

    #[test]
    fn accepts_exact_registered_file() {
        let reg = DialogPathRegistry::new();
        reg.register(Path::new("/tmp/export.txt"));
        assert!(reg.is_authorized(Path::new("/tmp/export.txt")));
        // sibling is not authorized
        assert!(!reg.is_authorized(Path::new("/tmp/other.txt")));
    }

    #[test]
    fn accepts_descendant_of_registered_directory() {
        // serial-debug auto-save registers a directory and writes files beneath it.
        let reg = DialogPathRegistry::new();
        reg.register(Path::new("/tmp/serial-debug"));
        assert!(reg.is_authorized(Path::new("/tmp/serial-debug/ttyUSB0/log.txt")));
        assert!(reg.is_authorized(Path::new("/tmp/serial-debug/nested/deep/log.txt")));
        // outside the directory is rejected
        assert!(!reg.is_authorized(Path::new("/tmp/other.txt")));
    }

    #[test]
    fn refreshes_ttl_on_re_register() {
        let reg = DialogPathRegistry::new();
        reg.register(Path::new("/tmp/export.txt"));
        // immediately re-register (simulating the auto-save dir staying active)
        reg.register(Path::new("/tmp/export.txt"));
        assert!(reg.is_authorized(Path::new("/tmp/export.txt")));
    }

    #[test]
    fn evicts_oldest_when_at_capacity() {
        let reg = DialogPathRegistry::new();
        // Fill to capacity with distinct paths.
        for i in 0..super::DIALOG_PATH_MAX_ENTRIES {
            reg.register(Path::new(&format!("/tmp/file-{i}.txt")));
        }
        assert!(reg.is_authorized(Path::new("/tmp/file-0.txt")));
        // Adding one more evicts the oldest (file-0 was registered first).
        reg.register(Path::new("/tmp/extra.txt"));
        assert!(reg.is_authorized(Path::new("/tmp/extra.txt")));
        assert!(
            !reg.is_authorized(Path::new("/tmp/file-0.txt")),
            "oldest entry should have been evicted"
        );
        // a later-inserted one should still be present
        assert!(reg.is_authorized(Path::new(&format!(
            "/tmp/file-{}.txt",
            super::DIALOG_PATH_MAX_ENTRIES - 1
        ))));
    }
}
