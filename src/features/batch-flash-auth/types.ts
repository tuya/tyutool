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
  | "reading"
  | "flashing"
  | "reading_mac"
  | "authorizing"
  | "done"
  | "failed"
  | "skipped"
  | "no_code";

export interface BatchSlotState {
  port: string;
  status: BatchSlotStatus;
  progress: number; // 0–100
  currentPhase: string;
  mac?: string;
  /** UUID from auth-read (set after a successful read probe). */
  authUuid?: string;
  /** true = authorized, false = not authorized, undefined = never probed. */
  isAuthorized?: boolean;
  /** Error message from the last read probe (if it failed). */
  readError?: string;
  error?: string;
  excelError?: string;
  /**
   * true ⇒ auth was written to device OTP but the eFuse lock command failed;
   * the operator must physically isolate this device to prevent UUID/Key reuse.
   */
  lockFailed?: boolean;
  /**
   * true ⇒ auth write command was sent to device but cancel arrived before
   * verify completed. KV storage is overwritable; OTP is permanently written.
   * Operator must physically isolate this device until manually verified.
   */
  cancelledAfterWrite?: boolean;
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
  authStorage: "kv" | "otp";
  /** Send `auth-otp-lock` after a successful new-firmware write+verify.
   *  ONLY effective when chipId='t5ai' AND authStorage='otp'.
   *  IRREVERSIBLE — burns the device eFuse.
   *  Default false; forced to false on every app start (see store
   *  loadPersistedData). */
  lockOtpAfterAuth: boolean;
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
    | "skipped"
    | "no_code"
    | "cancelled"
    | "cancelled_after_write";
  mac?: string;
  uuid?: string;
  error?: string;
  excelError?: string;
  event?: unknown;
  /** Present (and true) only when step="failed" was caused by a LockFailed
   *  result — auth was written to OTP but eFuse lock subsequently failed. */
  lockFailed?: boolean;
}

/** `batch-auth-read-progress` event from Rust (read-only probe). */
export interface BatchAuthReadProgressEvent {
  port: string;
  step: "done" | "failed" | "cancelled";
  mac?: string;
  uuid?: string;
  error?: string;
}

/** Mirrors Rust BatchAuthStartConfig. */
export interface BatchAuthStartConfig {
  chipId: string;
  baudRate: number;
  authBaudRate: number;
  firmwarePath?: string;
  /** Flash start address for the firmware (e.g. "0x00000000"). Required when firmwarePath is set. */
  flashStartHex?: string;
  /** Flash end address for the firmware (e.g. "0x001EDFFF"). Required when firmwarePath is set. */
  flashEndHex?: string;
  excelPath: string;
  conflictPolicy: "skip" | "overwrite";
  /** Auth storage destination. Only T5AI supports "otp"; defaults to "kv". */
  authStorage?: "kv" | "otp";
  /** Send `auth-otp-lock` after successful write+verify. ONLY effective
   *  when chipId='t5ai' AND authStorage='otp'. IRREVERSIBLE — burns eFuse.
   *  The Tauri backend re-validates chip + storage before forwarding. */
  lockOtpAfterAuth?: boolean;
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
