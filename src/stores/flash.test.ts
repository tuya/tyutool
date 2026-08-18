// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { nextTick } from "vue";
import {
  __setSerialDebugTransportForTest,
  type SerialDebugTransport,
} from "@/features/serial-debug/transport";

// Mock isTauriRuntime before any store import
vi.mock("@/runtime", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/runtime")>();
  return {
    ...actual,
    isTauriRuntime: vi.fn(() => false),
  };
});

// Mock ws-transport: runJob uses setInterval to simulate WS progress so fake timers work
vi.mock("@/transport/ws-transport", () => {
  let cancelFn: (() => void) | null = null;
  const runJob = vi.fn(
    (
      _job: unknown,
      _file: unknown,
      onProgress: (ev: {
        payload: {
          kind: string;
          value?: number;
          phase?: string;
          result?: { ok: { elapsed_secs: number } };
        };
      }) => void,
    ) => {
      // Fire-and-forget: start setInterval and simulate WS progress
      let step = 0;
      const timer = setInterval(() => {
        step += 1;
        const next = Math.min(100, step * 4);
        onProgress({ payload: { kind: "percent", value: next } });
        if (next >= 100) {
          clearInterval(timer);
          cancelFn = null;
          onProgress({
            payload: { kind: "done", result: { ok: { elapsed_secs: 1.0 } } },
          });
        }
      }, 220);
      cancelFn = () => {
        clearInterval(timer);
        cancelFn = null;
      };
      return Promise.resolve();
    },
  );
  const listPorts = vi.fn(async () => [] as string[]);
  const cancelJob = vi.fn(() => {
    cancelFn?.();
    cancelFn = null;
  });
  const authorizeConfirm = vi.fn((_confirmed: boolean) => {});
  const deviceReset = vi.fn(async () => {});
  return {
    wsTransport: {
      runJob,
      listPorts,
      cancelJob,
      authorizeConfirm,
      deviceReset,
    },
  };
});

// Workspace persistence is exercised via spies; stub the storage layer so tests
// don't touch real localStorage/disk.
vi.mock("@/stores/flash-workspace", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/stores/flash-workspace")>();
  return {
    ...actual,
    loadFlashWorkspaceFromStorage: vi.fn(async () => null),
    saveFlashWorkspaceToStorage: vi.fn(async () => {}),
  };
});

// Confirm dialog: default to confirmed; individual tests override per-call.
vi.mock("@/composables/confirmDialog", () => ({
  showConfirmDialog: vi.fn(async () => true),
}));

// Now import store (it will see the mocked isTauriRuntime)
import { useFlashStore } from "./flash";
import { useSerialDebugStore } from "./serial-debug";
import { wsTransport } from "@/transport/ws-transport";
import { showConfirmDialog } from "@/composables/confirmDialog";
import {
  loadFlashWorkspaceFromStorage,
  saveFlashWorkspaceToStorage,
} from "@/stores/flash-workspace";

function makeSerialDebugTransportWithDeferredClose() {
  let resolveClose: (() => void) | null = null;
  const transport: SerialDebugTransport = {
    async open() {},
    async close() {
      await new Promise<void>((resolve) => {
        resolveClose = resolve;
      });
    },
    async send() {},
    async clearSession() {},
    async appendSysLine() {
      return null;
    },
    async addFilter() {
      throw new Error("not implemented");
    },
    async removeFilter() {},
    async readFilterMatches() {
      return { filterId: "unused", totalMatches: 0, start: 0, items: [] };
    },
    async readSessionPage() {
      return { totalLines: 0, start: 0, items: [] };
    },
    async setArchiveLimit() {},
    onChunk() {
      return () => {};
    },
    onChunkBatch() {
      return () => {};
    },
    onDisconnect() {
      return () => {};
    },
    onFilterUpdated() {
      return () => {};
    },
    onArchiveCapped() {
      return () => {};
    },
  };

  return {
    transport,
    finishClose() {
      resolveClose?.();
      resolveClose = null;
    },
  };
}

