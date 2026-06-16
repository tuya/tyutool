export type RuntimeType = 'tauri' | 'vscode' | 'web';

/**
 * Tauri 2 injects `window.__TAURI_INTERNALS__` into the WebView at runtime.
 * `import.meta.env.TAURI_ENV_PLATFORM` is only set during `tauri dev` (Vite dev
 * server) but NOT during production builds — use runtime detection instead.
 */
export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export function getRuntime(): RuntimeType {
  if (typeof window === 'undefined') return 'web';
  if (isTauriRuntime()) return 'tauri';
  if ((window as { __TUYAOPEN_IDE_CONFIG?: { runtime?: string } }).__TUYAOPEN_IDE_CONFIG?.runtime === 'vscode') {
    return 'vscode';
  }
  return 'web';
}
