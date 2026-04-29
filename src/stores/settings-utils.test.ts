// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  applyThemeToDom,
  LEGACY_LOCALE_KEY,
  LEGACY_THEME_KEY,
  loadStoredLocale,
  loadStoredLogEnabled,
  loadStoredLogLevel,
  loadStoredTheme,
  LOCALE_KEY,
  LOG_ENABLED_KEY,
  LOG_LEVEL_KEY,
  THEME_KEY,
} from './settings-utils';

beforeEach(() => {
  localStorage.clear();
});

describe('loadStoredTheme', () => {
  it('returns "system" when no stored value', () => {
    expect(loadStoredTheme()).toBe('system');
  });

  it('returns stored valid theme', () => {
    localStorage.setItem(THEME_KEY, 'dark');
    expect(loadStoredTheme()).toBe('dark');

    localStorage.setItem(THEME_KEY, 'light');
    expect(loadStoredTheme()).toBe('light');
  });

  it('returns "system" for invalid stored value', () => {
    localStorage.setItem(THEME_KEY, 'neon');
    expect(loadStoredTheme()).toBe('system');
  });

  it('migrates legacy key and removes it', () => {
    localStorage.setItem(LEGACY_THEME_KEY, 'dark');
    expect(loadStoredTheme()).toBe('dark');
    // Legacy key should be removed, new key should be set
    expect(localStorage.getItem(THEME_KEY)).toBe('dark');
    expect(localStorage.getItem(LEGACY_THEME_KEY)).toBeNull();
  });

  it('ignores invalid legacy key', () => {
    localStorage.setItem(LEGACY_THEME_KEY, 'invalid');
    expect(loadStoredTheme()).toBe('system');
  });
});

describe('loadStoredLocale', () => {
  it('returns "auto" when no stored value', () => {
    expect(loadStoredLocale()).toBe('auto');
  });

  it('returns stored valid locale', () => {
    localStorage.setItem(LOCALE_KEY, 'zh-CN');
    expect(loadStoredLocale()).toBe('zh-CN');

    localStorage.setItem(LOCALE_KEY, 'en');
    expect(loadStoredLocale()).toBe('en');

    localStorage.setItem(LOCALE_KEY, 'auto');
    expect(loadStoredLocale()).toBe('auto');
  });

  it('returns "auto" for invalid stored value', () => {
    localStorage.setItem(LOCALE_KEY, 'fr');
    expect(loadStoredLocale()).toBe('auto');
  });

  it('migrates legacy key and removes it', () => {
    localStorage.setItem(LEGACY_LOCALE_KEY, 'en');
    expect(loadStoredLocale()).toBe('en');
    expect(localStorage.getItem(LOCALE_KEY)).toBe('en');
    expect(localStorage.getItem(LEGACY_LOCALE_KEY)).toBeNull();
  });
});

describe('loadStoredLogEnabled', () => {
  it('returns true when no stored value (first use default)', () => {
    expect(loadStoredLogEnabled()).toBe(true);
  });

  it('returns true when stored "true"', () => {
    localStorage.setItem(LOG_ENABLED_KEY, 'true');
    expect(loadStoredLogEnabled()).toBe(true);
  });

  it('returns false when stored "false"', () => {
    localStorage.setItem(LOG_ENABLED_KEY, 'false');
    expect(loadStoredLogEnabled()).toBe(false);
  });

  it('returns false for any non-"true" string', () => {
    localStorage.setItem(LOG_ENABLED_KEY, 'yes');
    expect(loadStoredLogEnabled()).toBe(false);
  });
});

describe('loadStoredLogLevel', () => {
  it('returns "info" when no stored value', () => {
    expect(loadStoredLogLevel()).toBe('info');
  });

  it('returns stored valid level', () => {
    for (const level of ['error', 'warn', 'info', 'debug', 'trace'] as const) {
      localStorage.setItem(LOG_LEVEL_KEY, level);
      expect(loadStoredLogLevel()).toBe(level);
    }
  });

  it('returns "info" for invalid level', () => {
    localStorage.setItem(LOG_LEVEL_KEY, 'verbose');
    expect(loadStoredLogLevel()).toBe('info');
  });
});

describe('applyThemeToDom', () => {
  afterEach(() => {
    document.documentElement.classList.remove('dark');
    vi.restoreAllMocks();
  });

  it('adds dark class for dark theme', () => {
    applyThemeToDom('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('removes dark class for light theme', () => {
    document.documentElement.classList.add('dark');
    applyThemeToDom('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });

  it('uses matchMedia for system theme (dark preference)', () => {
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: query === '(prefers-color-scheme: dark)',
        media: query,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
    applyThemeToDom('system');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('uses matchMedia for system theme (light preference)', () => {
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false,
        media: query,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
    applyThemeToDom('system');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });
});
