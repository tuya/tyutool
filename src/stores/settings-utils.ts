/**
 * Pure helpers for settings persistence (localStorage / DOM).
 * Extracted from the Pinia store for independent testability.
 */

import type {
  AutoUpdateIntervalId,
  LocalePreference,
  LogLevelId,
  ThemePreference /*, ThemeStyle*/,
} from "./settings";

const THEME_KEY = "tyutool-theme";
// const THEME_STYLE_KEY = 'tyutool-theme-style';
const LOCALE_KEY = "tyutool-locale";
const LEGACY_THEME_KEY = "tyutools-theme";
const LEGACY_LOCALE_KEY = "tyutools-locale";
const LOG_ENABLED_KEY = "tyutool-log-enabled";
const LOG_LEVEL_KEY = "tyutool-log-level";
const SERIAL_PORT_INDICATORS_ENABLED_KEY =
  "tyutool-serial-port-indicators-enabled";
const AUTO_UPDATE_INTERVAL_KEY = "tyutool-auto-update-interval";
const AUTO_UPDATE_LAST_CHECK_AT_KEY = "tyutool-auto-update-last-check-at";

export {
  THEME_KEY,
  // THEME_STYLE_KEY,
  LOCALE_KEY,
  LEGACY_THEME_KEY,
  LEGACY_LOCALE_KEY,
  LOG_ENABLED_KEY,
  LOG_LEVEL_KEY,
  SERIAL_PORT_INDICATORS_ENABLED_KEY,
  AUTO_UPDATE_INTERVAL_KEY,
  AUTO_UPDATE_LAST_CHECK_AT_KEY,
};

export function loadStoredTheme(): ThemePreference {
  let s = localStorage.getItem(THEME_KEY) as ThemePreference | null;
  if (!s) {
    const legacy = localStorage.getItem(
      LEGACY_THEME_KEY,
    ) as ThemePreference | null;
    if (legacy === "light" || legacy === "dark" || legacy === "system") {
      localStorage.setItem(THEME_KEY, legacy);
      localStorage.removeItem(LEGACY_THEME_KEY);
      s = legacy;
    }
  }
  if (s === "light" || s === "dark" || s === "system") {
    return s;
  }
  return "system";
}

// export function loadStoredThemeStyle(): ThemeStyle {
//   const s = localStorage.getItem(THEME_STYLE_KEY);
//   if (s === 'tuyaopen-ide') return 'tuyaopen-ide';
//   return 'default';
// }

export function loadStoredLocale(): LocalePreference {
  let s = localStorage.getItem(LOCALE_KEY);
  if (!s) {
    const legacy = localStorage.getItem(LEGACY_LOCALE_KEY);
    if (legacy === "zh-CN" || legacy === "en") {
      localStorage.setItem(LOCALE_KEY, legacy);
      localStorage.removeItem(LEGACY_LOCALE_KEY);
      s = legacy;
    }
  }
  if (s === "zh-CN" || s === "en" || s === "auto") {
    return s;
  }
  return "auto";
}

export function loadStoredLogEnabled(): boolean {
  const val = localStorage.getItem(LOG_ENABLED_KEY);
  if (val === null) return true; // 首次使用默认开启
  return val === "true";
}

export function loadStoredLogLevel(): LogLevelId {
  const val = localStorage.getItem(LOG_LEVEL_KEY);
  if (val && ["error", "warn", "info", "debug", "trace"].includes(val)) {
    return val as LogLevelId;
  }
  // 默认 debug:与后端 tauri-plugin-log 的启动级别保持一致,
  // 批量授权排查依赖 debug 级的串口收发日志(见 authorize.rs 的 [serial] 日志)。
  return "debug";
}

export function loadStoredSerialPortIndicatorsEnabled(): boolean {
  const val = localStorage.getItem(SERIAL_PORT_INDICATORS_ENABLED_KEY);
  if (val === null) return true;
  return val === "true";
}

export function loadStoredAutoUpdateInterval(): AutoUpdateIntervalId {
  const val = localStorage.getItem(AUTO_UPDATE_INTERVAL_KEY);
  if (
    val === "off" ||
    val === "1h" ||
    val === "6h" ||
    val === "12h" ||
    val === "24h"
  ) {
    return val;
  }
  return "6h";
}

export function parseStoredAutoUpdateLastCheckAt(
  value: string | number | null | undefined,
): number | null {
  if (value === null || value === undefined) return null;
  const parsed =
    typeof value === "number" ? value : Number.parseInt(String(value), 10);
  return Number.isFinite(parsed) ? parsed : null;
}

export function loadStoredAutoUpdateLastCheckAt(): number | null {
  return parseStoredAutoUpdateLastCheckAt(
    localStorage.getItem(AUTO_UPDATE_LAST_CHECK_AT_KEY),
  );
}

export function applyThemeToDom(
  pref: ThemePreference /*, style: ThemeStyle = 'default'*/,
): void {
  const root = document.documentElement;
  let mode: "light" | "dark" = "dark";
  if (pref === "system") {
    mode = window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  } else {
    mode = pref;
  }
  root.classList.toggle("dark", mode === "dark");
  // root.classList.toggle('tuyaopen-ide', style === 'tuyaopen-ide');
}
