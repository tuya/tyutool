// src/stores/batch-flash-auth.test.ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useBatchFlashAuthStore } from "./batch-flash-auth";

vi.mock("@/runtime", () => ({
  isTauriRuntime: () => false,
}));

describe("opMode", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("is auth-only by default (chip=esp32, no firmware, no excel)", () => {
    const store = useBatchFlashAuthStore();
    expect(store.opMode).toBe("auth-only");
  });

  it("is auth-only when chip=other regardless of firmware and excel", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "other";
    store.firmwarePath = "/fw.bin";
    store.authConfig.excelPath = "/auth.xlsx";
    expect(store.opMode).toBe("auth-only");
  });

  it("is auth-only when excel selected and chip supports auth, no firmware", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "esp32";
    store.authConfig.excelPath = "/path/to/auth.xlsx";
    expect(store.opMode).toBe("auth-only");
  });

  it("is flash-then-auth when both firmware and excel are set with supported chip", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "esp32";
    store.firmwarePath = "/path/to/fw.bin";
    store.authConfig.excelPath = "/path/to/auth.xlsx";
    expect(store.opMode).toBe("flash-then-auth");
  });
});

describe("canFlash", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("is true for esp32", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "esp32";
    expect(store.canFlash).toBe(true);
  });

  it("is false for other", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "other";
    expect(store.canFlash).toBe(false);
  });
});

describe("inputsValid", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("is false with no inputs", () => {
    const store = useBatchFlashAuthStore();
    expect(store.inputsValid).toBe(false);
  });

  it("is false when firmware set but no excel", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "esp32";
    store.firmwarePath = "/fw.bin";
    expect(store.inputsValid).toBe(false);
  });

  it("is true when excel set, no firmware", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "esp32";
    store.authConfig.excelPath = "/auth.xlsx";
    expect(store.inputsValid).toBe(true);
  });

  it("is true when chip=other with excel (firmware ignored)", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "other";
    store.firmwarePath = "/fw.bin";
    store.authConfig.excelPath = "/auth.xlsx";
    expect(store.inputsValid).toBe(true);
  });
});

describe("slot state machine", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("addPorts creates idle slots", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    expect(store.slots).toHaveLength(2);
    expect(store.slots[0]).toMatchObject({
      port: "COM3",
      status: "idle",
      progress: 0,
    });
  });

  it("addPorts deduplicates existing ports", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.addPorts(["COM3", "COM5"]);
    expect(store.slots).toHaveLength(2);
  });

  it("removeSlot removes idle slots", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.removeSlot("COM3");
    expect(store.slots).toHaveLength(0);
  });

  it("removeSlot does not remove flashing slots", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "flashing";
    store.removeSlot("COM3");
    expect(store.slots).toHaveLength(1);
  });

  it("handleFlashProgress percent updates slot progress", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "flashing";
    store.handleFlashProgress({
      port: "COM3",
      event: { kind: "percent", value: 68 },
    });
    expect(store.slots[0].progress).toBe(68);
  });

  it("handleFlashProgress done/ok transitions slot to done and increments cumulative", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "flashing";
    store.batchStartTime = Date.now();
    store.handleFlashProgress({
      port: "COM3",
      event: { kind: "done", result: { ok: { elapsed_secs: 10 } } },
    });
    expect(store.slots[0].status).toBe("done");
    expect(store.cumulativeStats.flash.total).toBe(1);
    expect(store.cumulativeStats.flash.success).toBe(1);
    expect(store.cumulativeStats.flash.fail).toBe(0);
  });

  it("handleFlashProgress done/err transitions slot to failed and increments cumulative", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "flashing";
    store.batchStartTime = Date.now();
    store.handleFlashProgress({
      port: "COM3",
      event: {
        kind: "done",
        result: { err: { message: "timeout", elapsed_secs: 5 } },
      },
    });
    expect(store.slots[0].status).toBe("failed");
    expect(store.slots[0].error).toBe("timeout");
    expect(store.cumulativeStats.flash.total).toBe(1);
    expect(store.cumulativeStats.flash.fail).toBe(1);
  });

  it("handleFlashProgress done/cancelled resets slot to idle without incrementing cumulative", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "flashing";
    store.handleFlashProgress({
      port: "COM3",
      event: { kind: "done", result: { cancelled: { elapsed_secs: 2 } } },
    });
    expect(store.slots[0].status).toBe("idle");
    expect(store.cumulativeStats.flash.total).toBe(0);
  });
});

