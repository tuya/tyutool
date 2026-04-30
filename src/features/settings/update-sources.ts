import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';

export interface CliPlatformManifest {
  url: string;
  sha256: string;
}

export interface PlatformManifest {
  url: string;
  signature: string;
}

export interface PortableManifest {
  url: string;
}

export interface LatestJson {
  version: string;
  notes: string;
  pub_date: string;
  platforms: Record<string, PlatformManifest>;
  cli: Record<string, CliPlatformManifest>;
  portable?: Record<string, PortableManifest>;
}

export interface UpdateSource {
  id: 'github' | 'gitee';
  labelKey: string;
  url: string;
  releasePageUrl: string;
}

export const UPDATE_SOURCES: UpdateSource[] = [
  {
    id: 'github',
    labelKey: 'settings.update.sourceGithub',
    url: 'https://github.com/tuya/tyutool/releases/latest/download/latest.json',
    releasePageUrl: 'https://github.com/tuya/tyutool/releases/latest',
  },
  {
    id: 'gitee',
    labelKey: 'settings.update.sourceGitee',
    url: 'https://gitee.com/tuya-open/tyutool/releases/download/latest/latest.json',
    releasePageUrl: 'https://gitee.com/tuya-open/tyutool/releases',
  },
];

/** Returns true if remoteVersion > localVersion (simple semver numeric comparison) */
export function isNewerVersion(remoteVersion: string, localVersion: string): boolean {
  const parse = (v: string): number[] => v.replace(/^v/, '').split('.').map(Number);
  const r = parse(remoteVersion);
  const l = parse(localVersion);
  for (let i = 0; i < 3; i++) {
    const rv = r[i] ?? 0;
    const lv = l[i] ?? 0;
    if (rv !== lv) return rv > lv;
  }
  return false;
}

/** Fetch latest.json from a source URL with timeout.
 *  In Tauri runtime, uses Rust-side HTTP to bypass WebView CSP restrictions.
 *  In browser dev mode, uses native fetch(). */
export async function fetchLatestJson(url: string, timeoutMs = 8000): Promise<LatestJson> {
  if (isTauriRuntime()) {
    const { invoke } = await import('@tauri-apps/api/core');
    const { info, error: logError } = await import('@tauri-apps/plugin-log');
    await info(`[Update] fetchLatestJson: url=${url}, timeoutMs=${timeoutMs}`);
    try {
      const text = await invoke<string>('fetch_url', { url, timeoutMs: timeoutMs });
      await info(`[Update] fetchLatestJson: response length=${text.length}, content=${text.substring(0, 300)}`);
      const json = JSON.parse(text) as LatestJson;
      await info(
        `[Update] fetchLatestJson: parsed version=${json.version}, platforms=${Object.keys(json.platforms ?? {}).join(',')}`
      );
      return json;
    } catch (e: unknown) {
      const errMsg = e instanceof Error ? e.message : String(e);
      await logError(`[Update] fetchLatestJson failed for ${url}: ${errMsg}`);
      throw e;
    }
  }
  // Browser fallback
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(url, { signal: controller.signal });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return (await res.json()) as LatestJson;
  } finally {
    clearTimeout(timer);
  }
}
