// @vitest-environment happy-dom
//
// init()/persistence coverage for the settings store. Uses happy-dom so
// init()'s DOM side effects (applyThemeToDom, window.matchMedia listener,
// document.documentElement.lang) run without manual stubs. The web runtime
// path (isTauriRuntime=false) persists via localStorage.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { nextTick } from "vue";

vi.mock("@/runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime")>()),
  isTauriRuntime: vi.fn(() => false),
}));

const invokeSpy = vi.fn(async () => undefined);
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeSpy }));

import { useSettingsStore } from "./settings";
import {
  THEME_KEY,
  THEME_STYLE_KEY,
  LOCALE_KEY,
  LOG_ENABLED_KEY,
  LOG_LEVEL_KEY,
} from "./settings-utils";

describe("useSettingsStore init() + web persistence", () => {
  beforeEach(() => {
    // happy-dom provides a real localStorage; clear it for isolation.
    localStorage.clear();
    document.documentElement.className = "";
    setActivePinia(createPinia());
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("applies the theme to the DOM on init() (dark class set from current pref)", () => {
    const s = useSettingsStore();
    s.setTheme("dark");
    s.init();
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });

  it("init() in web mode resolves ready() immediately (no Tauri store load)", async () => {
    const s = useSettingsStore();
    s.init();
    await expect(s.ready()).resolves.toBeUndefined();
  });

  it("changing theme persists to localStorage and re-applies the DOM class", async () => {
    const s = useSettingsStore();
    s.init();

    s.setTheme("dark");
    await nextTick();
    expect(localStorage.getItem(THEME_KEY)).toBe("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    s.setTheme("light");
    await nextTick();
    expect(localStorage.getItem(THEME_KEY)).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("changing themeStyle persists and toggles the tuyaopen-ide class", async () => {
    const s = useSettingsStore();
    s.init();

    s.setThemeStyle("tuyaopen-ide");
    await nextTick();
    expect(localStorage.getItem(THEME_STYLE_KEY)).toBe("tuyaopen-ide");
    expect(document.documentElement.classList.contains("tuyaopen-ide")).toBe(
      true,
    );

    s.setThemeStyle("default");
    await nextTick();
    expect(localStorage.getItem(THEME_STYLE_KEY)).toBe("default");
    expect(document.documentElement.classList.contains("tuyaopen-ide")).toBe(
      false,
    );
  });

  it("changing locale persists and updates document lang", async () => {
    const s = useSettingsStore();
    s.init();

    s.setLocale("zh-CN");
    await nextTick();
    expect(localStorage.getItem(LOCALE_KEY)).toBe("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");

    s.setLocale("en");
    await nextTick();
    expect(localStorage.getItem(LOCALE_KEY)).toBe("en");
    expect(document.documentElement.lang).toBe("en");
  });

  it("changing logEnabled persists the boolean as a string", async () => {
    const s = useSettingsStore();
    s.init();

    s.setLogEnabled(false);
    await nextTick();
    expect(localStorage.getItem(LOG_ENABLED_KEY)).toBe("false");

    s.setLogEnabled(true);
    await nextTick();
    expect(localStorage.getItem(LOG_ENABLED_KEY)).toBe("true");
  });

  it("changing logLevel persists the new level", async () => {
    const s = useSettingsStore();
    s.init();

    s.setLogLevel("debug");
    await nextTick();
    expect(localStorage.getItem(LOG_LEVEL_KEY)).toBe("debug");

    s.setLogLevel("trace");
    await nextTick();
    expect(localStorage.getItem(LOG_LEVEL_KEY)).toBe("trace");
  });

  it("applyLogLevel is a no-op in web mode (never invokes set_log_level)", async () => {
    const s = useSettingsStore();
    invokeSpy.mockClear();
    s.init();
    // The logEnabled/logLevel watchers call applyLogLevel, which returns early
    // before any dynamic import when isTauriRuntime() is false.
    s.setLogEnabled(false);
    await nextTick();
    s.setLogLevel("trace");
    s.setLogEnabled(true);
    await nextTick();
    expect(invokeSpy).not.toHaveBeenCalled();
  });
});
