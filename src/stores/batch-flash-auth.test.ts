// @vitest-environment happy-dom
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

  it("is auth-only when firmware flashing is disabled even with a firmware selected", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "esp32";
    store.flashFirmware = false;
    store.firmwarePath = "/path/to/fw.bin";
    store.authConfig.excelPath = "/path/to/auth.xlsx";
    expect(store.opMode).toBe("auth-only");
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

  it("canStart is true when idle slot exists, excel is set and stats show remaining codes", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.chipId = "esp32";
    store.authConfig.excelPath = "/auth.xlsx";
    store.excelStats = { total: 10, used: 0, inProgress: 0, remaining: 10 };
    expect(store.canStart).toBe(true);
  });

  it("canStart is false when excel path is set but stats are unknown (validation pending)", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.authConfig.excelPath = "/auth.xlsx";
    // excelStats stays null until validateExcel resolves
    expect(store.canStart).toBe(false);
  });

  it("canStart is false when excel validation errored", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.authConfig.excelPath = "/bad.xlsx";
    store.excelError = "file not found";
    expect(store.canStart).toBe(false);
  });

  it("canStart is false when excel codes are exhausted", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.authConfig.excelPath = "/auth.xlsx";
    store.excelStats = { total: 10, used: 10, inProgress: 0, remaining: 0 };
    expect(store.canStart).toBe(false);
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
      uuid: "550e8400-e29b-41d4-a716-446655440000",
    });
    expect(store.slots[0].status).toBe("done");
    expect(store.slots[0].mac).toBe("aabbccddeeff");
    expect(store.slots[0].authUuid).toBe(
      "550e8400-e29b-41d4-a716-446655440000",
    );
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

  it("handleAuthProgress sets slot status/error/mac for failed step", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "failed",
      error: "otp lock failed",
      mac: "aabbccddeeff",
    });
    expect(store.slots[0].status).toBe("failed");
    expect(store.slots[0].error).toBe("otp lock failed");
    expect(store.slots[0].mac).toBe("aabbccddeeff");
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
      uuid: "550e8400-e29b-41d4-a716-446655440001",
    });
    expect(store.slots[0].status).toBe("skipped");
    expect(store.slots[0].mac).toBe("aabbccddeeff");
    expect(store.slots[0].authUuid).toBe(
      "550e8400-e29b-41d4-a716-446655440001",
    );
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

describe("removeSlot — additional statuses", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("cannot remove a reading_mac slot", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "reading_mac";
    store.removeSlot("COM3");
    expect(store.slots).toHaveLength(1);
  });

  it("cannot remove an authorizing slot", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.removeSlot("COM3");
    expect(store.slots).toHaveLength(1);
  });

  it("cannot remove a failed slot", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "failed";
    store.removeSlot("COM3");
    expect(store.slots).toHaveLength(1);
  });
});

describe("slot defaults and field merging", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("initial slot has correct default fields including excelError", () => {
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

  it("updateSlot patch only changes named fields — mac preserved when updating phase", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].mac = "112233445566";
    store.handleAuthProgress({ port: "COM3", step: "reading_mac" });
    expect(store.slots[0].mac).toBe("112233445566");
  });

  it("excelError: undefined in patch clears the field", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].excelError = "row not found";
    store.handleAuthProgress({ port: "COM3", step: "cancelled" });
    expect(store.slots[0].excelError).toBeUndefined();
  });
});

describe("handleFlashProgress — port isolation", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("only updates the matching port", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.slots[0].status = "flashing";
    store.slots[1].status = "flashing";
    store.handleFlashProgress({
      port: "COM3",
      event: { kind: "percent", value: 75 },
    });
    expect(store.slots[0].progress).toBe(75);
    expect(store.slots[1].progress).toBe(0);
  });
});

