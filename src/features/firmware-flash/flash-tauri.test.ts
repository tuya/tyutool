import { afterEach, describe, expect, it } from 'vitest';
import { isTauriRuntime } from './flash-tauri';

describe('isTauriRuntime', () => {
  const originalWindow = globalThis.window;

  afterEach(() => {
    // Restore window to its original state
    if (originalWindow === undefined) {
      // @ts-expect-error -- test cleanup
      delete globalThis.window;
    } else {
      globalThis.window = originalWindow;
    }
  });

  it('returns false when window is undefined', () => {
    // @ts-expect-error -- simulate no window (SSR / Node)
    delete globalThis.window;
    expect(isTauriRuntime()).toBe(false);
  });

  it('returns false when __TAURI_INTERNALS__ is absent', () => {
    // In Node vitest, window may not exist; create a minimal stub
    globalThis.window = {} as typeof window;
    expect(isTauriRuntime()).toBe(false);
  });

  it('returns true when __TAURI_INTERNALS__ is present', () => {
    globalThis.window = { __TAURI_INTERNALS__: {} } as unknown as typeof window;
    expect(isTauriRuntime()).toBe(true);
  });
});
