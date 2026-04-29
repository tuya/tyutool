<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { provideFirmwareFlash } from './context';
import { isTauriRuntime } from './flash-tauri';
import FirmwareFlashAside from './components/FirmwareFlashAside.vue';
import FirmwareFlashConnection from './components/FirmwareFlashConnection.vue';
import FirmwareFlashOperations from './components/FirmwareFlashOperations.vue';

const { t } = useI18n();
provideFirmwareFlash();

async function resetWindowSize(): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('reset_main_window_layout');
  } catch (_e) {
    /* silently ignore — window API may not be permitted */
  }
}
</script>

<template>
  <div class="flex min-h-0 min-w-0 w-full flex-1 flex-col gap-2 md:gap-3">
    <header
      class="page-header relative flex min-w-0 flex-col gap-3 overflow-hidden p-3 sm:flex-row sm:items-center sm:p-3.5"
    >
      <!-- 背景装饰（与 conn-bar 统一） -->
      <div class="page-header-bg pointer-events-none absolute inset-0" aria-hidden="true" />

      <!-- 图标 + 标题 -->
      <div class="relative flex min-w-0 flex-1 items-center gap-3">
        <div class="page-header-icon flex size-10 shrink-0 items-center justify-center rounded-xl" aria-hidden="true">
          <FontAwesomeIcon :icon="['fas', 'microchip']" class="size-5" />
        </div>
        <div class="min-w-0">
          <p class="page-header-section">{{ t('flash.tool') }}</p>
          <h1 class="page-header-title mt-0.5">{{ t('flash.pageTitle') }}</h1>
        </div>
        <!-- 竖分隔线 -->
        <div class="page-header-divider ml-1 hidden h-8 w-px shrink-0 sm:block" aria-hidden="true" />
        <p class="page-header-desc relative hidden max-w-[26rem] sm:block">
          <span class="font-semibold text-[var(--ty-text)]">{{ t('flash.introLead') }}</span
          >{{ t('flash.introRest') }}
        </p>
      </div>

      <!-- 辅助按钮 -->
      <div v-if="isTauriRuntime()" class="relative flex shrink-0 items-center">
        <button
          type="button"
          class="page-header-btn rounded-lg px-2.5 py-1.5 text-xs font-semibold transition-all duration-150"
          :aria-label="t('app.resetWindowSize')"
          @click="resetWindowSize"
        >
          {{ t('app.resetWindowSize') }}
        </button>
      </div>
    </header>

    <FirmwareFlashConnection />

    <div class="flex min-h-0 flex-1 flex-col gap-2 lg:min-h-0 lg:flex-row lg:gap-3 lg:items-stretch">
      <FirmwareFlashOperations />
      <FirmwareFlashAside />
    </div>
  </div>
</template>
