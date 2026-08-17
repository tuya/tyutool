import { describe, expect, it } from "vitest";
import {
  parseSerialDebugWorkspace,
  SD_WORKSPACE_VERSION,
  type SerialDebugWorkspaceSerialized,
} from "./serial-debug-workspace";
import {
  DEFAULT_ARCHIVE_LIMIT_MIB,
  DEFAULT_VISIBLE_LOG_WINDOW_LINES,
  MAX_ARCHIVE_LIMIT_MIB,
  MAX_VISIBLE_LOG_WINDOW_LINES,
  MIN_ARCHIVE_LIMIT_MIB,
  MIN_VISIBLE_LOG_WINDOW_LINES,
} from "@/features/serial-debug/constants";

/** A minimal valid record; callers spread a patch over it. */
function base(): SerialDebugWorkspaceSerialized {
  return {
    v: SD_WORKSPACE_VERSION,
    port: "COM3",
    baudRate: 115200,
    customBaudRate: null,
    dataBits: "eight",
    parity: "none",
    stopBits: "one",
    autoRelease: false,
    hexView: false,
    hexBytesPerRow: 16,
    sendMode: "ascii",
    sendAppendCrlf: true,
    sendHistory: ["hello"],
    ansiEnabled: true,
    logFontSize: 12,
    autoSave: false,
    autoSaveDir: "",
    autoSaveTimestamp: true,
    showTimestamp: true,
    showDirBadge: true,
  };
}

