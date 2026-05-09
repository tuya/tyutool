import type {
  DebugDataBits, DebugParity, DebugStopBits, FilterMode, HexBytesPerRow, SendMode,
} from '@/features/serial-debug/types';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';

export const SD_WORKSPACE_VERSION = 1;
const KEY = 'serial-debug-workspace.v1';

export interface SerialDebugWorkspaceSerialized {
  v: number;
  port: string;
  baudRate: number;
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
  filterText: string;
  filterMode: FilterMode;
}

export async function loadSerialDebugWorkspace(): Promise<SerialDebugWorkspaceSerialized | null> {
  if (isTauriRuntime()) {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('.tyutool-workspace.dat');
      const raw = await store.get<SerialDebugWorkspaceSerialized>(KEY);
      return raw && raw.v === SD_WORKSPACE_VERSION ? raw : null;
    } catch {
      return null;
    }
  }
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as SerialDebugWorkspaceSerialized;
    return parsed.v === SD_WORKSPACE_VERSION ? parsed : null;
  } catch {
    return null;
  }
}

export async function saveSerialDebugWorkspace(data: SerialDebugWorkspaceSerialized): Promise<void> {
  if (isTauriRuntime()) {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('.tyutool-workspace.dat');
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
