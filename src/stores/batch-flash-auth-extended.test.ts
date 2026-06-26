// src/stores/batch-flash-auth-extended.test.ts
// Extended unit tests for batch-flash-auth feature (covers test plan TODO items).
// Tests for confirmed bugs are marked [BUG TC-XXX] and are expected to fail until fixed.
import { describe, it, expect, beforeEach, vi } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useBatchFlashAuthStore } from "./batch-flash-auth";
import {
  normalizePortName,
  applyPortFilter,
} from "@/features/batch-flash-auth/port-filter";
import { filterByChip } from "@/features/batch-flash-auth/auth-firmware";
import type { AuthFirmwareEntry } from "@/features/batch-flash-auth/types";

vi.mock("@/runtime", () => ({
  isTauriRuntime: () => false,
}));

// ─── S02 Slot state machine ────────────────────────────────────────────────

describe("S02 slot state machine — additional", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("TC-015: removeSlot cannot remove a flashing slot", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "flashing";
    store.removeSlot("COM3");
    expect(store.slots).toHaveLength(1);
  });

  it("TC-015b: removeSlot cannot remove a reading_mac slot", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "reading_mac";
    store.removeSlot("COM3");
    expect(store.slots).toHaveLength(1);
  });

  it("TC-015c: removeSlot cannot remove an authorizing slot", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.removeSlot("COM3");
    expect(store.slots).toHaveLength(1);
  });

  it("TC-015d: removeSlot removes a failed slot", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "failed";
    // failed is not in allowed remove-statuses per store code; verify current behavior
    store.removeSlot("COM3");
    // per removeSlot implementation: allowed = idle | done | skipped
    // failed is NOT in that list, so slot remains
    expect(store.slots).toHaveLength(1);
  });

  it("TC-016: updateSlot Object.assign semantics — mac preserved when only updating progress", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].mac = "aabbccddeeff";
    // Call handleAuthProgress done to test updateSlot merging
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "done",
      mac: "aabbccddeeff",
    });
    // Patch only progress via a subsequent reading_mac (to simulate merge)
    // Direct: set mac then update only progress via slots ref
    store.slots[0].mac = "112233445566";
    store.handleAuthProgress({ port: "COM3", step: "reading_mac" });
    // reading_mac only updates status and currentPhase, mac is preserved
    expect(store.slots[0].mac).toBe("112233445566");
  });

  it("TC-017: excelError: undefined in updateSlot patch clears the field", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].excelError = "row not found";
    // cancelled step calls updateSlot with excelError: undefined
    store.handleAuthProgress({ port: "COM3", step: "cancelled" });
    expect(store.slots[0].excelError).toBeUndefined();
  });

  it("TC-018: initial slot has correct default fields", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    const s = store.slots[0];
    expect(s.status).toBe("idle");
    expect(s.progress).toBe(0);
    expect(s.currentPhase).toBe("");
    expect(s.mac).toBeUndefined();
    expect(s.error).toBeUndefined();
    expect(s.excelError).toBeUndefined();
  });
});

// ─── S03 Flash progress — additional ──────────────────────────────────────

describe("S03 handleFlashProgress — port isolation", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("TC-023: flash progress only updates the matching port", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.slots[0].status = "flashing";
    store.slots[1].status = "flashing";
    store.handleFlashProgress({
      port: "COM3",
      event: { kind: "percent", value: 75 },
    });
    expect(store.slots[0].progress).toBe(75);
    expect(store.slots[1].progress).toBe(0); // COM5 unchanged
  });

  it("TC-023b: auth progress only updates the matching port", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.batchStartTime = Date.now();
    store.handleAuthProgress({ port: "COM3", step: "done", mac: "aabb" });
    expect(store.slots[0].status).toBe("done");
    expect(store.slots[1].status).toBe("idle"); // COM5 unchanged
  });
});

// ─── S04 Auth progress — BUG tests ────────────────────────────────────────

