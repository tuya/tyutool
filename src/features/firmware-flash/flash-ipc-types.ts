/**
 * Types aligned with `tyutool_core::FlashJob` / `FlashEvent` (snake_case JSON tag "kind").
 */

export type FlashJobMode = "flash" | "erase" | "read" | "authorize";

export interface FlashSegmentPayload {
  firmwarePath: string;
  startAddr: string;
  endAddr: string;
}

export interface FlashJobPayload {
  mode: FlashJobMode;
  chipId: string;
  port: string;
  baudRate: number;
  segments?: FlashSegmentPayload[] | null;
  flashStartHex?: string | null;
  flashEndHex?: string | null;
  eraseStartHex?: string | null;
  eraseEndHex?: string | null;
  readStartHex?: string | null;
  readEndHex?: string | null;
  readFilePath?: string | null;
  firmwarePath?: string | null;
  authorizeUuid?: string | null;
  authorizeKey?: string | null;
}

// Types aligned with tyutool_core::FlashEvent (snake_case JSON tag "kind")

export type FlashPhase =
  | "handshake"
  | "read_flash_id"
  | "unprotect"
  | "erase"
  | "write"
  | "verify"
  | "protect"
  | "reboot"
  | "read"
  | "save"
  | "load_ram"
  | "switch_baud"
  | "connect"
  | { write_segment: { current: number; total: number } }
  | { other: string };

export type FlashMilestone =
  | "handshake_complete"
  | "erase_complete"
  | "write_complete"
  | "verify_passed"
  | "rebooted"
  | { connected: { chip_info: string | null } }
  | { flash_id_read: { mid: number | null } }
  | { segment_written: { current: number; total: number } }
  | { auth_read_complete: { uuid: string; authkey: string } }
  | "auth_read_empty"
  | { auth_conflict: { existing_uuid: string; existing_authkey: string } };

export type FlashResultPayload =
  | { ok: { elapsed_secs: number } }
  | { err: { message: string; elapsed_secs: number } }
  | { cancelled: { elapsed_secs: number } };

export type JobDetails =
  | {
      type: "flash";
      firmware_path: string;
      firmware_size: number | null;
      range_start: string;
      range_end: string;
    }
  | {
      type: "read";
      output_path: string;
      range_start: string;
      range_end: string;
    }
  | { type: "erase"; range_start: string; range_end: string }
  | { type: "authorize"; write: boolean };

export type JobSummaryPayload = {
  port: string;
  baud: number;
  device: string | null;
  details: JobDetails;
};

export type FlashProgressPayload =
  | ({ kind: "job_summary" } & JobSummaryPayload)
  | { kind: "phase"; phase: FlashPhase }
  | { kind: "percent"; value: number }
  | { kind: "milestone"; milestone: FlashMilestone }
  | { kind: "warning"; message: string }
  | { kind: "done"; result: FlashResultPayload };