describe("canStart / canRetry / canCancel", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("canStart is false when no slots", () => {
    const store = useBatchFlashAuthStore();
    store.firmwarePath = "/fw.bin";
    expect(store.canStart).toBe(false);
  });

  it("canStart is true when idle slot exists and excel is set", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.chipId = "esp32";
    store.authConfig.excelPath = "/auth.xlsx";
    expect(store.canStart).toBe(true);
  });

  it("canRetry is false when no failed slots", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    expect(store.canRetry).toBe(false);
  });

  it("canRetry is true when any slot is failed", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "failed";
    expect(store.canRetry).toBe(true);
  });
});

describe("completionBanner", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("shows success banner when all done", () => {
    const store = useBatchFlashAuthStore();
    store.batchStartTime = Date.now();
    store.addPorts(["COM3", "COM5"]);
    store.slots.forEach((s) => {
      s.status = "flashing";
    });
    store.handleFlashProgress({
      port: "COM3",
      event: { kind: "done", result: { ok: { elapsed_secs: 5 } } },
    });
    store.handleFlashProgress({
      port: "COM5",
      event: { kind: "done", result: { ok: { elapsed_secs: 5 } } },
    });
    expect(store.completionBanner?.kind).toBe("success");
  });

  it("shows partial banner on mixed outcome", () => {
    const store = useBatchFlashAuthStore();
    store.batchStartTime = Date.now();
    store.addPorts(["COM3", "COM5"]);
    store.slots.forEach((s) => {
      s.status = "flashing";
    });
    store.handleFlashProgress({
      port: "COM3",
      event: { kind: "done", result: { ok: { elapsed_secs: 5 } } },
    });
    store.handleFlashProgress({
      port: "COM5",
      event: {
        kind: "done",
        result: { err: { message: "fail", elapsed_secs: 5 } },
      },
    });
    expect(store.completionBanner?.kind).toBe("partial");
  });
});

describe("resetFlashStats", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("resets flash cumulative to zero", () => {
    const store = useBatchFlashAuthStore();
    store.cumulativeStats.flash = { total: 10, success: 8, fail: 2 };
    store.resetFlashStats();
    expect(store.cumulativeStats.flash).toEqual({
      total: 0,
      success: 0,
      fail: 0,
    });
  });
});

describe("web-mode no-op for default firmware", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("loadDefaultFirmwareList is a silent no-op: leaves status idle, entries empty", async () => {
    const store = useBatchFlashAuthStore();
    await store.loadDefaultFirmwareList();
    expect(store.defaultFirmwareStatus).toBe("idle");
    expect(store.defaultFirmwareEntries).toHaveLength(0);
  });

  it("downloadDefaultFirmware is a silent no-op: leaves firmwarePath empty, status idle", async () => {
    const store = useBatchFlashAuthStore();
    await store.downloadDefaultFirmware("1.0.0");
    expect(store.defaultFirmwareStatus).toBe("idle");
    expect(store.defaultFirmwareEntries).toHaveLength(0);
    expect(store.firmwarePath).toBe("");
  });
});

describe("useBatchFlashAuthStore firmware source", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("defaults to local source with empty path", () => {
    const store = useBatchFlashAuthStore();
    expect(store.firmwareSource).toBe("local");
    expect(store.selectedDefaultVersion).toBe("");
  });

  it("switching source clears a previously chosen firmware path", () => {
    const store = useBatchFlashAuthStore();
    store.firmwarePath = "/tmp/local.bin";
    store.setFirmwareSource("default");
    expect(store.firmwareSource).toBe("default");
    expect(store.firmwarePath).toBe("");
  });

  it("switching back to local resets default-firmware status", () => {
    const store = useBatchFlashAuthStore();
    store.setFirmwareSource("default");
    store.setFirmwareSource("local");
    expect(store.firmwareSource).toBe("local");
    expect(store.defaultFirmwareStatus).toBe("idle");
  });
});