describe("handleAuthProgress — excelError", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("skipped step propagates excelError to slot", () => {
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

  it("skipped step without excelError leaves excelError undefined", () => {
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

  it("done step propagates excelError when present in event", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "done",
      mac: "aabb",
      excelError: "confirm failed",
    });
    expect(store.slots[0].excelError).toBe("confirm failed");
  });
});

describe("retryFailed", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("clears excelError on failed slot", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "failed";
    store.slots[0].error = "some error";
    store.slots[0].excelError = "confirm_row failed";
    await store.retryFailed();
    expect(store.slots[0].excelError).toBeUndefined();
  });

  it("clears error on failed slot", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "failed";
    store.slots[0].error = "timeout";
    await store.retryFailed();
    expect(store.slots[0].error).toBeUndefined();
  });

  it("only resets failed slots — done and skipped are unchanged", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5", "COM7"]);
    store.slots[0].status = "failed";
    store.slots[1].status = "done";
    store.slots[1].mac = "aabb";
    store.slots[2].status = "skipped";
    store.slots[2].mac = "ccdd";
    await store.retryFailed();
    expect(store.slots[0].status).toBe("idle");
    expect(store.slots[1].status).toBe("done");
    expect(store.slots[1].mac).toBe("aabb");
    expect(store.slots[2].status).toBe("skipped");
    expect(store.slots[2].mac).toBe("ccdd");
  });

  it("is a no-op when canRetry is false (no failed slots)", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.completionBanner = { kind: "all-success", count: 1 };
    await store.retryFailed();
    expect(store.completionBanner).not.toBeNull();
  });
});

describe("checkBatchCompletion — active slot gate", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("no-ops when any slot is still active", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.batchStartTime = Date.now();
    store.slots[0].status = "authorizing";
    store.slots[1].status = "done";
    store.checkBatchCompletion();
    expect(store.completionBanner).toBeNull();
  });

  it("banner appears only after all active slots finish", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.batchStartTime = Date.now();
    store.currentBatchPorts = ["COM3", "COM5"];
    store.slots[0].status = "authorizing";
    store.slots[1].status = "done";
    store.checkBatchCompletion();
    expect(store.completionBanner).toBeNull();
    store.slots[0].status = "done";
    store.checkBatchCompletion();
    expect(store.completionBanner?.kind).toBe("all-success");
  });
});

