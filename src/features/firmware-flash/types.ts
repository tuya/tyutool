/**
 * Domain types for the firmware flash feature (chip IDs, operations, UI phase).
 */

export type OpKind = "flash" | "erase" | "read" | "authorize";

export type FlashPhase = "idle" | "running" | "success" | "error";

/** Public operation metadata only: never include authorization credentials. */
export interface FlashOperationSnapshot {
  op: OpKind;
  chipId: string;
  port: string;
  baud: number;
  outputPath: string | null;
  authRead: boolean;
}

export interface FlashOperationResult extends FlashOperationSnapshot {
  status: "success" | "error" | "cancelled";
  elapsedSecs: number | null;
  message: string;
}

export type ErasePresetKind = "authInfo" | "fullChipNoRf" | "fullChip";

export interface FlashSegment {
  id: string;
  firmwarePath: string;
  firmwareFile: File | null;
  /** Last measured file size; absent when a restored path has not been inspected. */
  firmwareSize?: number;
  startAddr: string;
  endAddr: string;
}
