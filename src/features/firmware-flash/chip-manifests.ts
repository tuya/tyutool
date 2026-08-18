import { AUTH_ONLY_CHIP_ID, normalizeChipId, type ChipId } from "./constants";
import type { ErasePresetKind } from "./types";

export interface ChipManifest {
  /** Rust registry plugin id in `tyutool_core::FlashPluginRegistry`. */
  rustPluginId: string;
  /** Default baud rate for this chip's flash protocol. */
  defaultBaudRate: number;
  /** Default baud rate for TuyaOpen UART authorization (independent of flash baud). */
  defaultAuthBaudRate: number;
  /** Default baud rate for serial debug / log monitor. */
  defaultLogBaudRate: number;
  /** Total flash size (used as default read end address). */
  flashSize: string;
  /**
   * When true, erase UI validates half-open `[start,end)` against 4 KiB alignment
   * (ESP + Beken families).
   */
  eraseRequires4KAlignment: boolean;
  /** Predefined erase address ranges (chip-family specific; may not include all kinds). */
  erasePresets: Partial<
    Record<ErasePresetKind, { start: string; end: string }>
  >;
}

/** Single source of truth for all per-chip parameters. */
export const CHIP_MANIFEST: Record<ChipId, ChipManifest> = {
  esp32: {
    rustPluginId: "ESP32",
    defaultBaudRate: 460800,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 115200,
    flashSize: "0x00400000", // 4 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: "0x00000000", end: "0x003FFFFF" },
    },
  },
  esp32c3: {
    rustPluginId: "ESP32C3",
    defaultBaudRate: 460800,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 115200,
    flashSize: "0x00400000", // 4 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: "0x00000000", end: "0x003FFFFF" },
    },
  },
  esp32c6: {
    rustPluginId: "ESP32C6",
    defaultBaudRate: 460800,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 115200,
    flashSize: "0x00800000", // 8 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: "0x00000000", end: "0x007FFFFF" },
    },
  },
  esp32p4: {
    rustPluginId: "ESP32P4",
    defaultBaudRate: 460800,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 115200,
    flashSize: "0x01000000", // 16 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: "0x00000000", end: "0x00FFFFFF" },
    },
  },
  esp32s3: {
    rustPluginId: "ESP32S3",
    defaultBaudRate: 460800,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 115200,
    flashSize: "0x01000000", // 16 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: "0x00000000", end: "0x00FFFFFF" },
    },
  },
  t5ai: {
    rustPluginId: "T5AI",
    defaultBaudRate: 921600,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 460800,
    flashSize: "0x00800000", // 8 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      // TuyaOpen platform/T5AI/tuyaos/tuyaos_adapter/src/driver/tkl_flash.c:
      // the tuya_data partition 0x7CD000 (196 KiB) holds KV_PROTECTED / USER1 /
      // KV / UF / KV_KEY — i.e. both the authorization data and the network
      // provisioning data. sys_rf (0x7FE000) and sys_net (0x7FF000) are the
      // last 8 KiB and are preserved by both presets.
      authInfo: { start: "0x007CD000", end: "0x007FDFFF" },
      fullChipNoRf: { start: "0x00000000", end: "0x007FDFFF" },
    },
  },
  t1: {
    rustPluginId: "T1",
    defaultBaudRate: 921600,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 115200,
    flashSize: "0x00800000", // 8 MiB — same layout as T5AI
    eraseRequires4KAlignment: true,
    erasePresets: {
      authInfo: { start: "0x007CD000", end: "0x007FDFFF" },
      fullChipNoRf: { start: "0x00000000", end: "0x007FDFFF" },
    },
  },
  t3: {
    rustPluginId: "T3",
    defaultBaudRate: 921600,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 460800,
    flashSize: "0x00400000", // 4 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      // TuyaOpen platform/T3/tuyaos/tuyaos_adapter/src/driver/tkl_flash.c:
      // the usr_config partition 0x3C9000 (212 KiB) holds KV_PROTECTED / RES1 /
      // KV_KEY / KV / UF / RES2. The RF calibration (0x3FE000) and fast-connect
      // (0x3FF000) blocks are the last 8 KiB and are preserved by both presets.
      authInfo: { start: "0x003C9000", end: "0x003FDFFF" },
      fullChipNoRf: { start: "0x00000000", end: "0x003FDFFF" },
    },
  },
  t2: {
    rustPluginId: "T2",
    defaultBaudRate: 921600,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 115200,
    flashSize: "0x00200000", // 2 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      authInfo: { start: "0x001EE000", end: "0x001FFFFF" },
      fullChipNoRf: { start: "0x00000000", end: "0x001EDFFF" },
    },
  },
  bk7231n: {
    rustPluginId: "BK7231N",
    defaultBaudRate: 921600,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 115200,
    flashSize: "0x00200000", // 2 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      authInfo: { start: "0x001EE000", end: "0x001FFFFF" },
      fullChipNoRf: { start: "0x00000000", end: "0x001EDFFF" },
    },
  },
  ln882h: {
    rustPluginId: "LN882H",
    defaultBaudRate: 115200,
    defaultAuthBaudRate: 115200,
    defaultLogBaudRate: 115200,
    flashSize: "0x00200000", // 2 MiB
    eraseRequires4KAlignment: true,
    erasePresets: {
      fullChip: { start: "0x00000000", end: "0x00200000" },
    },
  },
};

/** Manifest for {@link AUTH_ONLY_CHIP_ID} — authorize tab only (no flash plugin). */
const AUTH_ONLY_CHIP_MANIFEST: ChipManifest = {
  rustPluginId: "OTHER",
  defaultBaudRate: 115200,
  defaultAuthBaudRate: 115200,
  defaultLogBaudRate: 115200,
  flashSize: "0x00000000",
  eraseRequires4KAlignment: false,
  erasePresets: {},
};

/** Get manifest for a chip id; throws if unknown. Accepts legacy ids via
 *  {@link normalizeChipId} (e.g. `t5` → `t5ai`). */
export function chipManifest(chipId: string): ChipManifest {
  const id = normalizeChipId(chipId);
  if (id === AUTH_ONLY_CHIP_ID) {
    return AUTH_ONLY_CHIP_MANIFEST;
  }
  const m = CHIP_MANIFEST[id as ChipId];
  if (!m) throw new Error(`Unknown chip: ${chipId}`);
  return m;
}

/** Maps UI chip id to Rust registry id. Accepts legacy ids via
 *  {@link normalizeChipId} (e.g. `t5` → `t5ai`). */
export function rustPluginIdForChip(uiId: string): string {
  const id = normalizeChipId(uiId);
  if (id === AUTH_ONLY_CHIP_ID) {
    return chipManifest(AUTH_ONLY_CHIP_ID).rustPluginId;
  }
  return (
    CHIP_MANIFEST[id as ChipId]?.rustPluginId ??
    id.toUpperCase().replace(/-/g, "")
  );
}