describe("resetAuthStats", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("resets only auth cumulative to zero, flash stats unchanged", () => {
    const store = useBatchFlashAuthStore();
    store.cumulativeStats.auth = { total: 10, success: 8, fail: 2 };
    store.cumulativeStats.flash = { total: 5, success: 4, fail: 1 };
    store.resetAuthStats();
    expect(store.cumulativeStats.auth).toEqual({
      total: 0,
      success: 0,
      fail: 0,
    });
    expect(store.cumulativeStats.flash).toEqual({
      total: 5,
      success: 4,
      fail: 1,
    });
  });

  it("subsequent auth events accumulate from zero after reset", () => {
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

describe("addBlockedPort / removeBlockedPort", () => {
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

  it("addBlockedPort does NOT remove an active slot", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["/dev/ttyUSB0"]);
    store.slots[0].status = "authorizing";
    store.addBlockedPort("/dev/ttyUSB0");
    expect(store.slots).toHaveLength(1);
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

  it("filterActive reflects blockedPorts non-empty state", () => {
    const store = useBatchFlashAuthStore();
    expect(store.filterActive).toBe(false);
    store.addBlockedPort("/dev/ttyUSB0");
    expect(store.filterActive).toBe(true);
    store.removeBlockedPort("/dev/ttyUSB0");
    expect(store.filterActive).toBe(false);
  });
});

describe("isBusy computed", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("is false when all slots are terminal (done / failed / skipped / idle)", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    store.slots[0].status = "done";
    store.slots[1].status = "failed";
    expect(store.isBusy).toBe(false);
  });

  it("is true for each active status", () => {
    for (const status of ["flashing", "reading_mac", "authorizing"] as const) {
      setActivePinia(createPinia());
      const s = useBatchFlashAuthStore();
      s.addPorts(["COM3"]);
      s.slots[0].status = status;
      expect(s.isBusy).toBe(true);
    }
  });
});

describe("cancelledAfterWrite (B1 OTP-brick safety)", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("handleAuthProgress sets cancelledAfterWrite and emits failed status when payload step is cancelled_after_write", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";
    store.batchStartTime = Date.now();
    store.handleAuthProgress({
      port: "COM3",
      step: "cancelled_after_write",
      mac: "aabbccddeeff",
      uuid: "uuid-abc-123",
    });
    expect(store.slots[0].status).toBe("failed");
    expect(store.slots[0].cancelledAfterWrite).toBe(true);
    expect(store.slots[0].mac).toBe("aabbccddeeff");
    expect(store.slots[0].authUuid).toBe("uuid-abc-123");
    expect(store.slots[0].error).toBe(
      "Cancelled after auth write — device may carry credential",
    );
    expect(store.cumulativeStats.auth.total).toBe(1);
    expect(store.cumulativeStats.auth.fail).toBe(1);
  });

  it("canRetry is false when only cancelledAfterWrite slots remain", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "failed";
    store.slots[0].cancelledAfterWrite = true;
    expect(store.canRetry).toBe(false);
  });

  it("retryFailed does not reset cancelledAfterWrite slots", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3", "COM5"]);
    // cancelledAfterWrite slot
    store.slots[0].status = "failed";
    store.slots[0].cancelledAfterWrite = true;
    store.slots[0].error =
      "Cancelled after auth write — device may carry credential";
    // normal failed slot
    store.slots[1].status = "failed";
    store.slots[1].error = "timeout";
    await store.retryFailed();
    // cancelledAfterWrite slot must remain unchanged
    expect(store.slots[0].status).toBe("failed");
    expect(store.slots[0].cancelledAfterWrite).toBe(true);
    // normal failed slot must be reset to idle
    expect(store.slots[1].status).toBe("idle");
    expect(store.slots[1].error).toBeUndefined();
  });

  it("retryPort refuses to restart a cancelledAfterWrite slot", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.slots[0].status = "failed";
    store.slots[0].cancelledAfterWrite = true;
    store.slots[0].error =
      "Cancelled after auth write — device may carry credential";
    await store.retryPort("COM3");
    // slot must remain in failed state — not reset to idle
    expect(store.slots[0].status).toBe("failed");
    expect(store.slots[0].cancelledAfterWrite).toBe(true);
  });
});

describe("startBatch / cancelAll / cancelPort — web mode no-ops", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("startBatch does not throw and leaves slots unchanged", async () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["COM3"]);
    store.authConfig.excelPath = "/auth.xlsx";
    await store.startBatch();
    expect(store.slots[0].status).toBe("idle");
  });

  it("cancelAll does not throw", async () => {
    const store = useBatchFlashAuthStore();
    await expect(store.cancelAll()).resolves.toBeUndefined();
  });

  it("cancelPort does not throw", async () => {
    const store = useBatchFlashAuthStore();
    await expect(store.cancelPort("COM3")).resolves.toBeUndefined();
  });
});

