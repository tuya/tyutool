import { describe, it, expect } from "vitest";
import { filterByChip, AUTH_FIRMWARE_SOURCES } from "./auth-firmware";
import type { AuthFirmwareEntry } from "./types";

const mk = (version: string, chip: string): AuthFirmwareEntry => ({
  version,
  chip,
  url: `https://example.com/${chip}-${version}.bin`,
  sha256: "00",
});

describe("filterByChip", () => {
  it("keeps only matching chip and sorts by version descending", () => {
    const entries = [
      mk("1.0.0", "esp32"),
      mk("1.2.0", "esp32"),
      mk("1.10.0", "esp32"),
      mk("2.0.0", "other"),
    ];
    const out = filterByChip(entries, "esp32");
    expect(out.map((e) => e.version)).toEqual(["1.10.0", "1.2.0", "1.0.0"]);
  });

  it("returns empty array when no entry matches", () => {
    expect(filterByChip([mk("1.0.0", "esp32")], "gd32")).toEqual([]);
  });

  it("tolerates a leading 'v' in version strings", () => {
    const out = filterByChip(
      [mk("v1.0.0", "esp32"), mk("v1.1.0", "esp32")],
      "esp32",
    );
    expect(out.map((e) => e.version)).toEqual(["v1.1.0", "v1.0.0"]);
  });
});

describe("AUTH_FIRMWARE_SOURCES", () => {
  it("lists GitHub first then Gitee, both pointing at the auth-firmware manifest", () => {
    expect(AUTH_FIRMWARE_SOURCES.map((s) => s.id)).toEqual(["github", "gitee"]);
    for (const s of AUTH_FIRMWARE_SOURCES) {
      expect(s.url).toContain("auth-firmware/auth-firmware.json");
    }
  });
});
