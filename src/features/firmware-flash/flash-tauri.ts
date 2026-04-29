/**
 * Types aligned with `tyutool_core::FlashJob` / `FlashProgress` (camelCase JSON).
 */

export type FlashJobMode = 'flash' | 'erase' | 'read' | 'authorize';

export interface FlashSegmentPayload {
  firmwarePath: string;
  startAddr: string;
  endAddr: string;
}

export interface FlashJobPayload {
  mode: FlashJobMode;
  chipId: string;
  port: string;
  baudRate: number;
  segments?: FlashSegmentPayload[] | null;
  flashStartHex?: string | null;
  flashEndHex?: string | null;
  eraseStartHex?: string | null;
  eraseEndHex?: string | null;
  readStartHex?: string | null;
  readEndHex?: string | null;
  readFilePath?: string | null;
  firmwarePath?: string | null;
  authorizeUuid?: string | null;
  authorizeKey?: string | null;
}

export type FlashProgressPayload =
  | { kind: 'percent'; value: number }
  | { kind: 'log_line'; line: string }
  | { kind: 'log_key'; key: string; params?: Record<string, string> }
  | { kind: 'phase'; name: string }
  | { kind: 'done'; ok: boolean; message?: string | null };

export function isTauriRuntime(): boolean {
  // Tauri 2 injects `window.__TAURI_INTERNALS__` into the WebView at runtime.
  // `import.meta.env.TAURI_ENV_PLATFORM` is only set during `tauri dev` (Vite dev
  // server) but NOT during production builds, so it gets compiled away by Vite and
  // the Tauri code branch is tree-shaken out.  Use runtime detection instead.
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
