// src/stores/batch-flash-auth.test.ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useBatchFlashAuthStore } from "./batch-flash-auth";
import type { BatchSlotState } from "@/features/batch-flash-auth/types";

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
    expect(store.completionBanner?.kind).toBe("all-success");
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

const slot = (
  port: string,
  status: BatchSlotState["status"],
): BatchSlotState => ({
  port,
  status,
  progress: 0,
  currentPhase: "",
});

describe("checkBatchCompletion banner", () => {
  beforeEach(() => setActivePinia(createPinia()));

  function runWith(slots: BatchSlotState[]) {
    const store = useBatchFlashAuthStore();
    store.slots = slots;
    store.batchStartTime = 1; // non-null so checkBatchCompletion proceeds
    store.currentBatchPorts = slots.map((s) => s.port);
    store.checkBatchCompletion();
    return store.completionBanner;
  }

  it("all-success when every device done", () => {
    expect(runWith([slot("a", "done"), slot("b", "done")])).toEqual({
      kind: "all-success",
      count: 2,
    });
  });

  it("all-failed when every device failed", () => {
    expect(runWith([slot("a", "failed")])).toEqual({ kind: "all-failed" });
  });

  it("all-skipped when only skipped", () => {
    expect(runWith([slot("a", "skipped"), slot("b", "skipped")])).toEqual({
      kind: "all-skipped",
      count: 2,
    });
  });

  it("partial when mixed done and failed", () => {
    expect(runWith([slot("a", "done"), slot("b", "failed")])).toEqual({
      kind: "partial",
      done: 1,
      failed: 1,
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

describe("handleAuthProgress", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("reading_mac step transitions to reading_mac status", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.handleAuthProgress({ port: "COM3", step: "reading_mac" });
    expect(store.slots[0].status).toBe("reading_mac");
    expect(store.slots[0].currentPhase).toBe("reading_mac");
  });

  it("reading_auth step transitions to authorizing with phase", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.handleAuthProgress({ port: "COM3", step: "reading_auth" });
    expect(store.slots[0].status).toBe("authorizing");
    expect(store.slots[0].currentPhase).toBe("reading_auth");
  });

  it("writing_auth step transitions to authorizing with phase", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.handleAuthProgress({ port: "COM3", step: "writing_auth" });
    expect(store.slots[0].status).toBe("authorizing");
    expect(store.slots[0].currentPhase).toBe("writing_auth");
  });

  it("verifying step transitions to authorizing with phase", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.handleAuthProgress({ port: "COM3", step: "verifying" });
    expect(store.slots[0].status).toBe("authorizing");
    expect(store.slots[0].currentPhase).toBe("verifying");
  });

  it("done step: status=done, mac saved, auth cumulative incremented", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "done",
      mac: "aabbccddeeff",
    });
    expect(store.slots[0].status).toBe("done");
    expect(store.slots[0].mac).toBe("aabbccddeeff");
    expect(store.slots[0].progress).toBe(100);
    expect(store.cumulativeStats.auth.total).toBe(1);
    expect(store.cumulativeStats.auth.success).toBe(1);
    expect(store.cumulativeStats.auth.fail).toBe(0);
  });

  it("done step: flash cumulative NOT incremented (only auth counter goes up)", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "done",
      mac: "aabbccddeeff",
    });
    expect(store.cumulativeStats.flash.total).toBe(0);
  });

  it("failed step: status=failed, error saved, auth fail incremented", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "failed",
      error: "verify mismatch",
    });
    expect(store.slots[0].status).toBe("failed");
    expect(store.slots[0].error).toBe("verify mismatch");
    expect(store.cumulativeStats.auth.total).toBe(1);
    expect(store.cumulativeStats.auth.fail).toBe(1);
    expect(store.cumulativeStats.auth.success).toBe(0);
  });

  it("failed step with no error message falls back to default", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.batchStartTime = Date.now();
    store.handleAuthProgress({ port: "COM3", step: "failed" });
    expect(store.slots[0].error).toBe("Unknown auth error");
  });

  it("skipped step: status=skipped, mac saved, auth cumulative NOT incremented", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "skipped",
      mac: "aabbccddeeff",
    });
    expect(store.slots[0].status).toBe("skipped");
    expect(store.slots[0].mac).toBe("aabbccddeeff");
    expect(store.slots[0].currentPhase).toBe("");
    // skipped does NOT count in auth cumulative
    expect(store.cumulativeStats.auth.total).toBe(0);
  });

  it("skipped step triggers completion banner when all ports complete", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.batchStartTime = Date.now();
    store.currentBatchPorts = ["COM3"];
    store.handleAuthProgress({
      port: "COM3",
      step: "skipped",
      mac: "aabbccddeeff",
    });
    expect(store.completionBanner?.kind).toBe("all-skipped");
  });

  it("flashing sub-step percent: updates slot progress", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "flashing";
    store.handleAuthProgress({
      port: "COM3",
      step: "flashing",
      event: { kind: "percent", value: 55 },
    });
    expect(store.slots[0].progress).toBe(55);
  });

  it("flashing sub-step done/ok: transitions to reading_mac for auth phase", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "flashing";
    store.handleAuthProgress({
      port: "COM3",
      step: "flashing",
      event: { kind: "done", result: { ok: { elapsed_secs: 10 } } },
    });
    expect(store.slots[0].status).toBe("reading_mac");
    expect(store.slots[0].progress).toBe(0);
    expect(store.slots[0].currentPhase).toBe("reading_mac");
  });

  it("ignores events for unknown ports", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    // should not throw
    store.handleAuthProgress({ port: "COM999", step: "done", mac: "aabb" });
    expect(store.slots[0].status).toBe("idle");
  });

  it("completion banner: all-success when 2 ports both done", () => {
    const store = useBatchFlashAuthStore();
    store.batchStartTime = Date.now();
    store.addPorts(["COM3", "COM5"]);
    store.slots.forEach((s) => (s.status = "authorizing"));
    store.currentBatchPorts = ["COM3", "COM5"];
    store.handleAuthProgress({ port: "COM3", step: "done", mac: "aabb" });
    expect(store.completionBanner).toBeNull(); // COM5 still active
    store.handleAuthProgress({ port: "COM5", step: "done", mac: "ccdd" });
    expect(store.completionBanner?.kind).toBe("all-success");
  });

  it("completion banner: partial when 1 done + 1 failed", () => {
    const store = useBatchFlashAuthStore();
    store.batchStartTime = Date.now();
    store.addPorts(["COM3", "COM5"]);
    store.slots.forEach((s) => (s.status = "authorizing"));
    store.currentBatchPorts = ["COM3", "COM5"];
    store.handleAuthProgress({ port: "COM3", step: "done", mac: "aabb" });
    store.handleAuthProgress({
      port: "COM5",
      step: "failed",
      error: "timeout",
    });
    expect(store.completionBanner?.kind).toBe("partial");
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
