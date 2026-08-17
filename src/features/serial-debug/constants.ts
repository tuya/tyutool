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

export const DEFAULT_VISIBLE_LOG_WINDOW_LINES = 5000;
export const MIN_VISIBLE_LOG_WINDOW_LINES = 500;
export const MAX_VISIBLE_LOG_WINDOW_LINES = 20000;
export const VISIBLE_LOG_WINDOW_PRESETS = [
  1000, 3000, 5000, 10000, 20000,
] as const;
// Per-session cap for the Rust-side archive (crates/tyutool-core/src/serial_debug.rs).
// MiB, not lines: a line is 1–4096 B, so a line count says nothing about disk
// use — and `logWindowLines` above already means "lines" in a different sense.
// Keep DEFAULT in sync with DEFAULT_SERIAL_DEBUG_ARCHIVE_MAX_BYTES in Rust,
// which bounds the window before this setting is pushed down.
export const DEFAULT_ARCHIVE_LIMIT_MIB = 256;
export const MIN_ARCHIVE_LIMIT_MIB = 16;
export const MAX_ARCHIVE_LIMIT_MIB = 4096;
export const ARCHIVE_LIMIT_PRESETS = [64, 128, 256, 512, 1024] as const;
export const FILTER_PAGE_SIZE = 400;
// Archive lines per request while the "All" tab pages back through the session
// archive. Deliberately the same order as FILTER_PAGE_SIZE and *not*
// EXPORT_PAGE_SIZE: `serial_debug_session_read_page` costs four syscalls plus a
// JSON parse per line and holds the Rust-side archive lock for the whole
// request, so a read issued while the user is scrolling has to stay short
// enough not to stall the serial writer.
export const HISTORY_PAGE_SIZE = 400;
// Pages fetched when history mode is entered (or when jumping to the session
// start): enough to fill a screen and leave room to scroll before the next
// request, while the window still grows on demand up to `logWindowLines`.
export const HISTORY_ENTRY_PAGES = 3;
export const EXPORT_PAGE_SIZE = 1000;
export const FILTER_LIVE_REFRESH_MS = 120;
export const MAX_PENDING_LINE_BYTES = 4096;
export const AUTO_SAVE_FLUSH_MAX_CHARS = 128 * 1024;
// Max appends one periodic (non-drainAll) auto-save flush issues before it
// gives the tick back. 16 x AUTO_SAVE_FLUSH_MAX_CHARS = 2 MiB per 5 s tick,
// ~3x what a 921600-baud port can produce in that window, so a backlog still
// converges to empty while one tick can never occupy the IPC channel
// indefinitely.
export const AUTO_SAVE_FLUSH_MAX_APPENDS_PER_TICK = 16;
// Archive lines per page when backfilling a mid-session auto-save enable.
// Same order of magnitude as EXPORT_PAGE_SIZE: one page is read, formatted and
// appended before the next is requested, so peak memory is one page.
export const AUTO_SAVE_BACKFILL_PAGE_SIZE = 1000;
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
