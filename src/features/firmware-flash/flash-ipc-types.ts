/**
 * Types aligned with `tyutool_core::FlashJob` / `FlashEvent` (snake_case JSON tag "kind").
 *
 * The `FlashJob` family (`FlashJobMode`, `FlashSegmentPayload`, `FlashJobPayload`) is
 * ts-rs-generated from `crates/tyutool-core/src/job.rs` / `authorize.rs` — see
 * `src/bindings/` and the ts-rs binding note in AGENTS.md's Tauri IPC contract section.
 * Re-exported under their pre-existing names here so consumers need no changes.
 * `FlashEvent` below is still hand-mirrored; do not add it to this generated set.
 */
export type { FlashJob as FlashJobPayload } from "@/bindings/FlashJob";
export type { FlashMode as FlashJobMode } from "@/bindings/FlashMode";
export type { FlashSegment as FlashSegmentPayload } from "@/bindings/FlashSegment";
export type { AuthStorage } from "@/bindings/AuthStorage";

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
  | { auth_conflict: { existing_uuid: string; existing_authkey: string } }
  | "auth_write_skipped"
  | "auth_write_sent";

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
