import { describe, it, expect } from "vitest";
import {
  buildArchiveFolderName,
  buildBatchArchiveSummary,
  buildSlotsCsv,
  type BatchArchiveInput,
} from "./archive";
import type { BatchSlotState } from "./types";

const NOW = new Date(2026, 6, 17, 14, 32, 5); // 2026-07-17 14:32:05 local

function slot(
  patch: Partial<BatchSlotState> & { port: string },
): BatchSlotState {
  return { status: "idle", progress: 0, currentPhase: "", ...patch };
}

function baseInput(): BatchArchiveInput {
  return {
    chipId: "esp32",
    opMode: "flash-then-auth",
    baudRate: 921600,
    authBaudRate: 115200,
    firmwareSource: "default",
    firmwareVersion: "1.2.0",
    firmwarePath: "C:/fw/auth.bin",
    excelPath: "C:/codes/batch.xlsx",
    conflictPolicy: "skip",
    authStorage: "kv",
    excelStats: {
      total: 100,
      used: 40,
      inProgress: 0,
      remaining: 60,
      invalid: 0,
    },
    completionBanner: { kind: "partial", done: 2, failed: 1 },
    batchStartTime: 1_000,
    batchEndTime: 61_000,
    currentBatchPorts: ["COM3", "COM4", "COM5"],
    slots: [
      slot({ port: "COM3", status: "done", mac: "AA:BB", authUuid: "uuid-3" }),
      slot({ port: "COM4", status: "failed", error: 'boom, with "quotes"' }),
      slot({ port: "COM5", status: "no_code" }),
      slot({ port: "COM9", status: "done" }), // prior-run slot, not in batch
    ],
    cumulativeStats: {
      flash: { total: 5, success: 4, fail: 1 },
      auth: { total: 5, success: 3, fail: 2 },
    },
    blockedPorts: ["COM1"],
  };
}

describe("buildArchiveFolderName", () => {
  it("formats a sortable stamp with the chip id", () => {
    expect(buildArchiveFolderName("esp32", NOW)).toBe(
      "batch-archive_20260717-143205_esp32",
    );
  });

  it("sanitizes unexpected characters in the chip id", () => {
    expect(buildArchiveFolderName("t5/ai", NOW)).toBe(
      "batch-archive_20260717-143205_t5_ai",
    );
  });
});

describe("buildBatchArchiveSummary", () => {
  it("counts lastRun stats only over the current batch ports", () => {
    const summary = buildBatchArchiveSummary(baseInput(), NOW);
    const lastRun = summary.lastRun as {
      stats: Record<string, number>;
      durationMs: number;
      ports: string[];
    };
    // COM9's done is a prior run and must not be counted.
    expect(lastRun.stats).toEqual({
      done: 1,
      failed: 1,
      skipped: 0,
      noCode: 1,
    });
    expect(lastRun.durationMs).toBe(60_000);
    expect(lastRun.ports).toEqual(["COM3", "COM4", "COM5"]);
  });

  it("marks prior-run slots as inBatch=false but still lists them", () => {
    const summary = buildBatchArchiveSummary(baseInput(), NOW);
    const slots = summary.slots as Array<{ port: string; inBatch: boolean }>;
    expect(slots).toHaveLength(4);
    expect(slots.find((s) => s.port === "COM9")?.inBatch).toBe(false);
    expect(slots.find((s) => s.port === "COM3")?.inBatch).toBe(true);
  });

  it("omits the firmware section in auth-only mode", () => {
    const input = { ...baseInput(), opMode: "auth-only" as const };
    const summary = buildBatchArchiveSummary(input, NOW);
    expect(summary.firmware).toBeNull();
  });

  it("keeps config and excel stats for traceability", () => {
    const summary = buildBatchArchiveSummary(baseInput(), NOW);
    expect(summary.config).toMatchObject({
      chipId: "esp32",
      conflictPolicy: "skip",
      authStorage: "kv",
      blockedPorts: ["COM1"],
    });
    expect(summary.excelStats).toMatchObject({ total: 100, remaining: 60 });
  });
});

describe("buildSlotsCsv", () => {
  it("emits a header plus one row per slot with CRLF endings", () => {
    const csv = buildSlotsCsv(baseInput().slots, ["COM3", "COM4", "COM5"]);
    const lines = csv.split("\r\n");
    expect(lines[0]).toBe(
      "port,inBatch,status,mac,uuid,cancelledAfterWrite,error,excelError",
    );
    expect(lines).toHaveLength(6); // header + 4 rows + trailing empty
    expect(lines[1]).toBe("COM3,true,done,AA:BB,uuid-3,false,,");
    expect(lines[4]).toBe("COM9,false,done,,,false,,");
  });

  it("quotes cells containing commas or quotes", () => {
    const csv = buildSlotsCsv(
      [slot({ port: "COM4", status: "failed", error: 'boom, with "quotes"' })],
      [],
    );
    expect(csv).toContain('"boom, with ""quotes"""');
  });
});
