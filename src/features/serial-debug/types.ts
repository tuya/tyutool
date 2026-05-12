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
  id: number;          // monotonic, used by filter subwindow to skip duplicates
  tsMs: number;
  direction: DebugLineDirection;
  text: string;        // already decoded (UTF-8, lossy) for ASCII view
  rawBytes?: Uint8Array; // kept when needed for hex view; undefined for 'sys' lines
}

export type SendMode = 'ascii' | 'hex';
export type FilterMode = 'off' | 'include' | 'exclude';
export type HexBytesPerRow = 8 | 16 | 32;

export interface SubWindow {
  id: string;
  name: string;
  filterText: string;
  useRegex: boolean;
  lines: DebugLogLine[];
}
