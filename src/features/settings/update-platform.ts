import { isTauriRuntime } from "@/runtime";
import type { LatestJson, UpdateSource } from "./update-sources";

/** Native build target; WebView user agents do not reliably identify CPU architecture. */
export async function getUpdatePlatformKey(): Promise<string> {
  if (!isTauriRuntime()) return "";
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<string>("get_update_platform");
  } catch {
    return "";
  }
}

export function manualUpdateUrl(
  manifest: LatestJson | null,
  source: UpdateSource,
  platform: string,
  debRpm: boolean,
): string {
  return (
    (!debRpm && manifest?.portable?.[platform]?.url) || source.releasePageUrl
  );
}
