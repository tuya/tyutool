import { APP_VERSION } from '@/config/app';
import { useFlashStore } from '@/stores/flash';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { useSettingsStore } from '@/stores/settings';
import { isTauriRuntime } from '@/runtime';
import { rLog } from '@/utils/log';

/**
 * Restore workspaces and refresh devices after settings (locale) are ready.
 */
export async function bootstrapApp(): Promise<void> {
  const settings = useSettingsStore();
  await settings.ready();

  rLog.info(`[Frontend] tyutool v${APP_VERSION} initialized`);
  rLog.info(
    `[Frontend] Platform: ${navigator.platform}, Lang: ${navigator.language}, Tauri: ${isTauriRuntime()}`,
  );

  const flash = useFlashStore();
  await flash.loadWorkspace();
  flash.startWorkspacePersistence();
  void flash.refreshDevice();

  const sd = useSerialDebugStore();
  await sd.loadWorkspace();
  sd.startWorkspacePersistence();
}
