import { ref, watch } from 'vue';
import { defineStore } from 'pinia';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { rLog } from '@/utils/log';
import {
  applyThemeToDom,
  loadStoredLocale,
  loadStoredLogEnabled,
  loadStoredLogLevel,
  loadStoredTheme,
  LOG_ENABLED_KEY,
  LOG_LEVEL_KEY,
  LOCALE_KEY,
  THEME_KEY,
} from './settings-utils';

export type ThemePreference = 'light' | 'dark' | 'system';
export type LocaleId = 'zh-CN' | 'en';
export type LocalePreference = LocaleId | 'auto';
export type LogLevelId = 'error' | 'warn' | 'info' | 'debug' | 'trace';

const STORE_FILE = 'settings.json';

/** Resolve a locale preference to a concrete locale id. */
export function resolveLocale(pref: LocalePreference): LocaleId {
  if (pref === 'auto') {
    const lang = navigator.language ?? '';
    return lang.startsWith('zh') ? 'zh-CN' : 'en';
  }
  return pref;
}

async function persistSetting(key: string, value: string): Promise<void> {
  if (isTauriRuntime()) {
    const { Store } = await import('@tauri-apps/plugin-store');
    const store = await Store.load(STORE_FILE);
    await store.set(key, value);
    await store.save();
  } else {
    localStorage.setItem(key, value);
  }
}

export const useSettingsStore = defineStore('settings', () => {
  const theme = ref<ThemePreference>(loadStoredTheme());
  const locale = ref<LocalePreference>(loadStoredLocale());
  const logEnabled = ref<boolean>(loadStoredLogEnabled());
  const logLevel = ref<LogLevelId>(loadStoredLogLevel());

  function setTheme(value: ThemePreference): void {
    theme.value = value;
  }

  function setLocale(value: LocalePreference): void {
    locale.value = value;
  }

  function setLogEnabled(value: boolean): void {
    logEnabled.value = value;
  }

  function setLogLevel(value: LogLevelId): void {
    logLevel.value = value;
  }

  async function applyLogLevel(): Promise<void> {
    if (!isTauriRuntime()) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const level = logEnabled.value ? logLevel.value : 'off';
      await invoke('set_log_level', { level });
    } catch (e) {
      console.warn('[tyutool] Failed to set log level:', e);
    }
  }

  async function loadFromTauriStore(): Promise<void> {
    const { Store } = await import('@tauri-apps/plugin-store');
    const store = await Store.load(STORE_FILE);
    const storedTheme = await store.get<ThemePreference>(THEME_KEY);
    const storedLocale = await store.get<LocalePreference>(LOCALE_KEY);
    if (storedTheme === 'light' || storedTheme === 'dark' || storedTheme === 'system') {
      theme.value = storedTheme;
    }
    if (storedLocale === 'zh-CN' || storedLocale === 'en' || storedLocale === 'auto') {
      locale.value = storedLocale;
    }
    const storedLogEnabled = await store.get<string>(LOG_ENABLED_KEY);
    if (storedLogEnabled !== null && storedLogEnabled !== undefined) {
      logEnabled.value = storedLogEnabled === 'true';
    }
    const storedLogLevel = await store.get<string>(LOG_LEVEL_KEY);
    if (storedLogLevel && ['error', 'warn', 'info', 'debug', 'trace'].includes(storedLogLevel)) {
      logLevel.value = storedLogLevel as LogLevelId;
    }
  }

  /** Resolves when all persisted settings (including async Tauri store) are loaded. */
  let _ready: Promise<void> = Promise.resolve();

  function init(): void {
    // Apply theme to DOM on startup
    applyThemeToDom(theme.value);

    // Load persisted settings from Tauri store (async, may update theme/locale after init)
    if (isTauriRuntime()) {
      _ready = loadFromTauriStore().then(() => {
        applyThemeToDom(theme.value);
      });
    }

    // Persist theme and re-apply on change
    watch(theme, v => {
      void persistSetting(THEME_KEY, v);
      applyThemeToDom(v);
      rLog.debug(`[Settings] Theme changed to: ${v}`);
    });

    // Persist locale on change
    watch(locale, v => {
      void persistSetting(LOCALE_KEY, v);
      const resolved = resolveLocale(v);
      document.documentElement.lang = resolved === 'zh-CN' ? 'zh-CN' : 'en';
      rLog.debug(`[Settings] Locale changed to: ${v} (resolved: ${resolved})`);
    });

    // Log settings — watch handles applyLogLevel on change;
    // initial apply happens once via Tauri store load or the startup call below.
    watch(logEnabled, v => {
      void persistSetting(LOG_ENABLED_KEY, String(v));
      void applyLogLevel();
      rLog.info(`[Settings] Log enabled: ${v}`);
    });
    watch(logLevel, v => {
      void persistSetting(LOG_LEVEL_KEY, v);
      if (logEnabled.value) void applyLogLevel();
      rLog.info(`[Settings] Log level changed to: ${v}`);
    });

    // Apply log level once on startup (non-Tauri or before Tauri store loads)
    if (!isTauriRuntime()) {
      void applyLogLevel();
    }

    // Handle system theme changes
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
      if (theme.value === 'system') {
        applyThemeToDom(theme.value);
      }
    });
  }

  return {
    theme,
    locale,
    logEnabled,
    logLevel,
    setTheme,
    setLocale,
    setLogEnabled,
    setLogLevel,
    init,
    ready: () => _ready,
  };
});
