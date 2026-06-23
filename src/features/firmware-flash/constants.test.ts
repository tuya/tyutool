import { describe, expect, it } from "vitest";
import {
  AUTH_CHIP_IDS,
  AUTH_ONLY_CHIP_ID,
  BAUD_RATE_OPTIONS,
  CHIP_IDS,
  DEFAULT_CHIP_ID,
  normalizeChipId,
  SERIAL_PORT_OPTIONS,
} from "./constants";

describe("CHIP_IDS", () => {
  it("is a non-empty array", () => {
    expect(CHIP_IDS.length).toBeGreaterThan(0);
  });

  it("is sorted by ASCII string order", () => {
    const copy = [...CHIP_IDS];
    copy.sort();
    expect([...CHIP_IDS]).toEqual(copy);
  });

  it("uses t5ai as default chip for first launch (not necessarily first in list)", () => {
    expect(DEFAULT_CHIP_ID).toBe("t5ai");
  });

  it("contains all expected chip IDs", () => {
    expect(CHIP_IDS).toContain("esp32");
    expect(CHIP_IDS).toContain("esp32c3");
    expect(CHIP_IDS).toContain("esp32c6");
    expect(CHIP_IDS).toContain("esp32s3");
    expect(CHIP_IDS).toContain("t5ai");
    expect(CHIP_IDS).toContain("t2");
    expect(CHIP_IDS).toContain("bk7231n");
  });
});

describe("AUTH_CHIP_IDS", () => {
  it("includes all flash chips plus the auth-only other option", () => {
    expect(AUTH_CHIP_IDS).toEqual([...CHIP_IDS, AUTH_ONLY_CHIP_ID]);
  });

  it("does not include other in CHIP_IDS", () => {
    expect(CHIP_IDS).not.toContain(AUTH_ONLY_CHIP_ID);
  });
});

describe("BAUD_RATE_OPTIONS", () => {
  it("is a non-empty array", () => {
    expect(BAUD_RATE_OPTIONS.length).toBeGreaterThan(0);
  });

  it("contains standard baud rates", () => {
    expect(BAUD_RATE_OPTIONS).toContain(115200);
    expect(BAUD_RATE_OPTIONS).toContain(921600);
  });

  it("all values are positive numbers", () => {
    for (const rate of BAUD_RATE_OPTIONS) {
      expect(rate).toBeGreaterThan(0);
    }
  });

  it("values are sorted ascending", () => {
    for (let i = 1; i < BAUD_RATE_OPTIONS.length; i++) {
      expect(BAUD_RATE_OPTIONS[i]).toBeGreaterThan(BAUD_RATE_OPTIONS[i - 1]);
    }
  });
});

describe("SERIAL_PORT_OPTIONS", () => {
  it("starts as an empty array (populated at runtime)", () => {
    expect(SERIAL_PORT_OPTIONS).toEqual([]);
  });
});

describe("normalizeChipId", () => {
  it("maps the legacy t5 / T5 ids to t5ai", () => {
    expect(normalizeChipId("t5")).toBe("t5ai");
    expect(normalizeChipId("T5")).toBe("t5ai");
    expect(normalizeChipId("  T5  ")).toBe("t5ai");
  });

  it("passes through ids that need no remap (lower-cased)", () => {
    expect(normalizeChipId("t5ai")).toBe("t5ai");
    expect(normalizeChipId("ESP32")).toBe("esp32");
    expect(normalizeChipId("bk7231n")).toBe("bk7231n");
  });

  it("leaves the auth-only id and unknown ids alone", () => {
    expect(normalizeChipId(AUTH_ONLY_CHIP_ID)).toBe(AUTH_ONLY_CHIP_ID);
    expect(normalizeChipId("nonexistent")).toBe("nonexistent");
  });
});
