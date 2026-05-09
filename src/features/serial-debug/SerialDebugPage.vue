<script setup lang="ts">
import { onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import SerialDebugConnectionBar from './components/SerialDebugConnectionBar.vue';
import SerialDebugFilterBar from './components/SerialDebugFilterBar.vue';
import SerialDebugLogPane from './components/SerialDebugLogPane.vue';
import SerialDebugSendBar from './components/SerialDebugSendBar.vue';
import RxSelectionHexPopup from './components/RxSelectionHexPopup.vue';

const { t } = useI18n();
const s = useSerialDebugStore();

onUnmounted(() => {
  if (s.open) void s.closePort();
});
</script>

<template>
  <div class="flex min-h-0 min-w-0 w-full flex-1 flex-col gap-2 md:gap-3">
    <header class="page-header relative flex min-w-0 flex-col gap-3 overflow-hidden p-3 sm:flex-row sm:items-center sm:p-3.5">
      <div class="page-header-bg pointer-events-none absolute inset-0" aria-hidden="true" />
      <div class="relative flex min-w-0 flex-1 items-center gap-3">
        <div class="page-header-icon flex size-10 shrink-0 items-center justify-center rounded-xl" aria-hidden="true">
          <FontAwesomeIcon :icon="['fas', 'terminal']" class="size-5" />
        </div>
        <div class="min-w-0">
          <p class="page-header-section">{{ t('serialDebug.tool') }}</p>
          <h1 class="page-header-title mt-0.5">{{ t('serialDebug.pageTitle') }}</h1>
        </div>
        <div class="page-header-divider ml-1 hidden h-8 w-px shrink-0 sm:block" aria-hidden="true" />
        <p class="page-header-desc relative hidden max-w-[26rem] sm:block">
          <span class="font-semibold text-[var(--ty-text)]">{{ t('serialDebug.introLead') }}</span>{{ t('serialDebug.introRest') }}
        </p>
      </div>
    </header>
    <SerialDebugConnectionBar />
    <SerialDebugFilterBar />
    <SerialDebugLogPane class="flex-1 min-h-0" />
    <SerialDebugSendBar />
    <RxSelectionHexPopup />
  </div>
</template>
