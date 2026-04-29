/**
 * Test helper: creates a fresh Pinia + flash store in jsdom environment.
 * Mocks isTauriRuntime() → false so all Tauri branches are skipped.
 */
import { createPinia, setActivePinia } from 'pinia';
import { vi } from 'vitest';

/**
 * Call in beforeEach to set up a fresh Pinia instance and mock Tauri runtime.
 * Returns the flash store instance.
 */
export async function createTestFlashStore() {
  // Mock isTauriRuntime before importing the store
  vi.mock('@/features/firmware-flash/flash-tauri', async importOriginal => {
    const actual = await importOriginal<typeof import('@/features/firmware-flash/flash-tauri')>();
    return {
      ...actual,
      isTauriRuntime: vi.fn(() => false),
    };
  });

  const pinia = createPinia();
  setActivePinia(pinia);

  // Dynamic import after mock is set up
  const { useFlashStore } = await import('../flash');
  return useFlashStore();
}
