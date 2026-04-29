/**
 * Domain types for the firmware flash feature (chip IDs, operations, UI phase).
 */

export type OpKind = 'flash' | 'erase' | 'read' | 'authorize';

export type FlashPhase = 'idle' | 'running' | 'success' | 'error';

export type ErasePresetKind = 'authInfo' | 'fullChipNoRf' | 'fullChip';

export interface FlashSegment {
  id: string;
  firmwarePath: string;
  firmwareFile: File | null;
  startAddr: string;
  endAddr: string;
}