describe("flash store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    // Reset call history on the shared module-level mocks (keeps implementations).
    vi.mocked(wsTransport.runJob).mockClear();
    vi.mocked(wsTransport.cancelJob).mockClear();
    vi.mocked(wsTransport.authorizeConfirm).mockClear();
    vi.mocked(showConfirmDialog).mockClear();
    vi.mocked(saveFlashWorkspaceToStorage).mockClear();
    vi.mocked(loadFlashWorkspaceFromStorage).mockClear();
  });

  afterEach(() => {
    // Kill any setInterval the runJob mock left running for an operation that
    // a test started but did not drive to completion, so a leaked timer can't
    // fire its captured onProgress into the next test's disposed store.
    vi.clearAllTimers();
    vi.useRealTimers();
    __setSerialDebugTransportForTest(null);
  });

  // validateOperation requires uuid length 20 and authkey length 32.
  const VALID_UUID = "u".repeat(20);
  const VALID_KEY = "k".repeat(32);
  const OTHER_UUID = "z".repeat(20);
  const OTHER_KEY = "y".repeat(32);

  // ── Initial state ───────────────────────────────────────────────

  describe("initial state", () => {
    it("has correct defaults", () => {
      const store = useFlashStore();
      expect(store.connected).toBe(false);
      expect(store.selectedSerialPort).toBe("");
      expect(store.selectedChipId).toBe("t5ai");
      expect(store.flashSegments.length).toBe(1);
      expect(store.flashSegments[0].firmwarePath).toBe("");
      expect(store.flashPhase).toBe("idle");
      expect(store.flashProgress).toBe(0);
      expect(store.flashMessage).toBe("");
      expect(store.runningOp).toBeNull();
      expect(store.autoConnected).toBe(false);
    });

    it("has default addresses", () => {
      const store = useFlashStore();
      expect(store.flashStartAddr).toBe("0x00000000");
      expect(store.flashEndAddr).toBe("0x00000000");
      expect(store.eraseStartAddr).toBe("0x00000000");
      expect(store.eraseEndAddr).toBe("0x00000000");
      expect(store.readStartAddr).toBe("0x00000000");
      // readEndAddr comes from chipManifest('t5ai').flashSize (8 MiB)
      expect(store.readEndAddr).toBe("0x00800000");
    });

    it("has initial log line", () => {
      const store = useFlashStore();
      expect(store.logLines.length).toBe(1);
    });

    it("has chip IDs in ASCII order", () => {
      const store = useFlashStore();
      const ids = [...store.CHIP_IDS];
      expect(ids.length).toBeGreaterThan(0);
      const sorted = [...ids].sort();
      expect(ids).toEqual(sorted);
    });
  });

  // ── segment management ──────────────────────────────────────────

  describe("segment management", () => {
    it("adds a segment up to 10", () => {
      const store = useFlashStore();
      for (let i = 0; i < 9; i++) {
        store.addSegment();
      }
      expect(store.flashSegments.length).toBe(10);
      store.addSegment(); // Should not add 11th
      expect(store.flashSegments.length).toBe(10);
    });

    it("removes a segment but not the first one", () => {
      const store = useFlashStore();
      store.addSegment();
      expect(store.flashSegments.length).toBe(2);
      store.removeSegment(1);
      expect(store.flashSegments.length).toBe(1);
      store.removeSegment(0); // Should not remove index 0
      expect(store.flashSegments.length).toBe(1);
    });

    it("chains new segment start/end from previous segment end address", () => {
      const store = useFlashStore();
      store.flashSegments[0].endAddr = "0x00001000";
      store.addSegment();
      expect(store.flashSegments[1].startAddr).toBe("0x00001000");
      expect(store.flashSegments[1].endAddr).toBe("0x00001000");
      store.flashSegments[1].endAddr = "0x00002000";
      store.addSegment();
      expect(store.flashSegments[2].startAddr).toBe("0x00002000");
      expect(store.flashSegments[2].endAddr).toBe("0x00002000");
    });
  });

  describe("appendLog", () => {
    it("adds a timestamped log line", () => {
      const store = useFlashStore();
      const before = store.logLines.length;
      store.appendLog("test message");
      expect(store.logLines.length).toBe(before + 1);
      expect(store.logLines[store.logLines.length - 1]).toContain(
        "test message",
      );
      // Should have timestamp prefix [HH:MM:SS]
      expect(store.logLines[store.logLines.length - 1]).toMatch(/^\[[\d:]+\]/);
    });

    it("truncates log at 500 lines", () => {
      const store = useFlashStore();
      // Fill up to 510 lines
      for (let i = 0; i < 510; i++) {
        store.appendLog(`line ${i}`);
      }
      expect(store.logLines.length).toBeLessThanOrEqual(500);
    });
  });

  // ── clearLogs ───────────────────────────────────────────────────

  describe("clearLogs", () => {
    it("clears all logs and adds cleared message", () => {
      const store = useFlashStore();
      store.appendLog("something");
      store.appendLog("something else");
      store.clearLogs();
      // Should have exactly 1 log: the "cleared" message
      expect(store.logLines.length).toBe(1);
    });
  });

  // ── computed: busy ──────────────────────────────────────────────

  describe("busy", () => {
    it("is false when idle", () => {
      const store = useFlashStore();
      expect(store.busy).toBe(false);
    });

    it("is true when running", () => {
      const store = useFlashStore();
      store.flashPhase = "running";
      expect(store.busy).toBe(true);
    });

    it("is false when success", () => {
      const store = useFlashStore();
      store.flashPhase = "success";
      expect(store.busy).toBe(false);
    });

    it("is false when error", () => {
      const store = useFlashStore();
      store.flashPhase = "error";
      expect(store.busy).toBe(false);
    });
  });

  // ── computed: canFlash / canErase / canRead ────────────────────

  describe("canFlash", () => {
    it("is false when no firmware and no port", () => {
      const store = useFlashStore();
      expect(store.canFlash).toBe(false);
    });

    it("is true when firmware and port are set", () => {
      const store = useFlashStore();
      store.flashSegments[0].firmwarePath = "/path/to/fw.bin";
      store.selectedSerialPort = "/dev/ttyUSB0";
      expect(store.canFlash).toBe(true);
    });

    it("is false when busy", () => {
      const store = useFlashStore();
      store.flashSegments[0].firmwarePath = "/path/to/fw.bin";
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.flashPhase = "running";
      expect(store.canFlash).toBe(false);
    });
  });

  describe("canErase", () => {
    it("is false without port", () => {
      const store = useFlashStore();
      expect(store.canErase).toBe(false);
    });

    it("is true with port", () => {
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      expect(store.canErase).toBe(true);
    });
  });

  describe("canRead", () => {
    it("in web mode (isTauriRuntime=false) requires only fileName and port — readDir not needed", () => {
      const store = useFlashStore();
      expect(store.canRead).toBe(false);

      store.selectedSerialPort = "/dev/ttyUSB0";
      // readFileName has a default; in web mode readDir is not required
      expect(store.canRead).toBe(true);
    });
  });

  // ── computed: readFilePath ──────────────────────────────────────

  describe("readFilePath", () => {
    it("returns empty when dir or name is empty", () => {
      const store = useFlashStore();
      store.readDir = "";
      expect(store.readFilePath).toBe("");
    });

    it("joins dir and name with separator", () => {
      const store = useFlashStore();
      store.readDir = "/home/user";
      store.readFileName = "output.bin";
      expect(store.readFilePath).toBe("/home/user/output.bin");
    });

    it("does not double separator when dir ends with /", () => {
      const store = useFlashStore();
      store.readDir = "/home/user/";
      store.readFileName = "output.bin";
      expect(store.readFilePath).toBe("/home/user/output.bin");
    });
  });

  // ── computed: statusText ────────────────────────────────────────

  describe("statusText", () => {
    it("reflects connection state", () => {
      const store = useFlashStore();
      const disconnectedText = store.statusText;
      store.connected = true;
      const connectedText = store.statusText;
      // They should be different strings
      expect(disconnectedText).not.toBe(connectedText);
    });
  });

  // ── computed: tabList ───────────────────────────────────────────

  describe("tabList", () => {
    it("has 4 tabs including authorize for ESP chips (UART-only)", () => {
      const store = useFlashStore();
      store.selectedChipId = "esp32";
      expect(store.tabList.length).toBe(4);
      expect(store.tabList.map((t) => t.id)).toEqual([
        "flash",
        "erase",
        "read",
        "authorize",
      ]);
    });

    it("has 4 tabs for Beken chips (includes authorize)", () => {
      const store = useFlashStore();
      store.selectedChipId = "t5ai";
      expect(store.tabList.length).toBe(4);
      expect(store.tabList.map((t) => t.id)).toEqual([
        "flash",
        "erase",
        "read",
        "authorize",
      ]);
    });

    it("has 4 tabs for ln882h (read supported)", () => {
      const store = useFlashStore();
      store.selectedChipId = "ln882h";
      expect(store.tabList.length).toBe(4);
      expect(store.tabList.map((t) => t.id)).toEqual([
        "flash",
        "erase",
        "read",
        "authorize",
      ]);
    });
  });

  // ── disconnect ──────────────────────────────────────────────────

  describe("disconnect", () => {
    it("sets connected to false and logs", () => {
      const store = useFlashStore();
      store.connected = true;
      const logsBefore = store.logLines.length;
      store.disconnect();
      expect(store.connected).toBe(false);
      expect(store.autoConnected).toBe(false);
      expect(store.logLines.length).toBeGreaterThan(logsBefore);
    });

    it("cancels running operation on disconnect", () => {
      const store = useFlashStore();
      store.connected = true;
      store.flashPhase = "running";
      store.runningOp = "flash";
      store.disconnect();
      expect(store.connected).toBe(false);
      expect(store.flashPhase).toBe("idle");
      expect(store.runningOp).toBeNull();
      expect(store.flashProgress).toBe(0);
    });
  });

  // ── resetFlash ──────────────────────────────────────────────────

  describe("resetFlash", () => {
    it("resets flash state when idle", () => {
      const store = useFlashStore();
      store.flashPhase = "success";
      store.flashProgress = 100;
      store.flashMessage = "done";
      store.resetFlash();
      expect(store.flashPhase).toBe("idle");
      expect(store.flashProgress).toBe(0);
      expect(store.flashMessage).toBe("");
      expect(store.runningOp).toBeNull();
    });

    it("does nothing when busy (running)", () => {
      const store = useFlashStore();
      store.flashPhase = "running";
      store.flashProgress = 50;
      store.resetFlash();
      // Should not change
      expect(store.flashPhase).toBe("running");
      expect(store.flashProgress).toBe(50);
    });
  });

  // ── applyErasePreset ───────────────────────────────────────────

  describe("applyErasePreset", () => {
    it("sets erase address range from authInfo preset (Beken chip)", () => {
      const store = useFlashStore();
      store.selectedChipId = "t5ai"; // switch to a Beken chip that has authInfo
      store.applyErasePreset("authInfo");
      expect(store.eraseStartAddr).toBe("0x007CD000");
      expect(store.eraseEndAddr).toBe("0x007FDFFF");
    });

    it("sets erase address range from fullChipNoRf preset (Beken chip)", () => {
      const store = useFlashStore();
      store.selectedChipId = "t5ai";
      store.applyErasePreset("fullChipNoRf");
      expect(store.eraseStartAddr).toBe("0x00000000");
      expect(store.eraseEndAddr).toBe("0x007FDFFF");
    });

    it("sets erase address range from fullChip preset (ESP chip)", () => {
      const store = useFlashStore();
      store.selectedChipId = "esp32"; // explicitly use ESP chip which has fullChip preset
      store.applyErasePreset("fullChip");
      expect(store.eraseStartAddr).toBe("0x00000000");
      expect(store.eraseEndAddr).toBe("0x003FFFFF");
    });

    it("does nothing for missing preset kind", () => {
      const store = useFlashStore();
      store.selectedChipId = "esp32"; // esp32 does not have authInfo preset
      // esp32 does not have authInfo — applyErasePreset should no-op
      store.eraseStartAddr = "0xABCDEF00";
      store.applyErasePreset("authInfo");
      expect(store.eraseStartAddr).toBe("0xABCDEF00"); // unchanged
    });

    it("does nothing when busy", () => {
      const store = useFlashStore();
      store.flashPhase = "running";
      store.eraseStartAddr = "0x00000000";
      store.applyErasePreset("authInfo");
      // Should NOT change
      expect(store.eraseStartAddr).toBe("0x00000000");
    });
  });

  // ── onReadFileNameInput ────────────────────────────────────────

  describe("onReadFileNameInput", () => {
    it("sets fileName and marks as modified", () => {
      const store = useFlashStore();
      expect(store.readFileNameModified).toBe(false);
      store.onReadFileNameInput("custom.bin");
      expect(store.readFileName).toBe("custom.bin");
      expect(store.readFileNameModified).toBe(true);
    });
  });

  // ── onFileChange ───────────────────────────────────────────────

  describe("onFileChange", () => {
    it("sets firmware path from file input", () => {
      const store = useFlashStore();
      const mockFile = new File(["content"], "firmware.bin", {
        type: "application/octet-stream",
      });
      const input = document.createElement("input");
      input.type = "file";

      // happy-dom may not fully emulate DataTransfer; create a mock FileList
      Object.defineProperty(input, "files", {
        value: [mockFile],
        writable: true,
      });

      store.onFileChange({ target: input } as unknown as Event);
      expect(store.flashSegments[0].firmwarePath).toBe("firmware.bin");
    });

    it("clears firmware path when no file selected", () => {
      const store = useFlashStore();
      store.flashSegments[0].firmwarePath = "old.bin";
      const input = document.createElement("input");
      input.type = "file";

      store.onFileChange({ target: input } as unknown as Event);
      expect(store.flashSegments[0].firmwarePath).toBe("");
    });
  });

  // ── startOperation input validation ────────────────────────────

  describe("startOperation validation", () => {
    it("rejects flash without firmware path", async () => {
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.flashSegments[0].firmwarePath = "";
      await store.startOperation("flash");
      expect(store.flashPhase).toBe("error");
    });

    it("rejects read without readDir in Tauri mode (web mode allows empty readDir)", async () => {
      // In web mode (isTauriRuntime=false), readDir is not required — the server writes to a
      // temp path and returns file_content for browser download.  The validation guard was
      // intentionally relaxed in Task 6 with `&& isTauriRuntime()`.
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.readDir = "";
      await store.startOperation("read");
      // Web mode: validation passes, operation starts (running or completes immediately via mock)
      expect(["running", "success"]).toContain(store.flashPhase);
    });

    it("rejects read without readFileName", async () => {
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.readDir = "/tmp";
      store.readFileName = "";
      await store.startOperation("read");
      expect(store.flashPhase).toBe("error");
    });

    it("rejects any operation without serial port", async () => {
      const store = useFlashStore();
      store.selectedSerialPort = "";
      store.flashSegments[0].firmwarePath = "/fw.bin";
      await store.startOperation("flash");
      expect(store.flashPhase).toBe("error");
    });

    it("authorize rejects when only uuid is filled (key missing)", async () => {
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.authorizeUuid = "uuid-test-12345678901234";
      store.authorizeAuthKey = ""; // missing — should be rejected
      await store.startOperation("authorize");
      expect(store.flashPhase).toBe("error");
    });

    it("rejects flash with invalid address range", async () => {
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.flashSegments[0].firmwarePath = "/fw.bin";
      store.flashSegments[0].startAddr = "0x2000";
      store.flashSegments[0].endAddr = "0x1000"; // end < start
      await store.startOperation("flash");
      expect(store.flashPhase).toBe("error");
    });

    it("rejects erase with invalid address range", async () => {
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.eraseStartAddr = "0xZZZZ"; // invalid hex
      store.eraseEndAddr = "0x1000";

      // erase also needs confirm dialog — but validation happens first
      await store.startOperation("erase");
      expect(store.flashPhase).toBe("error");
    });

    it("does nothing when already running", async () => {
      const store = useFlashStore();
      store.flashPhase = "running";
      store.flashMessage = "";
      await store.startOperation("flash");
      // Should stay running, no error set
      expect(store.flashPhase).toBe("running");
      expect(store.flashMessage).toBe("");
    });
  });

  // ── progressCaption ────────────────────────────────────────────

  describe("progressCaption", () => {
    it("shows default when not running", () => {
      const store = useFlashStore();
      expect(store.progressCaption).toBeTruthy();
    });

    it("shows operation-specific caption when running", () => {
      const store = useFlashStore();
      const defaultCaption = store.progressCaption;
      store.flashPhase = "running";
      store.runningOp = "flash";
      const runningCaption = store.progressCaption;
      expect(runningCaption).not.toBe(defaultCaption);
    });
  });

  // ── selectedChipId watch: sync state on chip change ──────────────

  describe("selectedChipId watch (chip-change side effects)", () => {
    it("updates readFileName, readEndAddr and selectedBaudRate per the new chip manifest", async () => {
      const store = useFlashStore();
      // Default is t5ai (baud 921600, flashSize 8 MiB)
      expect(store.selectedChipId).toBe("t5ai");
      expect(store.readFileName).toBe("tyutool_read_t5ai.bin");

      // Switch to ln882h: baud 115200, flashSize 2 MiB
      store.selectedChipId = "ln882h";
      await nextTick();

      expect(store.readFileName).toBe("tyutool_read_ln882h.bin");
      expect(store.readEndAddr).toBe("0x00200000");
      expect(store.selectedBaudRate).toBe(115200);
    });

    it("does NOT overwrite readFileName once the user has modified it", async () => {
      const store = useFlashStore();
      store.onReadFileNameInput("my-custom.bin");
      expect(store.readFileNameModified).toBe(true);

      store.selectedChipId = "esp32";
      await nextTick();

      // User-chosen name is preserved; addr/baud still sync
      expect(store.readFileName).toBe("my-custom.bin");
      expect(store.readEndAddr).toBe("0x00400000"); // esp32 flashSize 4 MiB
      expect(store.selectedBaudRate).toBe(460800); // esp32 default baud
    });

    it("appends a chip-changed log line when chip changes", async () => {
      const store = useFlashStore();
      const before = store.logLines.length;
      store.selectedChipId = "esp32";
      await nextTick();
      expect(store.logLines.length).toBeGreaterThan(before);
    });
  });

  // ── AUTH_ONLY_CHIP_ID ("other") transitions ─────────────────────

  describe('auth-only chip ("other") transitions', () => {
    it("switching to the auth-only chip saves the previous flash chip as lastFlashChipId", async () => {
      const store = useFlashStore();
      store.selectedChipId = "esp32";
      await nextTick();

      store.selectedChipId = "other";
      await nextTick();

      // canFlash / canErase are disabled for the auth-only chip
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.flashSegments[0].firmwarePath = "/fw.bin";
      expect(store.canFlash).toBe(false);
      expect(store.canErase).toBe(false);
      // auth-only auth baud comes from the auth-only manifest
      expect(store.selectedAuthBaudRate).toBe(115200);
    });

    it("leaving the authorize tab restores the last flash chip when 'other' was selected", async () => {
      const store = useFlashStore();
      // Pick a flash chip, then move to authorize tab and select 'other'
      store.selectedChipId = "esp32";
      await nextTick();
      store.activeTab = "authorize";
      await nextTick();
      store.selectedChipId = "other";
      await nextTick();
      expect(store.selectedChipId).toBe("other");

      // Switch away from authorize → chip restored to the last flash-capable chip
      store.activeTab = "flash";
      await nextTick();
      expect(store.selectedChipId).toBe("esp32");
    });

    it("does not change the chip when leaving authorize tab if a real chip is selected", async () => {
      const store = useFlashStore();
      store.selectedChipId = "esp32";
      store.activeTab = "authorize";
      await nextTick();
      store.activeTab = "flash";
      await nextTick();
      expect(store.selectedChipId).toBe("esp32");
    });
  });

  // ── startOperation: refusal / logged error branches ─────────────

  describe("startOperation refusal branches", () => {
    it("validation rejection logs a line and sets flashMessage", async () => {
      const store = useFlashStore();
      store.selectedSerialPort = ""; // missing port → validation fails
      store.flashSegments[0].firmwarePath = "/fw.bin";
      const before = store.logLines.length;
      await store.startOperation("flash");
      expect(store.flashPhase).toBe("error");
      expect(store.flashMessage).not.toBe("");
      expect(store.logLines.length).toBeGreaterThan(before);
    });

    it("erase with invalid segment is refused before any confirm dialog", async () => {
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.eraseStartAddr = "0xZZZZ"; // invalid hex → validation fails
      store.eraseEndAddr = "0x1000";
      await store.startOperation("erase");
      expect(store.flashPhase).toBe("error");
      expect(store.flashMessage).not.toBe("");
    });
  });

  // ── connect / disconnect lifecycle (web mode) ────────────────────

  describe("connect (web mode)", () => {
    it("connects and claims the port via the port manager", async () => {
      const { usePortManagerStore } = await import("@/stores/port-manager");
      const pm = usePortManagerStore();
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      await store.connect();
      expect(store.connected).toBe(true);
      expect(pm.currentOwner("/dev/ttyUSB0")).toBe("flash");
    });

    it("does nothing without a selected port", async () => {
      const store = useFlashStore();
      store.selectedSerialPort = "";
      await store.connect();
      expect(store.connected).toBe(false);
    });

    it("fails to connect when the port is already owned and unyielding", async () => {
      const { usePortManagerStore } = await import("@/stores/port-manager");
      const pm = usePortManagerStore();
      await pm.acquire({
        id: "serial-debug",
        port: "/dev/ttyUSB0",
        onReleaseRequest: async () => false,
        onReleased: () => {},
      });
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      await store.connect();
      expect(store.connected).toBe(false);
      expect(pm.currentOwner("/dev/ttyUSB0")).toBe("serial-debug");
    });

    it("disconnect releases the claimed port", async () => {
      const { usePortManagerStore } = await import("@/stores/port-manager");
      const pm = usePortManagerStore();
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      await store.connect();
      expect(pm.currentOwner("/dev/ttyUSB0")).toBe("flash");
      store.disconnect();
      expect(pm.currentOwner("/dev/ttyUSB0")).toBeNull();
    });
  });

  // ── deviceReset (web mode) ──────────────────────────────────────

  describe("deviceReset (web mode)", () => {
    it("delegates to wsTransport.deviceReset and logs success", async () => {
      const spy = vi
        .spyOn(wsTransport, "deviceReset")
        .mockResolvedValue(undefined);
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.selectedChipId = "t5ai";
      const before = store.logLines.length;
      await store.deviceReset();
      expect(spy).toHaveBeenCalledWith("/dev/ttyUSB0", "T5AI");
      expect(store.logLines.length).toBeGreaterThan(before);
    });

    it("does nothing without a port", async () => {
      const spy = vi
        .spyOn(wsTransport, "deviceReset")
        .mockResolvedValue(undefined);
      const store = useFlashStore();
      store.selectedSerialPort = "";
      await store.deviceReset();
      expect(spy).not.toHaveBeenCalled();
    });

    it("logs a failure line when the reset rejects", async () => {
      vi.spyOn(wsTransport, "deviceReset").mockRejectedValue(new Error("boom"));
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      const before = store.logLines.length;
      await store.deviceReset();
      expect(store.logLines.length).toBeGreaterThan(before);
    });
  });

  // ── startOperation happy path (web mode) ─────────────────────────

  describe("startOperation success path (web mode)", () => {
    afterEach(() => vi.restoreAllMocks());

    function completeImmediately() {
      vi.mocked(wsTransport.runJob).mockImplementationOnce(
        async (_job, _files, onProgress) => {
          onProgress({ payload: { kind: "percent", value: 100 } });
          onProgress({
            payload: { kind: "done", result: { ok: { elapsed_secs: 1.5 } } },
          } as never);
        },
      );
    }

    it("auto-connects, runs a flash job and logs the duration on done", async () => {
      completeImmediately();
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.flashSegments[0].firmwarePath = "/fw.bin";
      store.flashSegments[0].startAddr = "0x0";
      store.flashSegments[0].endAddr = "0x1000";
      expect(store.connected).toBe(false);
      await store.startOperation("flash");
      // connect() ran during the operation, runJob received the job
      expect(wsTransport.runJob).toHaveBeenCalledTimes(1);
      const job = vi.mocked(wsTransport.runJob).mock.calls[0][0] as {
        mode: string;
        chipId: string;
      };
      expect(job.mode).toBe("flash");
    });

    it("runs a read job (web mode skips readDir, server uses temp path)", async () => {
      completeImmediately();
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.readDir = "";
      await store.startOperation("read");
      expect(wsTransport.runJob).toHaveBeenCalledTimes(1);
      const job = vi.mocked(wsTransport.runJob).mock.calls[0][0] as {
        mode: string;
        readFilePath: string | null;
      };
      expect(job.mode).toBe("read");
      expect(job.readFilePath).toBeNull();
    });

    it("sets error phase and logs when the ws job rejects", async () => {
      vi.mocked(wsTransport.runJob).mockRejectedValueOnce(
        new Error("ws kaboom"),
      );
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.flashSegments[0].firmwarePath = "/fw.bin";
      const before = store.logLines.length;
      await store.startOperation("flash");
      expect(store.flashPhase).toBe("error");
      expect(store.flashMessage).toBe("ws kaboom");
      expect(store.runningOp).toBeNull();
      expect(store.logLines.length).toBeGreaterThan(before);
    });

    it("aborts when auto-connect fails because the port is busy", async () => {
      const { usePortManagerStore } = await import("@/stores/port-manager");
      const pm = usePortManagerStore();
      await pm.acquire({
        id: "serial-debug",
        port: "/dev/ttyUSB0",
        onReleaseRequest: async () => false,
        onReleased: () => {},
      });
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.flashSegments[0].firmwarePath = "/fw.bin";
      await store.startOperation("flash");
      expect(store.flashPhase).toBe("error");
      expect(store.runningOp).toBeNull();
      expect(wsTransport.runJob).not.toHaveBeenCalled();
    });

    it("waits for serial-debug to finish closing after release confirmation before starting flash", async () => {
      const { usePortManagerStore } = await import("@/stores/port-manager");
      const pm = usePortManagerStore();
      const serialTransport = makeSerialDebugTransportWithDeferredClose();
      __setSerialDebugTransportForTest(serialTransport.transport);

      const debugStore = useSerialDebugStore();
      debugStore.port = "/dev/ttyUSB0";
      debugStore.baudRate = 115200;
      debugStore.autoRelease = false;
      await debugStore.openPort();
      expect(debugStore.open).toBe(true);
      expect(pm.currentOwner("/dev/ttyUSB0")).toBe("serial-debug");

      let emitDone: (elapsedSecs?: number) => void = () => {
        throw new Error("emitDone not initialized");
      };
      vi.mocked(wsTransport.runJob).mockImplementationOnce(
        async (_job, _files, onProgress) =>
          await new Promise<void>((resolve) => {
            emitDone = (elapsedSecs = 1) => {
              onProgress({
                payload: {
                  kind: "done",
                  result: { ok: { elapsed_secs: elapsedSecs } },
                },
              } as never);
              resolve();
            };
          }),
      );

      const flashStore = useFlashStore();
      flashStore.selectedSerialPort = "/dev/ttyUSB0";
      flashStore.flashSegments[0].firmwarePath = "/fw.bin";
      flashStore.flashSegments[0].startAddr = "0x0";
      flashStore.flashSegments[0].endAddr = "0x1000";

      const opPromise = flashStore.startOperation("flash");
      await Promise.resolve();
      await nextTick();

      expect(pm.currentOwner("/dev/ttyUSB0")).toBe("serial-debug");
      expect(debugStore.open).toBe(false);
      expect(debugStore.opening).toBe(true);
      expect(wsTransport.runJob).not.toHaveBeenCalled();

      serialTransport.finishClose();
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
      await nextTick();

      expect(wsTransport.runJob).toHaveBeenCalledTimes(1);
      expect(pm.currentOwner("/dev/ttyUSB0")).toBe("flash");

      emitDone();
      await opPromise;
      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(flashStore.flashPhase).toBe("success");
      expect(debugStore.open).toBe(true);
      expect(debugStore.pendingResume).toBe(false);
      expect(pm.currentOwner("/dev/ttyUSB0")).toBe("serial-debug");
    });
  });

  // ── startOperation erase confirm dialog ──────────────────────────

  describe("startOperation erase confirmation", () => {
    afterEach(() => vi.restoreAllMocks());

    it("aborts (idle) when the user cancels the erase confirmation", async () => {
      vi.mocked(showConfirmDialog).mockResolvedValueOnce(false);
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.connected = true; // skip auto-connect
      store.eraseStartAddr = "0x0";
      store.eraseEndAddr = "0x1000";
      await store.startOperation("erase");
      expect(showConfirmDialog).toHaveBeenCalled();
      expect(store.flashPhase).not.toBe("running");
      expect(wsTransport.runJob).not.toHaveBeenCalled();
    });

    it("proceeds to run the erase job when confirmed", async () => {
      vi.mocked(showConfirmDialog).mockResolvedValueOnce(true);
      vi.mocked(wsTransport.runJob).mockImplementationOnce(
        async (_job, _files, onProgress) => {
          onProgress({
            payload: { kind: "done", result: { ok: { elapsed_secs: 1.0 } } },
          } as never);
        },
      );
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.connected = true;
      store.eraseStartAddr = "0x0";
      store.eraseEndAddr = "0x1000";
      await store.startOperation("erase");
      expect(wsTransport.runJob).toHaveBeenCalledTimes(1);
      const job = vi.mocked(wsTransport.runJob).mock.calls[0][0] as {
        mode: string;
      };
      expect(job.mode).toBe("erase");
    });
  });

  // ── startOperation authorize probe (web mode) ────────────────────

  describe("auth_conflict milestone handling (web mode)", () => {
    afterEach(() => vi.restoreAllMocks());

    it("sends authorizeConfirm(true) when user confirms overwrite", async () => {
      vi.mocked(showConfirmDialog).mockResolvedValueOnce(true);
      vi.mocked(wsTransport.runJob).mockImplementationOnce(
        async (_job, _files, onProgress) => {
          onProgress({
            payload: {
              kind: "milestone",
              milestone: {
                auth_conflict: {
                  existing_uuid: OTHER_UUID,
                  existing_authkey: OTHER_KEY,
                },
              },
            },
          } as never);
          onProgress({
            payload: { kind: "done", result: { ok: { elapsed_secs: 1.0 } } },
          } as never);
        },
      );
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.connected = true;
      store.authorizeUuid = VALID_UUID;
      store.authorizeAuthKey = VALID_KEY;
      await store.startOperation("authorize");
      await nextTick();
      expect(showConfirmDialog).toHaveBeenCalled();
      expect(wsTransport.authorizeConfirm).toHaveBeenCalledWith(true);
    });

    it("sends authorizeConfirm(false) when user declines overwrite", async () => {
      vi.mocked(showConfirmDialog).mockResolvedValueOnce(false);
      vi.mocked(wsTransport.runJob).mockImplementationOnce(
        async (_job, _files, onProgress) => {
          onProgress({
            payload: {
              kind: "milestone",
              milestone: {
                auth_conflict: {
                  existing_uuid: OTHER_UUID,
                  existing_authkey: OTHER_KEY,
                },
              },
            },
          } as never);
          onProgress({
            payload: { kind: "done", result: { ok: { elapsed_secs: 1.0 } } },
          } as never);
        },
      );
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.connected = true;
      store.authorizeUuid = VALID_UUID;
      store.authorizeAuthKey = VALID_KEY;
      await store.startOperation("authorize");
      await nextTick();
      expect(showConfirmDialog).toHaveBeenCalled();
      expect(wsTransport.authorizeConfirm).toHaveBeenCalledWith(false);
    });
  });

  // ── startAuthRead ────────────────────────────────────────────────

  describe("startAuthRead", () => {
    afterEach(() => vi.restoreAllMocks());

    it("runs an authorize job in read mode and restores credentials afterward", async () => {
      vi.mocked(wsTransport.runJob).mockImplementationOnce(
        async (_job, _files, onProgress) => {
          onProgress({
            payload: { kind: "done", result: { ok: { elapsed_secs: 1.0 } } },
          } as never);
        },
      );
      const store = useFlashStore();
      store.selectedSerialPort = "/dev/ttyUSB0";
      store.connected = true;
      store.authorizeUuid = VALID_UUID;
      store.authorizeAuthKey = VALID_KEY;
      await store.startAuthRead();
      // read mode never triggers auth_conflict; job ran with no credentials
      expect(wsTransport.runJob).toHaveBeenCalledTimes(1);
      const job = vi.mocked(wsTransport.runJob).mock.calls[0][0] as {
        mode: string;
        authorizeUuid: string | null;
      };
      expect(job.mode).toBe("authorize");
      expect(job.authorizeUuid).toBeNull();
      // original form values are restored
      expect(store.authorizeUuid).toBe(VALID_UUID);
      expect(store.authorizeAuthKey).toBe(VALID_KEY);
    });
  });

  // ── stopFlash / cancelBackendFlash (web mode) ────────────────────

  describe("stopFlash (web mode)", () => {
    afterEach(() => vi.restoreAllMocks());

    it("resetFlash on a non-running state cancels the backend job via wsTransport", () => {
      const store = useFlashStore();
      store.flashPhase = "error";
      store.resetFlash();
      expect(wsTransport.cancelJob).toHaveBeenCalled();
    });
  });

  // ── cleanup ──────────────────────────────────────────────────────

  describe("cleanup", () => {
    it("can be called safely with no active listeners or timers", () => {
      const store = useFlashStore();
      expect(() => store.cleanup()).not.toThrow();
    });
  });

  // ── onPickReadDir (web mode) ─────────────────────────────────────

  describe("onPickReadDir (web mode)", () => {
    it("logs the browser no-dir hint instead of opening a dialog", async () => {
      const store = useFlashStore();
      const before = store.logLines.length;
      await store.onPickReadDir();
      expect(store.logLines.length).toBe(before + 1);
    });
  });

  // ── workspace persistence ────────────────────────────────────────

  describe("workspace persistence", () => {
    afterEach(() => vi.restoreAllMocks());

    it("loadWorkspace is a no-op when storage has no data", async () => {
      vi.mocked(loadFlashWorkspaceFromStorage).mockResolvedValueOnce(null);
      const store = useFlashStore();
      await store.loadWorkspace();
      // defaults unchanged
      expect(store.selectedChipId).toBe("t5ai");
    });

    it("loadWorkspace restores serialized fields without re-triggering chip side effects", async () => {
      vi.mocked(loadFlashWorkspaceFromStorage).mockResolvedValueOnce({
        v: 1,
        activeTab: "erase",
        selectedSerialPort: "/dev/ttyUSB9",
        selectedBaudRate: 230400,
        selectedChipId: "esp32",
        flashSegments: [
          {
            id: "seg1",
            firmwarePath: "/saved/fw.bin",
            startAddr: "0x1000",
            endAddr: "0x2000",
          },
        ],
        activeSegmentIndex: 0,
        eraseAdvancedOpen: true,
        eraseStartAddr: "0x3000",
        eraseEndAddr: "0x4000",
        readStartAddr: "0x5000",
        readEndAddr: "0x6000",
        readDir: "/saved/dir",
        readFileName: "saved.bin",
        readFileNameModified: true,
        authorizeUuid: "saved-uuid",
        authorizeAuthKey: "saved-key",
        authBaudRate: 460800,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
      } as any);
      const store = useFlashStore();
      await store.loadWorkspace();
      expect(store.selectedChipId).toBe("esp32");
      expect(store.activeTab).toBe("erase");
      expect(store.selectedSerialPort).toBe("/dev/ttyUSB9");
      expect(store.flashSegments[0].firmwarePath).toBe("/saved/fw.bin");
      expect(store.flashSegments[0].startAddr).toBe("0x1000");
      expect(store.eraseStartAddr).toBe("0x3000");
      expect(store.eraseEndAddr).toBe("0x4000");
      expect(store.readStartAddr).toBe("0x5000");
      expect(store.readDir).toBe("/saved/dir");
      expect(store.eraseAdvancedOpen).toBe(true);
      // readFileNameModified was restored as true, so the chip watch (which
      // flushes after the muted restore) leaves the saved name untouched.
      expect(store.readFileName).toBe("saved.bin");
    });

    it("startWorkspacePersistence saves a debounced snapshot when state changes", async () => {
      vi.useFakeTimers();
      const store = useFlashStore();
      store.startWorkspacePersistence();
      store.selectedBaudRate = 12345;
      await nextTick();
      await vi.advanceTimersByTimeAsync(500);
      expect(saveFlashWorkspaceToStorage).toHaveBeenCalled();
      const snap = vi.mocked(saveFlashWorkspaceToStorage).mock.calls[0][0];
      expect(snap.selectedBaudRate).toBe(12345);
      vi.useRealTimers();
    });

    it("startWorkspacePersistence is idempotent (second call adds no watcher)", async () => {
      vi.useFakeTimers();
      const store = useFlashStore();
      store.startWorkspacePersistence();
      store.startWorkspacePersistence(); // guarded by workspacePersistStarted
      store.eraseAdvancedOpen = true;
      await nextTick();
      await vi.advanceTimersByTimeAsync(500);
      // A single watcher → exactly one save for one change.
      expect(saveFlashWorkspaceToStorage).toHaveBeenCalledTimes(1);
      vi.useRealTimers();
    });
  });
});
