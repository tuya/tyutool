/**
 * Pure helpers for settings persistence (localStorage / DOM).
 * Extracted from the Pinia store for independent testability.
 */

import type { LocalePreference, LogLevelId, ThemePreference } from './settings';

const THEME_KEY = 'tyutool-theme';
const LOCALE_KEY = 'tyutool-locale';
const LEGACY_THEME_KEY = 'tyutools-theme';
const LEGACY_LOCALE_KEY = 'tyutools-locale';
const LOG_ENABLED_KEY = 'tyutool-log-enabled';
const LOG_LEVEL_KEY = 'tyutool-log-level';

export { THEME_KEY, LOCALE_KEY, LEGACY_THEME_KEY, LEGACY_LOCALE_KEY, LOG_ENABLED_KEY, LOG_LEVEL_KEY };

export function loadStoredTheme(): ThemePreference {
  let s = localStorage.getItem(THEME_KEY) as ThemePreference | null;
  if (!s) {
    const legacy = localStorage.getItem(LEGACY_THEME_KEY) as ThemePreference | null;
    if (legacy === 'light' || legacy === 'dark' || legacy === 'system') {
      localStorage.setItem(THEME_KEY, legacy);
      localStorage.removeItem(LEGACY_THEME_KEY);
      s = legacy;
    }
  }
  if (s === 'light' || s === 'dark' || s === 'system') {
    return s;
  }
  return 'system';
}

export function loadStoredLocale(): LocalePreference {
  let s = localStorage.getItem(LOCALE_KEY);
  if (!s) {
    const legacy = localStorage.getItem(LEGACY_LOCALE_KEY);
    if (legacy === 'zh-CN' || legacy === 'en') {
      localStorage.setItem(LOCALE_KEY, legacy);
      localStorage.removeItem(LEGACY_LOCALE_KEY);
      s = legacy;
    }
  }
  if (s === 'zh-CN' || s === 'en' || s === 'auto') {
    return s;
  }
  return 'auto';
}

export function loadStoredLogEnabled(): boolean {
  const val = localStorage.getItem(LOG_ENABLED_KEY);
  if (val === null) return true; // 首次使用默认开启
  return val === 'true';
}

export function loadStoredLogLevel(): LogLevelId {
  const val = localStorage.getItem(LOG_LEVEL_KEY);
  if (val && ['error', 'warn', 'info', 'debug', 'trace'].includes(val)) {
    return val as LogLevelId;
  }
  return 'info';
}

export function applyThemeToDom(pref: ThemePreference): void {
  const root = document.documentElement;
  let mode: 'light' | 'dark' = 'dark';
  if (pref === 'system') {
    mode = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  } else {
    mode = pref;
  }
  root.classList.toggle('dark', mode === 'dark');
}
