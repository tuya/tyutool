import { describe, expect, it } from "vitest";
import { parseFlashWorkspaceJson, WORKSPACE_VERSION } from "./flash-workspace";

describe("parseFlashWorkspaceJson", () => {
  it("returns null for empty or invalid input", () => {
    expect(parseFlashWorkspaceJson(null)).toBeNull();
    expect(parseFlashWorkspaceJson("")).toBeNull();
    expect(parseFlashWorkspaceJson("not json")).toBeNull();
    expect(parseFlashWorkspaceJson("{}")).toBeNull();
  });

  it("parses a minimal valid workspace", () => {
    const raw = JSON.stringify({
      v: WORKSPACE_VERSION,
      activeTab: "flash",
      selectedSerialPort: "/dev/ttyUSB0",
      selectedBaudRate: 921600,
      selectedChipId: "t5ai",
      flashSegments: [
        {
          id: "seg1",
          firmwarePath: "/tmp/a.bin",
          startAddr: "0x00000000",
          endAddr: "0x00001000",
        },
      ],
      activeSegmentIndex: 0,
      eraseAdvancedOpen: false,
      eraseStartAddr: "0x00000000",
      eraseEndAddr: "0x00100000",
      readStartAddr: "0x00000000",
      readEndAddr: "0x00800000",
      readDir: "/home/u/out",
      readFileName: "dump.bin",
      readFileNameModified: true,
      authorizeUuid: "",
      authorizeAuthKey: "",
      authBaudRate: 115200,
    });
    const w = parseFlashWorkspaceJson(raw);
    expect(w).not.toBeNull();
    expect(w!.selectedChipId).toBe("t5ai");
    expect(w!.flashSegments).toHaveLength(1);
    expect(w!.flashSegments[0].firmwarePath).toBe("/tmp/a.bin");
    expect(w!.readFileNameModified).toBe(true);
    expect(w!.rememberAuth).toBe(false);
  });

  it("preserves authorize tab for ESP chips (UART-only flow)", () => {
    const raw = JSON.stringify({
      v: WORKSPACE_VERSION,
      activeTab: "authorize",
      selectedSerialPort: "",
      selectedBaudRate: 115200,
      selectedChipId: "esp32",
      flashSegments: [
        {
          id: "a",
          firmwarePath: "",
          startAddr: "0x00000000",
          endAddr: "0x00000000",
        },
      ],
      activeSegmentIndex: 0,
      eraseAdvancedOpen: false,
      eraseStartAddr: "0x0",
      eraseEndAddr: "0x0",
      readStartAddr: "0x0",
      readEndAddr: "0x0",
      readDir: "",
      readFileName: "x.bin",
      readFileNameModified: false,
      authorizeUuid: "",
      authorizeAuthKey: "",
      authBaudRate: 115200,
    });
    const w = parseFlashWorkspaceJson(raw);
    expect(w!.activeTab).toBe("authorize");
  });

  it("accepts other chip on authorize tab only", () => {
    const authorizeRaw = JSON.stringify({
      v: WORKSPACE_VERSION,
      activeTab: "authorize",
      selectedSerialPort: "",
      selectedBaudRate: 115200,
      selectedChipId: "other",
      flashSegments: [
        {
          id: "a",
          firmwarePath: "",
          startAddr: "0x00000000",
          endAddr: "0x00000000",
        },
      ],
      activeSegmentIndex: 0,
      eraseAdvancedOpen: false,
      eraseStartAddr: "0x0",
      eraseEndAddr: "0x0",
      readStartAddr: "0x0",
      readEndAddr: "0x0",
      readDir: "",
      readFileName: "x.bin",
      readFileNameModified: false,
      authorizeUuid: "",
      authorizeAuthKey: "",
      authBaudRate: 115200,
    });
    expect(parseFlashWorkspaceJson(authorizeRaw)?.selectedChipId).toBe("other");

    const flashRaw = authorizeRaw.replace('"authorize"', '"flash"');
    expect(parseFlashWorkspaceJson(flashRaw)).toBeNull();
  });

  it("upgrades a saved legacy 't5' chipId to 't5ai'", () => {
    const raw = JSON.stringify({
      v: WORKSPACE_VERSION,
      activeTab: "flash",
      selectedSerialPort: "/dev/ttyUSB0",
      selectedBaudRate: 921600,
      selectedChipId: "t5",
      flashSegments: [
        {
          id: "seg1",
          firmwarePath: "/tmp/a.bin",
          startAddr: "0x00000000",
          endAddr: "0x00001000",
        },
      ],
      activeSegmentIndex: 0,
      eraseAdvancedOpen: false,
      eraseStartAddr: "0x00000000",
      eraseEndAddr: "0x00100000",
      readStartAddr: "0x00000000",
      readEndAddr: "0x00800000",
      readDir: "",
      readFileName: "dump.bin",
      readFileNameModified: false,
      authorizeUuid: "",
      authorizeAuthKey: "",
      authBaudRate: 115200,
    });
    const w = parseFlashWorkspaceJson(raw);
    expect(w).not.toBeNull();
    expect(w!.selectedChipId).toBe("t5ai");
  });

  it("defaults rememberAuth to false when absent", () => {
    const raw = JSON.stringify({
      v: WORKSPACE_VERSION,
      activeTab: "authorize",
      selectedSerialPort: "",
      selectedBaudRate: 115200,
      selectedChipId: "other",
      flashSegments: [
        {
          id: "a",
          firmwarePath: "",
          startAddr: "0x00000000",
          endAddr: "0x00000000",
        },
      ],
      activeSegmentIndex: 0,
      eraseAdvancedOpen: false,
      eraseStartAddr: "0x0",
      eraseEndAddr: "0x0",
      readStartAddr: "0x0",
      readEndAddr: "0x0",
      readDir: "",
      readFileName: "x.bin",
      readFileNameModified: false,
      authorizeUuid: "UUID20CHARS",
      authorizeAuthKey: "KEY32CHARS",
      authBaudRate: 115200,
    });
    const w = parseFlashWorkspaceJson(raw);
    expect(w!.rememberAuth).toBe(false);
    // credentials pass through the parser verbatim; the store gates
    // persistence on rememberAuth, not the parser.
    expect(w!.authorizeUuid).toBe("UUID20CHARS");
  });

  it("preserves rememberAuth when explicitly true", () => {
    const raw = JSON.stringify({
      v: WORKSPACE_VERSION,
      activeTab: "authorize",
      selectedSerialPort: "",
      selectedBaudRate: 115200,
      selectedChipId: "other",
      flashSegments: [
        {
          id: "a",
          firmwarePath: "",
          startAddr: "0x00000000",
          endAddr: "0x00000000",
        },
      ],
      activeSegmentIndex: 0,
      eraseAdvancedOpen: false,
      eraseStartAddr: "0x0",
      eraseEndAddr: "0x0",
      readStartAddr: "0x0",
      readEndAddr: "0x0",
      readDir: "",
      readFileName: "x.bin",
      readFileNameModified: false,
      authorizeUuid: "UUID20CHARS",
      authorizeAuthKey: "KEY32CHARS",
      authBaudRate: 115200,
      rememberAuth: true,
    });
    expect(parseFlashWorkspaceJson(raw)?.rememberAuth).toBe(true);
  });
});
