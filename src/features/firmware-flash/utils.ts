/**
 * Pure utility helpers extracted from the flash store for testability.
 * No DOM, Vue, or Tauri dependencies — safe for Node-based unit tests.
 */

/** Format a duration in milliseconds to a human-readable string. */
export function formatDuration(ms: number): string {
  const totalSeconds = ms / 1000;
  if (totalSeconds < 60) {
    return `${totalSeconds.toFixed(1)}s`;
  }
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = Math.floor(totalSeconds % 60);
  if (totalSeconds < 3600) {
    return `${minutes}m ${seconds}s`;
  }
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return `${hours}h ${remainingMinutes}m ${seconds}s`;
}

/**
 * Append a timestamp suffix (e.g. `_20260410_143025`) before the file extension.
 * If there is no extension, the suffix is appended at the end.
 */
export function addTimestampSuffix(filePath: string, now = new Date()): string {
  const ts = [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, '0'),
    String(now.getDate()).padStart(2, '0'),
    '_',
    String(now.getHours()).padStart(2, '0'),
    String(now.getMinutes()).padStart(2, '0'),
    String(now.getSeconds()).padStart(2, '0'),
  ].join('');
  const lastDot = filePath.lastIndexOf('.');
  if (lastDot === -1) return `${filePath}_${ts}`;
  return `${filePath.substring(0, lastDot)}_${ts}${filePath.substring(lastDot)}`;
}
