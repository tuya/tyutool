// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { i18n } from "./index";
import en from "../locales/en.json";
import zhCN from "../locales/zh-CN.json";

const flatten = (obj: Record<string, unknown>, prefix = ""): string[] =>
  Object.entries(obj).flatMap(([k, v]) =>
    v && typeof v === "object"
      ? flatten(v as Record<string, unknown>, `${prefix}${k}.`)
      : [`${prefix}${k}`],
  );

describe("i18n instance", () => {
  it("has fallbackLocale set to en", () => {
    expect(i18n.global.fallbackLocale.value).toBe("en");
  });

  it("has zh-CN and en messages loaded", () => {
    const messages = i18n.global.messages.value;
    expect(messages).toHaveProperty("zh-CN");
    expect(messages).toHaveProperty("en");
  });

  it("zh-CN messages contain app namespace", () => {
    const zhCN = i18n.global.messages.value["zh-CN"] as Record<string, unknown>;
    expect(zhCN).toHaveProperty("app");
    expect(zhCN).toHaveProperty("flash");
    expect(zhCN).toHaveProperty("settings");
  });

  it("en messages contain app namespace", () => {
    const en = i18n.global.messages.value["en"] as Record<string, unknown>;
    expect(en).toHaveProperty("app");
    expect(en).toHaveProperty("flash");
    expect(en).toHaveProperty("settings");
  });

  it("can translate a known key", () => {
    const result = i18n.global.t("app.tagline");
    expect(typeof result).toBe("string");
    expect(result.length).toBeGreaterThan(0);
  });
});

describe("dynamic key families", () => {
  it("EXCEL_ERROR_CODES values are defined in both locales", async () => {
    const { EXCEL_ERROR_CODES } =
      await import("@/features/batch-flash-auth/types");
    for (const key of Object.values(EXCEL_ERROR_CODES)) {
      expect(i18n.global.te(key, "en")).toBe(true);
      expect(i18n.global.te(key, "zh-CN")).toBe(true);
    }
  });

  it("batchFlashAuth.phase.* keys are defined for all currentPhase values", () => {
    const phases = [
      // auth sub-phases
      "reading_mac",
      "reading_auth",
      "writing_auth",
      "verifying",
      "flashing",
      // flash sub-phases (from FlashPhase Rust enum)
      "handshake",
      "connect",
      "switch_baud",
      "read_flash_id",
      "load_ram",
      "erase",
      "write",
      "write_segment",
      "verify",
      "protect",
      "unprotect",
      "reboot",
    ];
    for (const phase of phases) {
      expect(i18n.global.te(`batchFlashAuth.phase.${phase}`, "en")).toBe(true);
    }
  });
});

describe("i18n key coverage", () => {
  const enKeys = new Set(flatten(en as Record<string, unknown>));
  const zhKeys = new Set(flatten(zhCN as Record<string, unknown>));

  // Enforces the CLAUDE.md rule "new keys must be added to both locales".
  it("en and zh-CN have identical key sets", () => {
    const onlyEn = [...enKeys].filter((k) => !zhKeys.has(k));
    const onlyZh = [...zhKeys].filter((k) => !enKeys.has(k));
    expect({ onlyEn, onlyZh }).toEqual({ onlyEn: [], onlyZh: [] });
  });

  // Catches keys referenced in source but never defined (the user only sees the
  // raw key string at runtime). Only literal t('…')/t("…") calls are checked;
  // dynamic template-literal keys (t(`flash.chips.${id}`)) are skipped.
  it("every literal t() key used in source exists in en.json", () => {
    const modules = import.meta.glob("../**/*.{ts,vue}", {
      query: "?raw",
      import: "default",
      eager: true,
    }) as Record<string, string>;
    const keyRe = /[^A-Za-z0-9_]t\(\s*["']([A-Za-z0-9_.]+)["']/g;
    const missing = new Set<string>();
    for (const [path, src] of Object.entries(modules)) {
      if (path.includes(".test.") || path.includes("/locales/")) continue;
      let m: RegExpExecArray | null;
      while ((m = keyRe.exec(src)) !== null) {
        if (!enKeys.has(m[1])) missing.add(m[1]);
      }
    }
    expect([...missing].sort()).toEqual([]);
  });
});