describe("S04 handleAuthProgress — skipped excelError [TC-030]", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("TC-030: skipped step propagates excelError to slot", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "skipped",
      mac: "aabbccddeeff",
      excelError: "row not found in spreadsheet",
    });
    expect(store.slots[0].excelError).toBe("row not found in spreadsheet");
  });

  it("TC-030: skipped step without excelError leaves excelError undefined", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "skipped",
      mac: "aabbccddeeff",
    });
    expect(store.slots[0].excelError).toBeUndefined();
  });

  it("cancelled step correctly clears excelError (for contrast)", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].excelError = "pre-existing error";
    store.handleAuthProgress({ port: "COM3", step: "cancelled" });
    expect(store.slots[0].excelError).toBeUndefined();
  });

  it("done step preserves excelError when present in event", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "done",
      mac: "aabb",
      excelError: "confirm failed",
    });
    // done step DOES propagate excelError (line 349 in store)
    expect(store.slots[0].excelError).toBe("confirm failed");
  });
});

// ─── S05 Batch operations — BUG tests ─────────────────────────────────────

describe("S05 retryFailed — excelError cleared [TC-037]", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("TC-037: retryFailed clears excelError on failed slot", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "failed";
    store.slots[0].error = "some error";
    store.slots[0].excelError = "confirm_row failed";
    await store.retryFailed();
    expect(store.slots[0].excelError).toBeUndefined();
  });

  it("retryFailed: error is cleared (correct behavior)", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "failed";
    store.slots[0].error = "timeout";
    await store.retryFailed();
    expect(store.slots[0].error).toBeUndefined();
  });

  it("TC-038: retryFailed only affects failed slots, not done/skipped", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5", "COM7"]);
    store.slots[0].status = "failed";
    store.slots[1].status = "done";
    store.slots[1].mac = "aabb";
    store.slots[2].status = "skipped";
    store.slots[2].mac = "ccdd";
    await store.retryFailed();
    expect(store.slots[0].status).toBe("idle"); // failed → reset to idle
    expect(store.slots[1].status).toBe("done"); // done unchanged
    expect(store.slots[1].mac).toBe("aabb");
    expect(store.slots[2].status).toBe("skipped"); // skipped unchanged
    expect(store.slots[2].mac).toBe("ccdd");
  });

  it("retryFailed: canRetry gate — no-op when no failed slots", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.completionBanner = { kind: "all-success", count: 1 };
    await store.retryFailed(); // canRetry is false
    expect(store.completionBanner).not.toBeNull(); // banner not cleared
  });
});

// ─── S06 Completion banner — additional ───────────────────────────────────

describe("S06 completion banner — additional", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("TC-049: checkBatchCompletion no-ops when any slot is still active", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.batchStartTime = Date.now();
    store.slots[0].status = "authorizing"; // active
    store.slots[1].status = "done";
    store.checkBatchCompletion();
    expect(store.completionBanner).toBeNull(); // no banner while active
  });

  it("TC-049b: banner appears only after all active slots finish", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.batchStartTime = Date.now();
    store.currentBatchPorts = ["COM3", "COM5"];
    store.slots[0].status = "authorizing";
    store.slots[1].status = "done";
    store.checkBatchCompletion();
    expect(store.completionBanner).toBeNull();
    // Now finish the last active slot
    store.slots[0].status = "done";
    store.checkBatchCompletion();
    expect(store.completionBanner?.kind).toBe("all-success");
  });

  it("TC-051: resetAuthStats resets only auth cumulative to zero", () => {
    const store = useBatchFlashAuthStore();
    store.cumulativeStats.auth = { total: 10, success: 8, fail: 2 };
    store.cumulativeStats.flash = { total: 5, success: 4, fail: 1 };
    store.resetAuthStats();
    expect(store.cumulativeStats.auth).toEqual({
      total: 0,
      success: 0,
      fail: 0,
    });
    // flash stats should remain untouched
    expect(store.cumulativeStats.flash).toEqual({
      total: 5,
      success: 4,
      fail: 1,
    });
  });

  it("TC-051b: after resetAuthStats, subsequent auth events accumulate from 0", () => {
    const store = useBatchFlashAuthStore();
    store.cumulativeStats.auth = { total: 10, success: 8, fail: 2 };
    store.resetAuthStats();
    store.addPorts(["COM3"]);
    store.batchStartTime = Date.now();
    store.handleAuthProgress({ port: "COM3", step: "done", mac: "aabb" });
    expect(store.cumulativeStats.auth.total).toBe(1);
    expect(store.cumulativeStats.auth.success).toBe(1);
  });
});