describe("startBatch — firmware toggle in Tauri mode", () => {
  beforeEach(() => {
    vi.resetModules();
    setActivePinia(createPinia());
  });

  it("omits firmware fields when firmware flashing is disabled", async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    vi.doMock("@/runtime", () => ({ isTauriRuntime: () => true }));
    vi.doMock("@tauri-apps/api/core", () => ({ invoke }));
    vi.doMock("@/stores/batch-flash-auth-workspace", () => ({
      loadBatchFlashAuthWorkspace: vi.fn(),
      saveBatchFlashAuthCumulative: vi.fn(),
      saveBatchFlashAuthFilterConfig: vi.fn(),
      saveBatchFlashAuthFirmwareConfig: vi.fn(),
      saveBatchFlashAuthConfig: vi.fn(),
      saveBatchFlashAuthSharedConfig: vi.fn(),
    }));

    const { useBatchFlashAuthStore: useStore } =
      await import("./batch-flash-auth");
    const store = useStore();
    store.addPorts(["COM3"]);
    store.chipId = "esp32";
    store.flashFirmware = false;
    store.firmwarePath = "/path/to/fw.bin";
    store.authConfig.excelPath = "/auth.xlsx";
    store.excelStats = { total: 1, used: 0, inProgress: 0, remaining: 1 };

    await store.startBatch();

    expect(invoke).toHaveBeenCalledWith("batch_auth_start", {
      ports: ["COM3"],
      config: expect.objectContaining({
        firmwarePath: undefined,
        flashStartHex: undefined,
        flashEndHex: undefined,
      }),
    });
  });
});

describe("loadPersistedData — web mode", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("is a no-op — returns without throwing", async () => {
    const store = useBatchFlashAuthStore();
    await expect(store.loadPersistedData()).resolves.toBeUndefined();
  });

  it("does not alter state", async () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "t5ai";
    store.authConfig.excelPath = "/my/path.xlsx";
    await store.loadPersistedData();
    expect(store.authConfig.excelPath).toBe("/my/path.xlsx");
  });
});

describe("dismissBanner", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("clears the completion banner", () => {
    const store = useBatchFlashAuthStore();
    store.completionBanner = { kind: "all-success", count: 1 };
    store.dismissBanner();
    expect(store.completionBanner).toBeNull();
  });
});

describe("currentStats computed", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("counts active / done / failed / skipped correctly", () => {
    const store = useBatchFlashAuthStore();
    store.addPorts(["a", "b", "c", "d", "e"]);
    store.slots[0].status = "authorizing";
    store.slots[1].status = "done";
    store.slots[2].status = "failed";
    store.slots[3].status = "skipped";
    expect(store.currentStats).toEqual({
      active: 1,
      done: 1,
      failed: 1,
      skipped: 1,
    });
  });
});

describe("addPorts — cross-call deduplication", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("deduplicates ports across multiple addPorts calls", () => {
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

describe("loadPersistedData — legacy lockOtpAfterAuth key does not leak in", () => {
  beforeEach(() => {
    vi.resetModules();
    setActivePinia(createPinia());
  });

  // OTP lock was removed to match TuyaOpen firmware; a legacy persisted
  // `lockOtpAfterAuth` key must be dropped rather than spread back into config.
  it("strips the removed field while keeping the other persisted values", async () => {
    vi.doMock("@/stores/batch-flash-auth-workspace", () => ({
      loadBatchFlashAuthWorkspace: vi.fn().mockResolvedValue({
        authConfig: {
          excelPath: "/persisted/path.xlsx",
          conflictPolicy: "overwrite",
          authStorage: "otp",
          lockOtpAfterAuth: true,
        },
        sharedConfig: {
          chipId: "t5ai",
          baudRate: 921600,
          authBaudRate: 115200,
        },
      }),
      saveBatchFlashAuthCumulative: vi.fn(),
      saveBatchFlashAuthFilterConfig: vi.fn(),
      saveBatchFlashAuthFirmwareConfig: vi.fn(),
      saveBatchFlashAuthConfig: vi.fn(),
      saveBatchFlashAuthSharedConfig: vi.fn(),
    }));
    vi.doMock("@/runtime", () => ({ isTauriRuntime: () => true }));
    const { useBatchFlashAuthStore: useStore } =
      await import("./batch-flash-auth");
    const store = useStore();
    await store.loadPersistedData();
    expect(store.authConfig.excelPath).toBe("/persisted/path.xlsx");
    expect(store.authConfig.authStorage).toBe("otp");
    expect(
      (store.authConfig as Record<string, unknown>).lockOtpAfterAuth,
    ).toBeUndefined();
  });
});
