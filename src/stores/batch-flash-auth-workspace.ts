import { isTauriRuntime } from "@/runtime";
import type {
  CumulativeStats,
  PortFilterConfig,
  BatchFirmwareSource,
  BatchAuthConfigData,
} from "@/features/batch-flash-auth/types";

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

    const sharedConfig =
      (await store.get<BatchSharedConfig>(SHARED_CONFIG_KEY)) ?? null;

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
