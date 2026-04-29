/**
 * Persisted flash UI state (workspace): chip, baud, paths, addresses, tabs.
 * Stored in the same Tauri Store file as app settings, or localStorage in web dev.
 */

import {
  BAUD_RATE_OPTIONS,
  AUTH_BAUD_RATE_DEFAULT,
  CHIP_IDS,
  DEFAULT_CHIP_ID,
} from '@/features/firmware-flash/constants';
import { chipManifest } from '@/features/firmware-flash/chip-manifests';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import type { OpKind } from '@/features/firmware-flash/types';

/** Same file as `settings.ts` — single JSON store on disk. */
const STORE_FILE = 'settings.json';

export const WORKSPACE_JSON_KEY = 'tyutool-flash-workspace-json';

export const WORKSPACE_VERSION = 1 as const;

export interface FlashWorkspaceSerialized {
  v: typeof WORKSPACE_VERSION;
  activeTab: OpKind;
  selectedSerialPort: string;
  selectedBaudRate: number;
  /** Baud rate used exclusively for TuyaOpen UART authorization (default 115200). */
  authBaudRate: number;
  selectedChipId: string;
  flashSegments: Array<{
    id: string;
    firmwarePath: string;
    startAddr: string;
    endAddr: string;
  }>;
  activeSegmentIndex: number;
  eraseAdvancedOpen: boolean;
  eraseStartAddr: string;
  eraseEndAddr: string;
  readStartAddr: string;
  readEndAddr: string;
  readDir: string;
  readFileName: string;
  readFileNameModified: boolean;
  authorizeUuid: string;
  authorizeAuthKey: string;
}

const OP_KINDS: OpKind[] = ['flash', 'erase', 'read', 'authorize'];

function isOpKind(x: unknown): x is OpKind {
  return typeof x === 'string' && (OP_KINDS as string[]).includes(x);
}

function normalizeBaud(chipId: string, baud: unknown): number {
  const n = typeof baud === 'number' && Number.isFinite(baud) ? baud : chipManifest(DEFAULT_CHIP_ID).defaultBaudRate;
  if ((BAUD_RATE_OPTIONS as readonly number[]).includes(n)) {
    return n;
  }
  try {
    return chipManifest(chipId).defaultBaudRate;
  } catch {
    return chipManifest(DEFAULT_CHIP_ID).defaultBaudRate;
  }
}

function normalizeAuthBaud(baud: unknown): number {
  const n = typeof baud === 'number' && Number.isFinite(baud) ? baud : AUTH_BAUD_RATE_DEFAULT;
  return (BAUD_RATE_OPTIONS as readonly number[]).includes(n) ? n : AUTH_BAUD_RATE_DEFAULT;
}

function clampSegmentIndex(index: unknown, len: number): number {
  if (typeof index !== 'number' || !Number.isFinite(index)) {
    return 0;
  }
  const i = Math.floor(index);
  if (i < 0 || i >= len) {
    return 0;
  }
  return i;
}

/**
 * Parse and validate workspace JSON. Returns null if invalid or unsupported version.
 */
export function parseFlashWorkspaceJson(raw: string | null): FlashWorkspaceSerialized | null {
  if (raw === null || raw === undefined || raw.trim() === '') {
    return null;
  }
  try {
    const o = JSON.parse(raw) as unknown;
    if (!o || typeof o !== 'object') {
      return null;
    }
    const rec = o as Record<string, unknown>;
    if (rec.v !== WORKSPACE_VERSION) {
      return null;
    }
    const chipId = typeof rec.selectedChipId === 'string' ? rec.selectedChipId : '';
    if (!(CHIP_IDS as readonly string[]).includes(chipId)) {
      return null;
    }
    const activeTab: OpKind = isOpKind(rec.activeTab) ? rec.activeTab : 'flash';
    const segsIn = rec.flashSegments;
    if (!Array.isArray(segsIn) || segsIn.length < 1 || segsIn.length > 10) {
      return null;
    }
    const flashSegments: FlashWorkspaceSerialized['flashSegments'] = [];
    for (let i = 0; i < segsIn.length; i++) {
      const s = segsIn[i];
      if (!s || typeof s !== 'object') {
        return null;
      }
      const seg = s as Record<string, unknown>;
      const firmwarePath = typeof seg.firmwarePath === 'string' ? seg.firmwarePath : '';
      const startAddr = typeof seg.startAddr === 'string' ? seg.startAddr : '0x00000000';
      const endAddr = typeof seg.endAddr === 'string' ? seg.endAddr : '0x00000000';
      const id = typeof seg.id === 'string' && seg.id.length > 0 ? seg.id : Math.random().toString(36).substring(2, 9);
      flashSegments.push({ id, firmwarePath, startAddr, endAddr });
    }
    const baud = normalizeBaud(chipId, rec.selectedBaudRate);
    const authBaud = normalizeAuthBaud(rec.authBaudRate);
    const activeSegmentIndex = clampSegmentIndex(rec.activeSegmentIndex, flashSegments.length);
    return {
      v: WORKSPACE_VERSION,
      activeTab,
      selectedSerialPort: typeof rec.selectedSerialPort === 'string' ? rec.selectedSerialPort : '',
      selectedBaudRate: baud,
      selectedChipId: chipId,
      flashSegments,
      activeSegmentIndex,
      eraseAdvancedOpen: rec.eraseAdvancedOpen === true,
      eraseStartAddr: typeof rec.eraseStartAddr === 'string' ? rec.eraseStartAddr : '0x00000000',
      eraseEndAddr: typeof rec.eraseEndAddr === 'string' ? rec.eraseEndAddr : '0x00000000',
      readStartAddr: typeof rec.readStartAddr === 'string' ? rec.readStartAddr : '0x00000000',
      readEndAddr: typeof rec.readEndAddr === 'string' ? rec.readEndAddr : chipManifest(chipId).flashSize,
      readDir: typeof rec.readDir === 'string' ? rec.readDir : '',
      readFileName:
        typeof rec.readFileName === 'string' ? rec.readFileName : `tyutool_read_${chipId.toLowerCase()}.bin`,
      readFileNameModified: rec.readFileNameModified === true,
      authorizeUuid: typeof rec.authorizeUuid === 'string' ? rec.authorizeUuid : '',
      authorizeAuthKey: typeof rec.authorizeAuthKey === 'string' ? rec.authorizeAuthKey : '',
      authBaudRate: authBaud,
    };
  } catch {
    return null;
  }
}

/** Load workspace from Tauri store or localStorage. */
export async function loadFlashWorkspaceFromStorage(): Promise<FlashWorkspaceSerialized | null> {
  let raw: string | null = null;
  if (isTauriRuntime()) {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load(STORE_FILE);
      const v = await store.get<string>(WORKSPACE_JSON_KEY);
      raw = v ?? null;
    } catch {
      raw = null;
    }
  } else {
    raw = localStorage.getItem(WORKSPACE_JSON_KEY);
  }
  return parseFlashWorkspaceJson(raw);
}

/** Save workspace JSON. */
export async function saveFlashWorkspaceToStorage(data: FlashWorkspaceSerialized): Promise<void> {
  const json = JSON.stringify(data);
  if (isTauriRuntime()) {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load(STORE_FILE);
      await store.set(WORKSPACE_JSON_KEY, json);
      await store.save();
    } catch {
      /* ignore */
    }
  } else {
    try {
      localStorage.setItem(WORKSPACE_JSON_KEY, json);
    } catch {
      /* ignore */
    }
  }
}
