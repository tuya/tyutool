import { isTauriRuntime } from "@/runtime";
import type {
  CumulativeStats,
  PortFilterConfig,
  BatchFirmwareSource,
  BatchAuthConfigData,
} from "@/features/batch-flash-auth/types";
import { BATCH_AUTH_TOOL_CHIP_OPTIONS } from "@/features/batch-flash-auth/types";
import { BAUD_RATE_OPTIONS } from "@/features/firmware-flash/constants";
import { chipManifest } from "@/features/firmware-flash/chip-manifests";

export interface BatchFirmwareConfig {
  source: BatchFirmwareSource;
  version: string;
  localPath?: string;
}

export interface BatchSharedConfig {
  chipId: string;
  baudRate: number;
  authBaudRate: number;
  flashFirmware?: boolean;
  authorizeEnabled?: boolean;
}

const FIRMWARE_KEY = "batch-flash-auth-firmware";
const CUMULATIVE_KEY = "batch-flash-auth-cumulative";
const FILTER_KEY = "batch-flash-auth-port-filter";
const AUTH_CONFIG_KEY = "batch-flash-auth-config";
const SHARED_CONFIG_KEY = "batch-flash-auth-shared-config";
const LEGACY_CUMULATIVE_KEY = "batch-flash-cumulative";
const LEGACY_FILTER_KEY = "batch-flash-port-filter";
const STORE_FILE = "settings.json";

/** Safe default chip for the batch-auth tool when a persisted chipId is invalid. */
const DEFAULT_BATCH_CHIP_ID = "esp32";

/**
 * Parse and validate a persisted shared-config record (already deserialized by
 * the Tauri store). Mirrors the strict validators in flash-workspace.ts /
 * serial-debug-workspace.ts: each field is checked against its legal enum / type
 * and repaired to a safe default when corrupted, so a stale or damaged record
 * can never crash the UI (e.g. an unknown chipId throwing inside chipManifest).
 * Returns null for non-object input, indicating "no shared config" / first run.
 */
export function parseBatchSharedConfig(rec: unknown): BatchSharedConfig | null {
  if (!rec || typeof rec !== "object") return null;
  const r = rec as Record<string, unknown>;
  try {
    const bool = (v: unknown, fallback: boolean): boolean =>
      typeof v === "boolean" ? v : fallback;
    const numOrNull = (v: unknown): number | null =>
      typeof v === "number" && Number.isFinite(v) ? v : null;

    // chipId must be a valid batch-auth chip option; otherwise fall back.
    const chipId =
      typeof r.chipId === "string" &&
      (BATCH_AUTH_TOOL_CHIP_OPTIONS as readonly string[]).includes(r.chipId)
        ? r.chipId
        : DEFAULT_BATCH_CHIP_ID;

    // Baud rates must be recognized options; otherwise fall back to the chip's
    // manifest defaults (chipManifest throws on unknown chips — guarded above).
    const baudRaw = numOrNull(r.baudRate);
    const baudRate =
      baudRaw !== null &&
      (BAUD_RATE_OPTIONS as readonly number[]).includes(baudRaw)
        ? baudRaw
        : chipManifest(chipId).defaultBaudRate;

    const authBaudRaw = numOrNull(r.authBaudRate);
    const authBaudRate =
      authBaudRaw !== null &&
      (BAUD_RATE_OPTIONS as readonly number[]).includes(authBaudRaw)
        ? authBaudRaw
        : chipManifest(chipId).defaultAuthBaudRate;

    return {
      chipId,
      baudRate,
      authBaudRate,
      flashFirmware: bool(r.flashFirmware, true),
      authorizeEnabled: bool(r.authorizeEnabled, true),
    };
  } catch {
    // chipManifest or any other unexpected throw → treat as no shared config.
    return null;
  }
}

