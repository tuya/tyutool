import { isTauriRuntime } from "@/runtime";
import type {
  CumulativeStats,
  PortFilterConfig,
} from "@/features/batch-flash-auth/types";

const CUMULATIVE_KEY = "batch-flash-auth-cumulative";
const FILTER_KEY = "batch-flash-auth-port-filter";
const LEGACY_CUMULATIVE_KEY = "batch-flash-cumulative";
const LEGACY_FILTER_KEY = "batch-flash-port-filter";
const STORE_FILE = "settings.json";

export async function loadBatchFlashAuthWorkspace(): Promise<{
  cumulative: CumulativeStats | null;
  filter: PortFilterConfig | null;
}> {
  if (!isTauriRuntime()) {
    return { cumulative: null, filter: null };
  }
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

  return { cumulative, filter };
}

export async function saveBatchFlashAuthCumulative(
  stats: CumulativeStats,
): Promise<void> {
  if (!isTauriRuntime()) return;
  const { Store } = await import("@tauri-apps/plugin-store");
  const store = await Store.load(STORE_FILE);
  await store.set(CUMULATIVE_KEY, stats);
  await store.save();
}

export async function saveBatchFlashAuthFilterConfig(
  filter: PortFilterConfig,
): Promise<void> {
  if (!isTauriRuntime()) return;
  const { Store } = await import("@tauri-apps/plugin-store");
  const store = await Store.load(STORE_FILE);
  await store.set(FILTER_KEY, filter);
  await store.save();
}
