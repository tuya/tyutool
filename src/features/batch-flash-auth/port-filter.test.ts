// src/features/batch-flash-auth/port-filter.test.ts
import { describe, it, expect } from "vitest";
import { normalizePortName, applyPortFilter } from "./port-filter";

const isWin =
  (typeof navigator !== "undefined" &&
    navigator.platform?.toLowerCase().includes("win")) ||
  process.platform === "win32";

describe("normalizePortName", () => {
  if (isWin) {
    it("uppercases COM ports on Windows for case-insensitive matching", () => {
      expect(normalizePortName("com3")).toBe("COM3");
      expect(normalizePortName("COM3")).toBe("COM3");
    });

    it("leaves non-COM port strings unchanged on Windows", () => {
      expect(normalizePortName("/dev/ttyUSB0")).toBe("/dev/ttyUSB0");
    });
  } else {
    it("leaves port names unchanged on Unix", () => {
      expect(normalizePortName("/dev/ttyUSB0")).toBe("/dev/ttyUSB0");
      expect(normalizePortName("COM3")).toBe("COM3");
    });
  }
});

describe("applyPortFilter", () => {
  it("returns all ports when blockedPorts is empty", () => {
    const ports = ["/dev/ttyUSB0", "/dev/ttyUSB1"];
    expect(applyPortFilter(ports, [])).toEqual(ports);
  });

  it("removes a single blocked port", () => {
    expect(
      applyPortFilter(
        ["/dev/ttyUSB0", "/dev/ttyS0", "/dev/ttyUSB1"],
        ["/dev/ttyS0"],
      ),
    ).toEqual(["/dev/ttyUSB0", "/dev/ttyUSB1"]);
  });

  it("removes multiple blocked ports", () => {
    expect(applyPortFilter(["COM1", "COM3", "COM5"], ["COM1", "COM5"])).toEqual(
      ["COM3"],
    );
  });

  if (isWin) {
    it("matches blocked COM ports case-insensitively on Windows", () => {
      expect(applyPortFilter(["COM1", "COM3"], ["com1"])).toEqual(["COM3"]);
    });
  }

  it("returns empty array when all ports are blocked", () => {
    expect(applyPortFilter(["COM1", "COM2"], ["COM1", "COM2"])).toEqual([]);
  });

  it("does not remove ports that are not blocked", () => {
    expect(
      applyPortFilter(["/dev/ttyUSB0", "/dev/ttyUSB1"], ["/dev/ttyACM0"]),
    ).toEqual(["/dev/ttyUSB0", "/dev/ttyUSB1"]);
  });
});
