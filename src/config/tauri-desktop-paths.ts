/**
 * Must stay in sync with `identifier` in `src-tauri/tauri.conf.json`.
 * Matches Tauri `PathResolver::app_log_dir` (see tauri `path/desktop.rs`).
 */
export const TAURI_APP_IDENTIFIER = 'com.tyutool.desktop';

/**
 * Human-readable hint for where desktop tyutool log files live, for UI copy when
 * `appLogDir()` is unavailable (e.g. browser dev). Uses `navigator.userAgent`; if
 * OS cannot be guessed, returns the Linux default.
 */
export function desktopAppLogDirHint(): string {
  if (typeof navigator === 'undefined') {
    return `~/.local/share/${TAURI_APP_IDENTIFIER}/logs`;
  }
  const ua = navigator.userAgent;
  if (/Windows/i.test(ua)) {
    return `%LOCALAPPDATA%\\${TAURI_APP_IDENTIFIER}\\logs`;
  }
  if (/Mac OS X|Macintosh/i.test(ua)) {
    return `~/Library/Logs/${TAURI_APP_IDENTIFIER}`;
  }
  return `~/.local/share/${TAURI_APP_IDENTIFIER}/logs`;
}
