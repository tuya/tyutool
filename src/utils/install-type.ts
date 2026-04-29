import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { rLog } from '@/utils/log';

let cached: boolean | null = null;

/**
 * Detect whether the app is running as a portable installation.
 * Result is cached because install type never changes at runtime.
 *
 * In Tauri: calls the `get_install_type` command and checks for "portable".
 * In browser dev mode: always returns false.
 */
export async function isPortableInstall(): Promise<boolean> {
  if (cached !== null) return cached;

  if (!isTauriRuntime()) {
    cached = false;
    return false;
  }

  try {
    const { invoke } = await import('@tauri-apps/api/core');
    const installType = await invoke<string>('get_install_type');
    rLog.info(`[InstallType] Detected: "${installType}"`);
    cached = installType.toLowerCase().includes('portable');
  } catch {
    cached = false;
  }

  return cached;
}
