import type { ChipId } from './constants';
import type { ErasePresetKind } from './types';

export interface ChipManifest {
  /** Rust registry plugin id in `tyutool_core::FlashPluginRegistry`. */
  rustPluginId: string;
  /** Default baud rate for this chip's flash protocol. */
  defaultBaudRate: number;
  /** Total flash size (used as default read end address). */
  flashSize: string;
  /**
   * When true, erase UI validates half-open `[start,end)` against 4 KiB alignment
   * (ESP + Beken families).
   */
  eraseRequires4KAlignment: boolean;
  /** Predefined erase address ranges (chip-family specific; may not include all kinds). */
  erasePresets: Partial<Record<ErasePresetKind, { start: string; end: string }>>;
}

/** Single source of truth for all per-chip parameters. */
export const CHIP_MANIFEST: Record<ChipId, ChipManifest> = {
  esp32: {
    rustPluginId: 'ESP32',
    defaultBaudRate: 460800,
    flashSize: '0x00400000', // 4 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: '0x00000000', end: '0x003FFFFF' },
    },
  },
  esp32c3: {
    rustPluginId: 'ESP32C3',
    defaultBaudRate: 460800,
    flashSize: '0x00400000', // 4 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: '0x00000000', end: '0x003FFFFF' },
    },
  },
  esp32c6: {
    rustPluginId: 'ESP32C6',
    defaultBaudRate: 460800,
    flashSize: '0x00800000', // 8 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: '0x00000000', end: '0x007FFFFF' },
    },
  },
  esp32s3: {
    rustPluginId: 'ESP32S3',
    defaultBaudRate: 460800,
    flashSize: '0x01000000', // 16 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: '0x00000000', end: '0x00FFFFFF' },
    },
  },
  t5: {
    rustPluginId: 'T5',
    defaultBaudRate: 921600,
    flashSize: '0x00800000', // 8 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      authInfo: { start: '0x001EE000', end: '0x001FFFFF' },
      fullChipNoRf: { start: '0x00000000', end: '0x001EDFFF' },
    },
  },
  t1: {
    rustPluginId: 'T1',
    defaultBaudRate: 921600,
    flashSize: '0x00800000', // 8 MiB — same layout as T5
    eraseRequires4KAlignment: true,
    erasePresets: {
      authInfo: { start: '0x001EE000', end: '0x001FFFFF' },
      fullChipNoRf: { start: '0x00000000', end: '0x001EDFFF' },
    },
  },
  t3: {
    rustPluginId: 'T3',
    defaultBaudRate: 921600,
    flashSize: '0x00400000', // 4 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      authInfo: { start: '0x001EE000', end: '0x001FFFFF' },
      fullChipNoRf: { start: '0x00000000', end: '0x003FDFFF' },
    },
  },
  t2: {
    rustPluginId: 'T2',
    defaultBaudRate: 921600,
    flashSize: '0x00200000', // 2 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      authInfo: { start: '0x001EE000', end: '0x001FFFFF' },
      fullChipNoRf: { start: '0x00000000', end: '0x001EDFFF' },
    },
  },
  bk7231n: {
    rustPluginId: 'BK7231N',
    defaultBaudRate: 921600,
    flashSize: '0x00200000', // 2 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      authInfo: { start: '0x001EE000', end: '0x001FFFFF' },
      fullChipNoRf: { start: '0x00000000', end: '0x001EDFFF' },
    },
  },
};

/** Get manifest for a chip id; throws if unknown. */
export function chipManifest(chipId: string): ChipManifest {
  const m = CHIP_MANIFEST[chipId as ChipId];
  if (!m) throw new Error(`Unknown chip: ${chipId}`);
  return m;
}

/** Maps UI chip id to Rust registry id. */
export function rustPluginIdForChip(uiId: string): string {
  return CHIP_MANIFEST[uiId as ChipId]?.rustPluginId ?? uiId.toUpperCase().replace(/-/g, '');
}