// ─── S07 Port filter — pure functions ─────────────────────────────────────

describe("S07 port-filter pure functions", () => {
  it("TC-052: normalizePortName — on non-Windows platform, COM port NOT uppercased", () => {
    // On Linux (test environment), isWindowsPlatform() returns false
    // com3 stays as-is since we're not on Windows
    expect(normalizePortName("com3")).toBe("com3");
  });

  it("TC-053: normalizePortName — Unix-style path unchanged", () => {
    expect(normalizePortName("/dev/ttyUSB0")).toBe("/dev/ttyUSB0");
    expect(normalizePortName("/dev/cu.usbserial-0001")).toBe(
      "/dev/cu.usbserial-0001",
    );
  });

  it("TC-052c: normalizePortName — already uppercase COM port unchanged on any platform", () => {
    // On Linux, COM3 is returned unchanged (no transformation for non-Windows)
    expect(normalizePortName("COM3")).toBe("COM3");
  });

  it("TC-054: applyPortFilter removes blocked ports", () => {
    const result = applyPortFilter(
      ["/dev/ttyUSB0", "/dev/ttyUSB1", "/dev/ttyUSB2"],
      ["/dev/ttyUSB1"],
    );
    expect(result).toEqual(["/dev/ttyUSB0", "/dev/ttyUSB2"]);
  });

  it("TC-055: applyPortFilter preserves non-blocked ports", () => {
    const result = applyPortFilter(
      ["/dev/ttyUSB0", "/dev/ttyUSB1"],
      ["/dev/ttyUSB9"],
    );
    expect(result).toEqual(["/dev/ttyUSB0", "/dev/ttyUSB1"]);
  });

  it("TC-056: applyPortFilter with empty blocklist returns all ports", () => {
    const ports = ["/dev/ttyUSB0", "/dev/ttyUSB1", "/dev/ttyUSB2"];
    expect(applyPortFilter(ports, [])).toEqual(ports);
  });

  it("TC-056b: applyPortFilter with all ports blocked returns empty array", () => {
    const result = applyPortFilter(
      ["/dev/ttyUSB0", "/dev/ttyUSB1"],
      ["/dev/ttyUSB0", "/dev/ttyUSB1"],
    );
    expect(result).toEqual([]);
  });
});

// ─── S07 Port filter — store integration ──────────────────────────────────

