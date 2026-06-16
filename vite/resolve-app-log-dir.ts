/**
 * Node-only: resolve Tauri `app_log_dir` on the local machine (see tauri `path/desktop.rs`).
 * Used by `plugin-dev-open-app-log-dir.ts`; keep in sync with `src/config/app-identifier.ts`.
 */
import os from 'node:os';
import path from 'node:path';
import { TAURI_APP_IDENTIFIER } from '../src/config/app-identifier';

export function resolveAppLogDirAbsolute(): string {
  const home = os.homedir();
  if (process.platform === 'win32') {
    const localAppData =
      process.env.LOCALAPPDATA || path.join(home, 'AppData', 'Local');
    return path.join(localAppData, TAURI_APP_IDENTIFIER, 'logs');
  }
  if (process.platform === 'darwin') {
    return path.join(home, 'Library', 'Logs', TAURI_APP_IDENTIFIER);
  }
  const xdgDataHome =
    process.env.XDG_DATA_HOME || path.join(home, '.local', 'share');
  return path.join(xdgDataHome, TAURI_APP_IDENTIFIER, 'logs');
}
