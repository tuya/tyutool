import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

vi.mock("@/runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime")>()),
  isTauriRuntime: vi.fn(() => false),
}));

import { resolveLocale, useSettingsStore } from "./settings";

describe("resolveLocale", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("returns zh-CN when preference is auto and browser language starts with zh", () => {
    vi.stubGlobal("navigator", { language: "zh-CN" });
    expect(resolveLocale("auto")).toBe("zh-CN");
  });

  it("returns zh-CN for zh-TW browser language", () => {
    vi.stubGlobal("navigator", { language: "zh-TW" });
    expect(resolveLocale("auto")).toBe("zh-CN");
  });

  it("returns en when preference is auto and browser language is en-US", () => {
    vi.stubGlobal("navigator", { language: "en-US" });
    expect(resolveLocale("auto")).toBe("en");
  });

  it("returns en when preference is auto and browser language is ja", () => {
    vi.stubGlobal("navigator", { language: "ja" });
    expect(resolveLocale("auto")).toBe("en");
  });

  it("returns en when preference is auto and navigator.language is empty", () => {
    vi.stubGlobal("navigator", { language: "" });
    expect(resolveLocale("auto")).toBe("en");
  });

  it("returns zh-CN when preference is zh-CN", () => {
    expect(resolveLocale("zh-CN")).toBe("zh-CN");
  });

  it("returns en when preference is en", () => {
    expect(resolveLocale("en")).toBe("en");
  });
});

describe("useSettingsStore setters", () => {
  beforeEach(() => {
    // The store reads persisted prefs from localStorage on creation; provide a
    // minimal in-memory stub so the setup store can be constructed in node env.
    const mem = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => (mem.has(k) ? mem.get(k)! : null),
      setItem: (k: string, v: string) => void mem.set(k, v),
      removeItem: (k: string) => void mem.delete(k),
    });
    setActivePinia(createPinia());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults to system theme, auto locale, log enabled at debug", () => {
    const s = useSettingsStore();
    expect(s.theme).toBe("system");
    expect(s.locale).toBe("auto");
    expect(s.logEnabled).toBe(true);
    expect(s.logLevel).toBe("debug");
    expect(s.serialPortIndicatorsEnabled).toBe(true);
    expect(s.autoUpdateInterval).toBe("6h");
  });

  it("setTheme mutates the theme ref", () => {
    const s = useSettingsStore();
    s.setTheme("dark");
    expect(s.theme).toBe("dark");
    s.setTheme("light");
    expect(s.theme).toBe("light");
  });

  it("setLocale mutates the locale ref", () => {
    const s = useSettingsStore();
    s.setLocale("zh-CN");
    expect(s.locale).toBe("zh-CN");
    s.setLocale("en");
    expect(s.locale).toBe("en");
  });

  it("setLogEnabled mutates the logEnabled ref", () => {
    const s = useSettingsStore();
    s.setLogEnabled(false);
    expect(s.logEnabled).toBe(false);
    s.setLogEnabled(true);
    expect(s.logEnabled).toBe(true);
  });

  it("setLogLevel mutates the logLevel ref", () => {
    const s = useSettingsStore();
    s.setLogLevel("debug");
    expect(s.logLevel).toBe("debug");
    s.setLogLevel("trace");
    expect(s.logLevel).toBe("trace");
  });

  it("setSerialPortIndicatorsEnabled mutates the flag", () => {
    const s = useSettingsStore();
    s.setSerialPortIndicatorsEnabled(false);
    expect(s.serialPortIndicatorsEnabled).toBe(false);
    s.setSerialPortIndicatorsEnabled(true);
    expect(s.serialPortIndicatorsEnabled).toBe(true);
  });

  it("setAutoUpdateInterval mutates the interval", () => {
    const s = useSettingsStore();
    s.setAutoUpdateInterval("off");
    expect(s.autoUpdateInterval).toBe("off");
    s.setAutoUpdateInterval("24h");
    expect(s.autoUpdateInterval).toBe("24h");
  });
});
