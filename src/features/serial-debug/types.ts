/**
 * Types shared across the serial-debug feature.
 * Shapes mirror `tyutool_core::serial_debug::*` (camelCase over JSON / Tauri events).
 */

export type DebugDataBits = 'five' | 'six' | 'seven' | 'eight';
export type DebugParity = 'none' | 'odd' | 'even';
export type DebugStopBits = 'one' | 'onePointFive' | 'two';

export interface DebugConfig {
  port: string;
  baudRate: number;
  dataBits: DebugDataBits;
  parity: DebugParity;
  stopBits: DebugStopBits;
}

export interface DebugChunk {
  direction: 'tx' | 'rx';
  tsMs: number;
  bytes: number[]; // Tauri deserializes Vec<u8> as a number[]
}

export interface DisconnectPayload {
  reason: string;
}

export type DebugLineDirection = 'tx' | 'rx' | 'sys';

export interface DebugLogLine {
  id: number;          // monotonic
  tsMs: number;
  direction: DebugLineDirection;
  text: string;        // already decoded (UTF-8, lossy) for ASCII view
  rawBytes?: Uint8Array; // kept when needed for hex view; undefined for 'sys' lines
}

export type SendMode = 'ascii' | 'hex';
export type HexBytesPerRow = 8 | 16 | 32;

export type ChipMode = 'highlight' | 'filter' | 'off';

export interface WatchChip {
  id: string;
  keyword: string;
  useRegex: boolean;
  mode: ChipMode;
  color: string; // CSS hex string, e.g. '#ef4444'
}