export async function loadBatchFlashAuthWorkspace(): Promise<{
  cumulative: CumulativeStats | null;
  filter: PortFilterConfig | null;
  firmware: BatchFirmwareConfig | null;
  authConfig: BatchAuthConfigData | null;
  sharedConfig: BatchSharedConfig | null;
}> {
  if (!isTauriRuntime()) {
    return {
      cumulative: null,
      filter: null,
      firmware: null,
      authConfig: null,
      sharedConfig: null,
    };
  }
  try {
    const { Store } = await import("@tauri-apps/plugin-store");
    const store = await Store.load(STORE_FILE);

    let cumulative = (await store.get<CumulativeStats>(CUMULATIVE_KEY)) ?? null;
    if (!cumulative) {
      cumulative =
        (await store.get<CumulativeStats>(LEGACY_CUMULATIVE_KEY)) ?? null;
      if (cumulative) {
        await store.set(CUMULATIVE_KEY, cumulative);
        await store.save();
      }
    }

    let filter = (await store.get<PortFilterConfig>(FILTER_KEY)) ?? null;
    if (!filter) {
      filter = (await store.get<PortFilterConfig>(LEGACY_FILTER_KEY)) ?? null;
      if (filter) {
        await store.set(FILTER_KEY, filter);
        await store.save();
      }
    }

    const firmware =
      (await store.get<BatchFirmwareConfig>(FIRMWARE_KEY)) ?? null;

    const authConfig =
      (await store.get<BatchAuthConfigData>(AUTH_CONFIG_KEY)) ?? null;

    const sharedConfig = parseBatchSharedConfig(
      await store.get<BatchSharedConfig>(SHARED_CONFIG_KEY),
    );

    return { cumulative, filter, firmware, authConfig, sharedConfig };
  } catch (e) {
    console.warn(
      "[batch-flash-auth] workspace load failed, using defaults:",
      e,
    );
    return {
      cumulative: null,
      filter: null,
      firmware: null,
      authConfig: null,
      sharedConfig: null,
    };
  }
}

export async function saveBatchFlashAuthCumulative(
  stats: CumulativeStats,
): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const { Store } = await import("@tauri-apps/plugin-store");
    const store = await Store.load(STORE_FILE);
    await store.set(CUMULATIVE_KEY, stats);
    await store.save();
  } catch (e) {
    console.warn("[batch-flash-auth] cumulative stats save failed:", e);
  }
}

export async function saveBatchFlashAuthFilterConfig(
  filter: PortFilterConfig,
): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const { Store } = await import("@tauri-apps/plugin-store");
    const store = await Store.load(STORE_FILE);
    await store.set(FILTER_KEY, filter);
    await store.save();
  } catch (e) {
    console.warn("[batch-flash-auth] filter config save failed:", e);
  }
}

export async function saveBatchFlashAuthFirmwareConfig(
  cfg: BatchFirmwareConfig,
): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const { Store } = await import("@tauri-apps/plugin-store");
    const store = await Store.load(STORE_FILE);
    await store.set(FIRMWARE_KEY, cfg);
    await store.save();
  } catch (e) {
    console.warn("[batch-flash-auth] firmware config save failed:", e);
  }
}

export async function saveBatchFlashAuthConfig(
  cfg: BatchAuthConfigData,
): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const { Store } = await import("@tauri-apps/plugin-store");
    const store = await Store.load(STORE_FILE);
    await store.set(AUTH_CONFIG_KEY, cfg);
    await store.save();
  } catch (e) {
    console.warn("[batch-flash-auth] auth config save failed:", e);
  }
}

export async function saveBatchFlashAuthSharedConfig(
  cfg: BatchSharedConfig,
): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const { Store } = await import("@tauri-apps/plugin-store");
    const store = await Store.load(STORE_FILE);
    await store.set(SHARED_CONFIG_KEY, cfg);
    await store.save();
  } catch (e) {
    console.warn("[batch-flash-auth] shared config save failed:", e);
  }
}