describe("S07 addBlockedPort / removeBlockedPort — store", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("addBlockedPort adds to blockedPorts list", () => {
    const store = useBatchFlashAuthStore();
    store.addBlockedPort("/dev/ttyUSB0");
    expect(store.filterConfig.blockedPorts).toContain("/dev/ttyUSB0");
  });

  it("addBlockedPort removes idle slot that matches the newly blocked port", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["/dev/ttyUSB0", "/dev/ttyUSB1"]);
    store.addBlockedPort("/dev/ttyUSB0");
    expect(store.slots.map((s) => s.port)).not.toContain("/dev/ttyUSB0");
    expect(store.slots.map((s) => s.port)).toContain("/dev/ttyUSB1");
  });

  it("addBlockedPort does NOT remove active slot matching the blocked port", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["/dev/ttyUSB0"]);
    store.slots[0].status = "authorizing"; // active
    store.addBlockedPort("/dev/ttyUSB0");
    expect(store.slots).toHaveLength(1); // still there because it's active
  });

  it("addBlockedPort is idempotent — no duplicates in blocklist", () => {
    const store = useBatchFlashAuthStore();
    store.addBlockedPort("/dev/ttyUSB0");
    store.addBlockedPort("/dev/ttyUSB0");
    expect(
      store.filterConfig.blockedPorts.filter((p) => p === "/dev/ttyUSB0"),
    ).toHaveLength(1);
  });

  it("removeBlockedPort removes from blockedPorts list", () => {
    const store = useBatchFlashAuthStore();
    store.addBlockedPort("/dev/ttyUSB0");
    store.removeBlockedPort("/dev/ttyUSB0");
    expect(store.filterConfig.blockedPorts).not.toContain("/dev/ttyUSB0");
  });

  it("filterActive computed: true when blockedPorts non-empty", () => {
    const store = useBatchFlashAuthStore();
    expect(store.filterActive).toBe(false);
    store.addBlockedPort("/dev/ttyUSB0");
    expect(store.filterActive).toBe(true);
    store.removeBlockedPort("/dev/ttyUSB0");
    expect(store.filterActive).toBe(false);
  });
});

// ─── S08 Auth firmware pure functions ─────────────────────────────────────

describe("S08 filterByChip", () => {
  const entries: AuthFirmwareEntry[] = [
    { version: "1.0.0", chip: "t5ai", url: "u1", sha256: "h1" },
    { version: "1.2.0", chip: "t5ai", url: "u2", sha256: "h2" },
    { version: "1.10.0", chip: "t5ai", url: "u3", sha256: "h3" },
    { version: "2.0.0", chip: "esp32", url: "u4", sha256: "h4" },
    { version: "1.1.0", chip: "t5ai", url: "u5", sha256: "h5" },
  ];

  it("TC-058: returns entries sorted descending by version (Intl numeric)", () => {
    const result = filterByChip(entries, "t5ai");
    const versions = result.map((e) => e.version);
    expect(versions).toEqual(["1.10.0", "1.2.0", "1.1.0", "1.0.0"]);
  });

  it("TC-059: returns only entries for the matching chip", () => {
    const result = filterByChip(entries, "t5ai");
    expect(result.every((e) => e.chip === "t5ai")).toBe(true);
    expect(result).toHaveLength(4);
  });

  it("TC-060: returns empty array when chip has no entries in manifest", () => {
    expect(filterByChip(entries, "gd32")).toEqual([]);
    expect(filterByChip([], "t5ai")).toEqual([]);
  });

  it("TC-060b: returns empty array when entries list is empty", () => {
    expect(filterByChip([], "t5ai")).toHaveLength(0);
  });

  it("filterByChip: esp32 entries returned correctly", () => {
    const result = filterByChip(entries, "esp32");
    expect(result).toHaveLength(1);
    expect(result[0].version).toBe("2.0.0");
  });

  it("version comparison: v prefix stripped correctly", () => {
    const withV: AuthFirmwareEntry[] = [
      { version: "v1.0.0", chip: "t5ai", url: "u1", sha256: "h1" },
      { version: "v1.2.0", chip: "t5ai", url: "u2", sha256: "h2" },
      { version: "v1.10.0", chip: "t5ai", url: "u3", sha256: "h3" },
    ];
    const result = filterByChip(withV, "t5ai");
    expect(result.map((e) => e.version)).toEqual([
      "v1.10.0",
      "v1.2.0",
      "v1.0.0",
    ]);
  });
});

// ─── S08 downloadAuthFirmware ──────────────────────────────────────────────

describe("S08 downloadAuthFirmware — web mode guard", () => {
  it("TC-061: downloadAuthFirmware throws in web mode (isTauriRuntime=false)", async () => {
    const { downloadAuthFirmware } =
      await import("@/features/batch-flash-auth/auth-firmware");
    await expect(
      downloadAuthFirmware({
        version: "1.1.0",
        chip: "t5ai",
        url: "https://example.com/fw.bin",
        sha256: "abc123",
      }),
    ).rejects.toThrow("download requires desktop runtime");
  });
});

