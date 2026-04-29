import { onMounted, onUnmounted } from 'vue';
import { useFlashStore } from '@/stores/flash';

/** Firmware flash workspace: mounts/unmounts lifecycle around the flash Pinia store. */
export function useFirmwareFlash() {
  const store = useFlashStore();

  onMounted(() => {
    // Re-attach the flash-progress listener (it may have been
    // detached if the component was unmounted during idle).
    // Port scanning is done once at app startup (main.ts), not on
    // every page navigation — avoids redundant scans when switching
    // between settings and flash pages.
    void store.ensureFlashListener();
  });

  onUnmounted(() => {
    // Only clean up timers/listeners if no operation is running.
    // If a flash/read/erase is in progress, keep the event listener alive
    // so we can still receive progress updates when the user navigates back.
    if (!store.busy) {
      store.cleanup();
    }
  });

  return store;
}

export type FirmwareFlashContext = ReturnType<typeof useFirmwareFlash>;
