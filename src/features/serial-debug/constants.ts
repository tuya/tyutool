import type {
  DebugDataBits,
  DebugParity,
  DebugStopBits,
  HexBytesPerRow,
} from "./types";

export const COMMON_BAUD_RATES = [
  9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600,
] as const;

export const DEFAULT_BAUD_RATE = 115200;
export const DEFAULT_DATA_BITS: DebugDataBits = "eight";
export const DEFAULT_PARITY: DebugParity = "none";
export const DEFAULT_STOP_BITS: DebugStopBits = "one";
export const DEFAULT_HEX_BYTES_PER_ROW: HexBytesPerRow = 16;

export const VISIBLE_LOG_WINDOW_LINES = 3000;
export const FILTER_PAGE_SIZE = 400;
export const EXPORT_PAGE_SIZE = 1000;
export const FILTER_LIVE_REFRESH_MS = 120;
export const MAX_PENDING_LINE_BYTES = 4096;
export const AUTO_SAVE_FLUSH_MAX_CHARS = 128 * 1024;
export const MAX_SEND_HISTORY = 20;

// Six distinct colors that cycle when chips are added
export const CHIP_COLORS = [
  "#6366f1", // indigo
  "#06b6d4", // cyan
  "#10b981", // emerald
  "#8b5cf6", // violet
  "#0ea5e9", // sky
  "#ec4899", // pink
] as const;
