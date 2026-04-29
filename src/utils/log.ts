/**
 * Convenience wrappers around `@tauri-apps/plugin-log`.
 *
 * Every call guards on `isTauriRuntime()` so callers never need to check.
 * In browser-preview mode the functions silently no-op.
 */
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';

type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error';

/** Write a single line to the Rust-side log file. */
async function rustLog(level: LogLevel, msg: string): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const mod = await import('@tauri-apps/plugin-log');
    await mod[level](msg);
  } catch {
    /* plugin not available — swallow */
  }
}

/**
 * Structured logger that writes to the Rust-side log file via `tauri-plugin-log`.
 *
 * Usage:
 * ```ts
 * import { rLog } from '@/utils/log';
 * rLog.info('[App] tyutool started');
 * rLog.debug('[Settings] Theme changed to dark');
 * ```
 */
export const rLog = {
  trace: (msg: string): void => void rustLog('trace', msg),
  debug: (msg: string): void => void rustLog('debug', msg),
  info: (msg: string): void => void rustLog('info', msg),
  warn: (msg: string): void => void rustLog('warn', msg),
  error: (msg: string): void => void rustLog('error', msg),
};
