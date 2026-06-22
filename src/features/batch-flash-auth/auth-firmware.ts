import { isTauriRuntime } from "@/runtime";
import type { UpdateSource } from "@/features/settings/update-sources";
import type { AuthFirmwareEntry, AuthFirmwareManifest } from "./types";

/** Manifest sources for the designated `auth-firmware` release.
 *  GitHub first, Gitee as fallback — mirrors the update-source resolution. */
export const AUTH_FIRMWARE_SOURCES: UpdateSource[] = [
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

/** Descending numeric semver-ish comparison (tolerates a leading 'v'). */
function compareVersionDesc(a: string, b: string): number {
  const parse = (v: string): number[] =>
    v.replace(/^v/, "").split(".").map(Number);
  const pa = parse(a);
  const pb = parse(b);
  for (let i = 0; i < 3; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return y - x;
  }
  return 0;
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
  for (const source of AUTH_FIRMWARE_SOURCES) {
    try {
      const manifest = await fetchManifest(source.url, timeoutMs);
      return { sourceId: source.id, manifest };
    } catch {
      // try next source
    }
  }
  throw new Error("All auth firmware sources failed");
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
