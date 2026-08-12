import type {
  DebugDataBits,
  DebugParity,
  DebugStopBits,
  HexBytesPerRow,
  SendMode,
} from "@/features/serial-debug/types";
import {
  COMMON_BAUD_RATES,
  DEFAULT_DATA_BITS,
  DEFAULT_HEX_BYTES_PER_ROW,
  DEFAULT_PARITY,
  DEFAULT_STOP_BITS,
  MAX_SEND_HISTORY,
} from "@/features/serial-debug/constants";
import { isTauriRuntime } from "@/runtime";

export const SD_WORKSPACE_VERSION = 1;
const KEY = "serial-debug-workspace.v1";

const VALID_DATA_BITS: readonly DebugDataBits[] = [
  "five",
  "six",
  "seven",
  "eight",
];
const VALID_PARITY: readonly DebugParity[] = ["none", "odd", "even"];
const VALID_STOP_BITS: readonly DebugStopBits[] = [
  "one",
  "onePointFive",
  "two",
];
const VALID_SEND_MODE: readonly SendMode[] = ["ascii", "hex"];
const VALID_HEX_BYTES_PER_ROW: readonly HexBytesPerRow[] = [8, 16, 32];
const VALID_FONT_SIZES: readonly number[] = [
  10, 11, 12, 13, 14, 16, 18, 20, 24,
];

export interface SerialDebugWorkspaceSerialized {
  v: number;
  port: string;
  /** null = follow flash chip default; number = user's explicit choice. */
  baudRate: number | null;
  customBaudRate: number | null;
  dataBits: DebugDataBits;
  parity: DebugParity;
  stopBits: DebugStopBits;
  autoRelease: boolean;
  hexView: boolean;
  hexBytesPerRow: HexBytesPerRow;
  sendMode: SendMode;
  sendAppendCrlf: boolean;
  sendHistory: string[];
  ansiEnabled?: boolean;
  logFontSize?: number;
  autoSave?: boolean;
  autoSaveDir?: string;
  autoSaveTimestamp?: boolean;
  showTimestamp?: boolean;
  showDirBadge?: boolean;
}

/**
 * Parse and validate a raw workspace record (already deserialized). Each field
 * is checked against its legal enum / type and clamped to a safe default when
 * corrupted — mirroring the strict parseFlashWorkspaceJson strategy. Damaged
 * data is repaired in place rather than crashing the UI.
 */
export function parseSerialDebugWorkspace(
  rec: unknown,
): SerialDebugWorkspaceSerialized | null {
  if (!rec || typeof rec !== "object") return null;
  const r = rec as Record<string, unknown>;
  if (r.v !== SD_WORKSPACE_VERSION) return null;

  const str = (v: unknown, fallback = ""): string =>
    typeof v === "string" ? v : fallback;
  const bool = (v: unknown, fallback = false): boolean =>
    typeof v === "boolean" ? v : fallback;
  const numOrNull = (v: unknown): number | null =>
    typeof v === "number" && Number.isFinite(v) ? v : null;

  // Baud rate: null (follow chip default) or a recognized common rate.
  const baudRaw = numOrNull(r.baudRate);
  const baudRate =
    baudRaw === null
      ? null
      : (COMMON_BAUD_RATES as readonly number[]).includes(baudRaw)
        ? baudRaw
        : null;

  // Custom baud rate: only meaningful as a positive finite number.
  const customBaudRaw = numOrNull(r.customBaudRate);
  const customBaudRate =
    customBaudRaw !== null && customBaudRaw > 0 ? customBaudRaw : null;

  const dataBits: DebugDataBits = VALID_DATA_BITS.includes(
    r.dataBits as DebugDataBits,
  )
    ? (r.dataBits as DebugDataBits)
    : DEFAULT_DATA_BITS;
  const parity: DebugParity = VALID_PARITY.includes(r.parity as DebugParity)
    ? (r.parity as DebugParity)
    : DEFAULT_PARITY;
  const stopBits: DebugStopBits = VALID_STOP_BITS.includes(
    r.stopBits as DebugStopBits,
  )
    ? (r.stopBits as DebugStopBits)
    : DEFAULT_STOP_BITS;
  const sendMode: SendMode = VALID_SEND_MODE.includes(r.sendMode as SendMode)
    ? (r.sendMode as SendMode)
    : "ascii";
  const hexBytesPerRow: HexBytesPerRow = VALID_HEX_BYTES_PER_ROW.includes(
    r.hexBytesPerRow as HexBytesPerRow,
  )
    ? (r.hexBytesPerRow as HexBytesPerRow)
    : DEFAULT_HEX_BYTES_PER_ROW;
  const logFontSize = (() => {
    const n = numOrNull(r.logFontSize);
    return n !== null && (VALID_FONT_SIZES as readonly number[]).includes(n)
      ? n
      : 12;
  })();

  // Send history: must be an array of strings; cap length + drop non-strings.
  const sendHistory: string[] = Array.isArray(r.sendHistory)
    ? r.sendHistory
        .filter((s): s is string => typeof s === "string")
        .slice(0, MAX_SEND_HISTORY)
    : [];

  return {
    v: SD_WORKSPACE_VERSION,
    port: str(r.port),
    baudRate,
    customBaudRate,
    dataBits,
    parity,
    stopBits,
    autoRelease: bool(r.autoRelease),
    hexView: bool(r.hexView),
    hexBytesPerRow,
    sendMode,
    sendAppendCrlf: bool(r.sendAppendCrlf),
    sendHistory,
    ansiEnabled:
      r.ansiEnabled === undefined ? undefined : bool(r.ansiEnabled, true),
    logFontSize,
    autoSave: r.autoSave === undefined ? undefined : bool(r.autoSave),
    autoSaveDir: r.autoSaveDir === undefined ? undefined : str(r.autoSaveDir),
    autoSaveTimestamp:
      r.autoSaveTimestamp === undefined
        ? undefined
        : bool(r.autoSaveTimestamp, true),
    showTimestamp:
      r.showTimestamp === undefined ? undefined : bool(r.showTimestamp, true),
    showDirBadge:
      r.showDirBadge === undefined ? undefined : bool(r.showDirBadge, true),
  };
}

export async function loadSerialDebugWorkspace(): Promise<SerialDebugWorkspaceSerialized | null> {
  if (isTauriRuntime()) {
    try {
      const { Store } = await import("@tauri-apps/plugin-store");
      const store = await Store.load(".tyutool-workspace.dat");
      const raw = await store.get<unknown>(KEY);
      return parseSerialDebugWorkspace(raw);
    } catch {
      return null;
    }
  }
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    return parseSerialDebugWorkspace(JSON.parse(raw));
  } catch {
    return null;
  }
}

export async function saveSerialDebugWorkspace(
  data: SerialDebugWorkspaceSerialized,
): Promise<void> {
  if (isTauriRuntime()) {
    try {
      const { Store } = await import("@tauri-apps/plugin-store");
      const store = await Store.load(".tyutool-workspace.dat");
      await store.set(KEY, data);
      await store.save();
    } catch {
      // ignore
    }
    return;
  }
  try {
    localStorage.setItem(KEY, JSON.stringify(data));
  } catch {
    // ignore
  }
}
