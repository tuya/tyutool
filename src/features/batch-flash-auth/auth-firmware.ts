import { isTauriRuntime } from "@/runtime";
import type { AuthFirmwareEntry, AuthFirmwareManifest } from "./types";
import { rLog } from "@/utils/log";

/** Same shape as the app-update `UpdateSource`, but auth firmware keeps its own
 *  source set (GitHub + Gitee) independent of the app-update sources. */
export interface AuthFirmwareSource {
  id: "github" | "gitee";
  labelKey: string;
  url: string;
  releasePageUrl: string;
}

/** Manifest sources for the designated `auth-firmware` release.
 *  GitHub first, Gitee as fallback. */
export const AUTH_FIRMWARE_SOURCES: AuthFirmwareSource[] = [
  {
    id: "github",
    labelKey: "settings.update.sourceGithub",
    url: "https://github.com/tuya/tyutool/releases/download/auth-firmware/auth-firmware.json",
    releasePageUrl:
      "https://github.com/tuya/tyutool/releases/tag/auth-firmware",
  },
  {
    id: "gitee",
    labelKey: "settings.update.sourceGitee",
    url: "https://gitee.com/tuya-open/tyutool/releases/download/auth-firmware/auth-firmware.json",
    releasePageUrl:
      "https://gitee.com/tuya-open/tyutool/releases/tag/auth-firmware",
  },
];

/** Descending numeric-aware version comparison (tolerates a leading 'v').
 *  Uses Intl numeric collation so all segments and non-numeric suffixes
 *  (e.g. `v1.0.0-rc1`) get a deterministic order. Mirrors the script-side
 *  helper in scripts/generate-auth-firmware-manifest.ts. */
function compareVersionDesc(a: string, b: string): number {
  const strip = (v: string): string => v.replace(/^v/, "");
  return strip(b).localeCompare(strip(a), "en", { numeric: true });
}

/** Keep only entries for the given chip, sorted newest-first. */
export function filterByChip(
  entries: AuthFirmwareEntry[],
  chipId: string,
): AuthFirmwareEntry[] {
  return entries
    .filter((e) => e.chip === chipId)
    .sort((a, b) => compareVersionDesc(a.version, b.version));
}

async function fetchManifest(
  url: string,
  timeoutMs: number,
): Promise<AuthFirmwareManifest> {
  if (isTauriRuntime()) {
    const { invoke } = await import("@tauri-apps/api/core");
    const text = await invoke<string>("fetch_url", { url, timeoutMs });
    return JSON.parse(text) as AuthFirmwareManifest;
  }
  // Browser/dev fallback
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(url, { signal: controller.signal });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return (await res.json()) as AuthFirmwareManifest;
  } finally {
    clearTimeout(timer);
  }
}

/** Fetch the manifest, trying GitHub then Gitee. Throws if all sources fail. */
export async function fetchAuthFirmwareManifest(
  timeoutMs = 8000,
): Promise<{ sourceId: "github" | "gitee"; manifest: AuthFirmwareManifest }> {
  const errors: string[] = [];
  for (const source of AUTH_FIRMWARE_SOURCES) {
    try {
      const manifest = await fetchManifest(source.url, timeoutMs);
      return { sourceId: source.id, manifest };
    } catch (e) {
      const reason = e instanceof Error ? e.message : String(e);
      rLog.warn(`[AuthFw] manifest source '${source.id}' failed: ${reason}`);
      errors.push(`${source.id}: ${reason}`);
    }
  }
  const msg = `All auth firmware sources failed (${errors.join("; ")})`;
  rLog.warn(`[AuthFw] ${msg}`);
  throw new Error(msg);
}

/** Download (and SHA-256 verify) a firmware entry via the Rust command.
 *  Desktop-only — returns the absolute local path. */
export async function downloadAuthFirmware(
  entry: AuthFirmwareEntry,
): Promise<string> {
  if (!isTauriRuntime()) {
    throw new Error("download requires desktop runtime");
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string>("download_auth_firmware", {
    url: entry.url,
    sha256: entry.sha256,
    version: entry.version,
  });
}
