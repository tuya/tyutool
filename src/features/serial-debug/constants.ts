import type { DebugDataBits, DebugParity, DebugStopBits, HexBytesPerRow } from './types';

export const COMMON_BAUD_RATES = [
  9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600,
] as const;

export const DEFAULT_BAUD_RATE = 115200;
export const DEFAULT_DATA_BITS: DebugDataBits = 'eight';
export const DEFAULT_PARITY: DebugParity = 'none';
export const DEFAULT_STOP_BITS: DebugStopBits = 'one';
export const DEFAULT_HEX_BYTES_PER_ROW: HexBytesPerRow = 16;

export const MAX_LOG_LINES = 20000;
export const MAX_SEND_HISTORY = 20;
export const MAX_SUB_WINDOW_LINES = 10_000;
export const MAX_SUB_WINDOW_NAME_LENGTH = 15;
