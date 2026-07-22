// Tauri-only IPC wrappers for the per-source in-app updater. Unlike the
// plugin-updater JS `check()`, which always walks the static endpoint list in
// tauri.conf.json (GitHub first), these commands honor the source the user
// picked, so "update from Tuya OSS" really downloads via the OSS mirror.
import type { UpdateSource } from "./update-sources";

// Mirrors UpdateCheckReply in src-tauri/src/lib.rs
export interface UpdateCheckReply {
  available: boolean;
  version: string;
  currentVersion: string;
  date: string | null;
  body: string | null;
}

// Mirrors UpdateDownloadEvent in src-tauri/src/lib.rs
export type UpdateDownloadEvent =
  | { event: "Started"; data: { contentLength: number | null } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

export const UPDATE_DOWNLOAD_EVENT = "update-download-progress";

/** Check for an update against the manifest endpoint of the given source,
 *  staging the result Rust-side for {@link updateDownload}. */
export async function updateCheck(
  sourceId: UpdateSource["id"],
): Promise<UpdateCheckReply> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<UpdateCheckReply>("update_check", { source: sourceId });
}

/** Download the staged update, forwarding progress events to `onEvent`.
 *  The bytes stay Rust-side until {@link updateInstall}. */
export async function updateDownload(
  onEvent: (event: UpdateDownloadEvent) => void,
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<UpdateDownloadEvent>(
    UPDATE_DOWNLOAD_EVENT,
    (event) => onEvent(event.payload),
  );
  try {
    await invoke("update_download");
  } finally {
    unlisten();
  }
}

/** Install the downloaded update. On Windows this launches the installer;
 *  the caller relaunches the app afterwards. */
export async function updateInstall(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("update_install");
}
