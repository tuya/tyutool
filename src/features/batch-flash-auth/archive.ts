// src/features/batch-flash-auth/archive.ts
// Pure helpers for the post-batch archive: folder naming, the summary object
// written to batch-summary.json, and the per-slot CSV. File-system work
// (copying the Excel/firmware, zipping logs) happens in the Rust command
// `archive_batch_cmd`, which also injects environment info and the firmware
// SHA-256 into the summary before writing it.
import type {
  BatchFirmwareSource,
  BatchOpMode,
  BatchSlotState,
  CompletionBanner,
  CumulativeStats,
  ExcelStats,
} from "./types";

export interface BatchArchiveInput {
  chipId: string;
  opMode: BatchOpMode;
  baudRate: number;
  authBaudRate: number;
  firmwareSource: BatchFirmwareSource;
  /** Selected default-firmware version ("" for local firmware). */
  firmwareVersion: string;
  firmwarePath: string;
  excelPath: string;
  conflictPolicy: "skip" | "overwrite";
  authStorage: "kv" | "otp";
  excelStats: ExcelStats | null;
  completionBanner: CompletionBanner | null;
  batchStartTime: number | null;
  batchEndTime: number | null;
  currentBatchPorts: string[];
  slots: BatchSlotState[];
  cumulativeStats: CumulativeStats;
  blockedPorts: string[];
}

/** `batch-archive_20260717-143205_esp32` — sortable, one folder per run. */
export function buildArchiveFolderName(chipId: string, now: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  const stamp =
    `${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}` +
    `-${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`;
  const chip = chipId.replace(/[^a-zA-Z0-9_-]/g, "_") || "unknown";
  return `batch-archive_${stamp}_${chip}`;
}

function iso(ms: number | null): string | null {
  return ms === null ? null : new Date(ms).toISOString();
}

/** The JSON written to batch-summary.json. Rust merges an `environment`
 *  section (app version, OS, session id) and firmware sha256/size into this
 *  before writing, so keep top-level key names stable. */
export function buildBatchArchiveSummary(
  input: BatchArchiveInput,
  now: Date,
): Record<string, unknown> {
  const batchPorts = new Set(input.currentBatchPorts);
  const batchSlots =
    batchPorts.size > 0
      ? input.slots.filter((s) => batchPorts.has(s.port))
      : input.slots;
  const count = (status: BatchSlotState["status"]) =>
    batchSlots.filter((s) => s.status === status).length;
  return {
    archivedAt: now.toISOString(),
    config: {
      chipId: input.chipId,
      opMode: input.opMode,
      baudRate: input.baudRate,
      authBaudRate: input.authBaudRate,
      conflictPolicy: input.conflictPolicy,
      authStorage: input.authStorage,
      excelPath: input.excelPath,
      blockedPorts: input.blockedPorts,
    },
    firmware:
      input.opMode !== "auth-only"
        ? {
            source: input.firmwareSource,
            version: input.firmwareVersion || null,
            path: input.firmwarePath,
          }
        : null,
    excelStats: input.excelStats,
    // Snapshot of the LAST started run only (the ports passed to the most
    // recent Start click). A production job done in multiple rounds is fully
    // recorded in the archived Excel copy — the sheet is the complete
    // device↔code ledger; this section is just the final round's scene.
    lastRun: {
      startedAt: iso(input.batchStartTime),
      endedAt: iso(input.batchEndTime),
      durationMs:
        input.batchStartTime !== null && input.batchEndTime !== null
          ? input.batchEndTime - input.batchStartTime
          : null,
      completion: input.completionBanner,
      ports: input.currentBatchPorts,
      stats: {
        done: count("done"),
        failed: count("failed"),
        skipped: count("skipped"),
        noCode: count("no_code"),
      },
    },
    cumulativeStats: input.cumulativeStats,
    slots: input.slots.map((s) => ({
      port: s.port,
      inBatch: batchPorts.size === 0 || batchPorts.has(s.port),
      status: s.status,
      mac: s.mac ?? null,
      uuid: s.authUuid ?? null,
      isAuthorized: s.isAuthorized ?? null,
      cancelledAfterWrite: s.cancelledAfterWrite ?? false,
      error: s.error ?? null,
      excelError: s.excelError ?? null,
      readError: s.readError ?? null,
    })),
  };
}

function csvCell(value: string): string {
  return /[",\n\r]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

/** batch-slots.csv — one row per slot, openable directly in Excel. */
export function buildSlotsCsv(
  slots: BatchSlotState[],
  currentBatchPorts: string[],
): string {
  const batchPorts = new Set(currentBatchPorts);
  const header = [
    "port",
    "inBatch",
    "status",
    "mac",
    "uuid",
    "cancelledAfterWrite",
    "error",
    "excelError",
  ];
  const rows = slots.map((s) =>
    [
      s.port,
      String(batchPorts.size === 0 || batchPorts.has(s.port)),
      s.status,
      s.mac ?? "",
      s.authUuid ?? "",
      String(s.cancelledAfterWrite ?? false),
      s.error ?? "",
      s.excelError ?? "",
    ]
      .map(csvCell)
      .join(","),
  );
  return [header.join(","), ...rows].join("\r\n") + "\r\n";
}