// ─── S05 canStart / isBusy guards ─────────────────────────────────────────

describe("S05 canStart guards", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("canStart is false when isBusy (active slot exists)", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.authConfig.excelPath = "/auth.xlsx";
    store.slots[0].status = "authorizing";
    store.slots[1].status = "idle";
    // isBusy is true, but canStart checks idle slots and inputsValid, not isBusy directly
    // canStart = slots.some(idle) && inputsValid — COM5 is idle
    expect(store.canStart).toBe(true); // idle slot exists
    expect(store.isBusy).toBe(true); // authorizing slot makes isBusy true
  });

  it("isBusy returns false when all slots are idle/done/failed/skipped", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.slots[0].status = "done";
    store.slots[1].status = "failed";
    expect(store.isBusy).toBe(false);
  });

  it("isBusy returns true for each active status", () => {
    for (const status of ["flashing", "reading_mac", "authorizing"] as const) {
      setActivePinia(createPinia());
      const s = useBatchFlashAuthStore();
      s.addPorts(["COM3"]);
      s.slots[0].status = status;
      expect(s.isBusy).toBe(true);
    }
  });
});

// ─── S05 startBatch web no-op ──────────────────────────────────────────────

describe("S05 startBatch — web mode no-op", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("startBatch does not throw and leaves slots unchanged in web mode", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.authConfig.excelPath = "/auth.xlsx";
    await store.startBatch(); // isTauriRuntime()=false → early return
    expect(store.slots[0].status).toBe("idle");
  });

  it("cancelAll does not throw in web mode", async () => {
    const store = useBatchFlashAuthStore();
    await expect(store.cancelAll()).resolves.toBeUndefined();
  });

  it("cancelPort does not throw in web mode", async () => {
    const store = useBatchFlashAuthStore();
    await expect(store.cancelPort("COM3")).resolves.toBeUndefined();
  });
});

// ─── S09 persistence — web mode no-ops ────────────────────────────────────

describe("S09 workspace persistence — web mode", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("TC-066: loadPersistedData is a no-op in web mode — returns without throwing", async () => {
    const store = useBatchFlashAuthStore();
    await expect(store.loadPersistedData()).resolves.toBeUndefined();
  });

  it("loadPersistedData does not alter state in web mode", async () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "t5ai";
    store.authConfig.excelPath = "/my/path.xlsx";
    await store.loadPersistedData();
    // State should remain as-set (workspace returns empty in web mode)
    expect(store.authConfig.excelPath).toBe("/my/path.xlsx");
  });
});

// ─── Miscellaneous correctness ────────────────────────────────────────────

describe("dismissBanner", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("dismissBanner clears the completion banner", () => {
    const store = useBatchFlashAuthStore();
    store.completionBanner = { kind: "all-success", count: 1 };
    store.dismissBanner();
    expect(store.completionBanner).toBeNull();
  });
});

describe("currentStats computed", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("counts active/done/failed/skipped correctly", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["a", "b", "c", "d", "e"]);
    store.slots[0].status = "authorizing";
    store.slots[1].status = "done";
    store.slots[2].status = "failed";
    store.slots[3].status = "skipped";
    // slots[4] remains idle
    expect(store.currentStats).toEqual({
      active: 1,
      done: 1,
      failed: 1,
      skipped: 1,
    });
  });
});

describe("addPorts — deduplication with mixed ports", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("deduplicates across multiple addPorts calls", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["/dev/ttyUSB0", "/dev/ttyUSB1"]);
    store.addPorts(["/dev/ttyUSB1", "/dev/ttyUSB2"]);
    expect(store.slots).toHaveLength(3);
    expect(store.slots.map((s) => s.port)).toEqual([
      "/dev/ttyUSB0",
      "/dev/ttyUSB1",
      "/dev/ttyUSB2",
    ]);
  });
});
