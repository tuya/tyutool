// src/features/batch-flash-auth/types.ts
import type { FlashProgressPayload } from "@/features/firmware-flash/flash-ipc-types";

/** Chips available in the batch auth tool (all support the auth serial protocol). */
// When GD32 support is added to the Rust plugin registry, append "gd32" here.
export const BATCH_AUTH_TOOL_CHIP_OPTIONS = ["esp32", "t5ai", "other"] as const;

/** Subset of BATCH_AUTH_TOOL_CHIP_OPTIONS that also have a registered flash plugin. */
// When GD32 support is added to the Rust plugin registry, append "gd32" here.
export const BATCH_FLASH_CAPABLE_CHIPS = ["esp32", "t5ai"] as const;

export type BatchOpMode = "auth-only" | "flash-then-auth";

export type BatchSlotStatus =
  | "idle"
  | "flashing"
  | "reading_mac"
  | "authorizing"
  | "done"
  | "failed"
  | "skipped";

export interface BatchSlotState {
  port: string;
  status: BatchSlotStatus;
  progress: number; // 0–100
  currentPhase: string;
  mac?: string;
  error?: string;
}

export interface CumulativeStats {
  flash: { total: number; success: number; fail: number };
  auth: { total: number; success: number; fail: number };
}

export interface PortFilterConfig {
  blockedPorts: string[];
}

export interface BatchAuthConfigData {
  excelPath: string;
  conflictPolicy: "skip" | "overwrite";
}

/** `batch-flash-progress` event payload from Rust. */
export interface BatchFlashProgressEvent {
  port: string;
  event: FlashProgressPayload;
}

/** `batch-auth-progress` event from Rust. */
export interface BatchAuthProgressEvent {
  port: string;
  step:
    | "flashing"
    | "reading_mac"
    | "reading_auth"
    | "writing_auth"
    | "verifying"
    | "done"
    | "failed"
    | "skipped";
  mac?: string;
  error?: string;
  event?: unknown;
}

/** Mirrors Rust BatchAuthStartConfig. */
export interface BatchAuthStartConfig {
  chipId: string;
  baudRate: number;
  firmwarePath?: string;
  excelPath: string;
  conflictPolicy: "skip" | "overwrite";
}

/** Stats returned by validate_excel_cmd. */
export interface ExcelStats {
  total: number;
  used: number;
  remaining: number;
}

/** Discriminated by `kind`; carries the counts the UI needs to render a
 *  localized message (no preformatted string — translation happens in the view). */
export type CompletionBanner =
  | { kind: "all-skipped"; count: number }
  | { kind: "all-success"; count: number }
  | { kind: "all-failed" }
  | { kind: "partial"; done: number; failed: number };

/** Firmware source mode for the batch auth tool. */
export type BatchFirmwareSource = "local" | "default";

/** One downloadable authorization firmware version.
 *  Mirrors an entry in the `auth-firmware.json` release manifest. */
export interface AuthFirmwareEntry {
  version: string;
  chip: string;
  url: string;
  sha256: string;
  size?: number;
  notes?: string;
}

/** Top-level shape of `auth-firmware.json`. */
export interface AuthFirmwareManifest {
  firmwares: AuthFirmwareEntry[];
}