describe("parseSerialDebugWorkspace", () => {
  it("returns null for non-object or wrong version", () => {
    expect(parseSerialDebugWorkspace(null)).toBeNull();
    expect(parseSerialDebugWorkspace("x")).toBeNull();
    expect(parseSerialDebugWorkspace(42)).toBeNull();
    expect(parseSerialDebugWorkspace({ v: 99 })).toBeNull();
    expect(parseSerialDebugWorkspace({})).toBeNull();
  });

  it("parses a valid record unchanged", () => {
    const w = parseSerialDebugWorkspace(base());
    expect(w).not.toBeNull();
    expect(w!.port).toBe("COM3");
    expect(w!.baudRate).toBe(115200);
    expect(w!.dataBits).toBe("eight");
    expect(w!.sendHistory).toEqual(["hello"]);
  });

  it("rejects an unrecognized baud rate, falling back to null (chip default)", () => {
    const w = parseSerialDebugWorkspace({ ...base(), baudRate: 12345678 });
    expect(w!.baudRate).toBeNull();
  });

  it("clamps an invalid dataBits enum to the default", () => {
    const w = parseSerialDebugWorkspace({
      ...base(),
      dataBits: "ninety" as never,
    });
    expect(w!.dataBits).toBe("eight");
  });

  it("clamps invalid parity / stopBits / sendMode / hexBytesPerRow", () => {
    const w = parseSerialDebugWorkspace({
      ...base(),
      parity: "mark" as never,
      stopBits: "three" as never,
      sendMode: "binary" as never,
      hexBytesPerRow: 64 as never,
    });
    expect(w!.parity).toBe("none");
    expect(w!.stopBits).toBe("one");
    expect(w!.sendMode).toBe("ascii");
    expect(w!.hexBytesPerRow).toBe(16);
  });

  it("drops non-string sendHistory entries and caps length", () => {
    const long = Array.from({ length: 50 }, (_, i) => `cmd${i}`);
    const w = parseSerialDebugWorkspace({
      ...base(),
      sendHistory: ["ok", 123, null, { x: 1 }, ...long, "tail"],
    });
    expect(w!.sendHistory.every((s) => typeof s === "string")).toBe(true);
    expect(w!.sendHistory.length).toBeLessThanOrEqual(20);
  });

  it("returns [] when sendHistory is not an array", () => {
    const w = parseSerialDebugWorkspace({ ...base(), sendHistory: "oops" });
    expect(w!.sendHistory).toEqual([]);
  });

  it("falls back to 12 for an invalid logFontSize", () => {
    const w1 = parseSerialDebugWorkspace({ ...base(), logFontSize: 7 });
    const w2 = parseSerialDebugWorkspace({ ...base(), logFontSize: "big" });
    expect(w1!.logFontSize).toBe(12);
    expect(w2!.logFontSize).toBe(12);
  });

  it("coerces non-boolean flags to their defaults", () => {
    const w = parseSerialDebugWorkspace({
      ...base(),
      autoRelease: "yes",
      hexView: 1,
      sendAppendCrlf: null,
    });
    expect(w!.autoRelease).toBe(false);
    expect(w!.hexView).toBe(false);
    expect(w!.sendAppendCrlf).toBe(false);
  });

  it("preserves undefined optional fields as undefined", () => {
    const minimal = {
      v: SD_WORKSPACE_VERSION,
      port: "",
      baudRate: null,
      customBaudRate: null,
      dataBits: "eight",
      parity: "none",
      stopBits: "one",
      autoRelease: false,
      hexView: false,
      hexBytesPerRow: 16,
      sendMode: "ascii",
      sendAppendCrlf: false,
      sendHistory: [],
    };
    const w = parseSerialDebugWorkspace(minimal);
    expect(w).not.toBeNull();
    expect(w!.ansiEnabled).toBeUndefined();
    expect(w!.autoSave).toBeUndefined();
    expect(w!.autoSaveDir).toBeUndefined();
  });

  it("falls back to the default logWindowLines when absent or invalid", () => {
    expect(parseSerialDebugWorkspace(base())!.logWindowLines).toBe(
      DEFAULT_VISIBLE_LOG_WINDOW_LINES,
    );
    expect(
      parseSerialDebugWorkspace({ ...base(), logWindowLines: "lots" })!
        .logWindowLines,
    ).toBe(DEFAULT_VISIBLE_LOG_WINDOW_LINES);
    expect(
      parseSerialDebugWorkspace({ ...base(), logWindowLines: Infinity })!
        .logWindowLines,
    ).toBe(DEFAULT_VISIBLE_LOG_WINDOW_LINES);
  });

  it("clamps an out-of-range logWindowLines into [MIN, MAX]", () => {
    expect(
      parseSerialDebugWorkspace({ ...base(), logWindowLines: 1 })!
        .logWindowLines,
    ).toBe(MIN_VISIBLE_LOG_WINDOW_LINES);
    expect(
      parseSerialDebugWorkspace({ ...base(), logWindowLines: 999999 })!
        .logWindowLines,
    ).toBe(MAX_VISIBLE_LOG_WINDOW_LINES);
    expect(
      parseSerialDebugWorkspace({ ...base(), logWindowLines: 3000 })!
        .logWindowLines,
    ).toBe(3000);
  });

  it("falls back to the default archiveLimitMib when absent or invalid", () => {
    expect(parseSerialDebugWorkspace(base())!.archiveLimitMib).toBe(
      DEFAULT_ARCHIVE_LIMIT_MIB,
    );
    expect(
      parseSerialDebugWorkspace({ ...base(), archiveLimitMib: "big" })!
        .archiveLimitMib,
    ).toBe(DEFAULT_ARCHIVE_LIMIT_MIB);
    expect(
      parseSerialDebugWorkspace({ ...base(), archiveLimitMib: Infinity })!
        .archiveLimitMib,
    ).toBe(DEFAULT_ARCHIVE_LIMIT_MIB);
  });

  it("clamps an out-of-range archiveLimitMib into [MIN, MAX]", () => {
    expect(
      parseSerialDebugWorkspace({ ...base(), archiveLimitMib: 0 })!
        .archiveLimitMib,
    ).toBe(MIN_ARCHIVE_LIMIT_MIB);
    expect(
      parseSerialDebugWorkspace({ ...base(), archiveLimitMib: 999999 })!
        .archiveLimitMib,
    ).toBe(MAX_ARCHIVE_LIMIT_MIB);
    expect(
      parseSerialDebugWorkspace({ ...base(), archiveLimitMib: 512 })!
        .archiveLimitMib,
    ).toBe(512);
  });

  it("rejects a negative or non-finite customBaudRate", () => {
    expect(
      parseSerialDebugWorkspace({ ...base(), customBaudRate: -1 })!
        .customBaudRate,
    ).toBeNull();
    expect(
      parseSerialDebugWorkspace({ ...base(), customBaudRate: Infinity })!
        .customBaudRate,
    ).toBeNull();
  });
});
