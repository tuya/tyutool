import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { rLog } from '@/utils/log';

let cachedInstallType: string | null = null;

async function resolveInstallType(): Promise<string> {
  if (!isTauriRuntime()) {
    return 'browser';
  }
  if (cachedInstallType !== null) {
    return cachedInstallType;
  }

  try {
    const { invoke } = await import('@tauri-apps/api/core');
    cachedInstallType = await invoke<string>('get_install_type');
    rLog.info(`[InstallType] Detected: "${cachedInstallType}"`);
  } catch {
    cachedInstallType = 'unknown';
  }

  return cachedInstallType;
}

export interface ManualUpdateFlags {
  /** True for portable tree or Linux .deb/.rpm install under /usr or /opt (no Tauri in-app updater). */
  manualOnly: boolean;
  /** True when installed via distro package (.deb/.rpm path); prefer releases page over portable tarball URL. */
  debRpm: boolean;
}

export function getManualUpdateFlagsForInstallType(installType: string): ManualUpdateFlags {
  const t = installType.toLowerCase();
  return {
    manualOnly: t.includes('portable') || t.includes('deb/rpm'),
    debRpm: t.includes('deb/rpm'),
  };
}

export function canUseInAppUpdater(installTypeReady: boolean, flags: ManualUpdateFlags): boolean {
  return installTypeReady && !flags.manualOnly;
}

/**
 * Returns flags derived from `get_install_type` (cached).
 * `manualOnly` enables the same UX as portable: no plugin-updater download/install, releases-page flow.
 */
export async function getManualUpdateFlags(): Promise<ManualUpdateFlags> {
  const raw = await resolveInstallType();
  return getManualUpdateFlagsForInstallType(raw);
}

/**
 * Detect whether the app is running as a portable installation.
 * Result is cached because install type never changes at runtime.
 *
 * In Tauri: calls the `get_install_type` command and checks for "portable".
 * In browser dev mode: always returns false.
 */
export async function isPortableInstall(): Promise<boolean> {
  const t = (await resolveInstallType()).toLowerCase();
  return t.includes('portable');
}
