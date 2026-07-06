<!-- src/features/batch-flash/components/BatchFlashSlotList.vue -->
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import BatchFlashAuthSlotRow from "./BatchFlashAuthSlotRow.vue";

const { t } = useI18n();
const store = useBatchFlashAuthStore();

async function onCancel(port: string) {
  await store.cancelPort(port);
}
async function onRetry(port: string) {
  await store.retryPort(port);
}
function onRemove(port: string) {
  store.removeSlot(port);
}
function onBlock(port: string) {
  store.blockPort(port);
}
async function onRead(port: string) {
  await store.readPort(port);
}
</script>

<template>
  <div
    class="flex flex-col overflow-hidden rounded-xl border border-[var(--ty-border)]"
  >
    <!-- Header row -->
    <div
      class="flex h-8 items-center gap-3 border-b border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-3 text-xs font-medium text-[var(--ty-text-muted)]"
    >
      <span class="w-20 shrink-0">{{ t("batchFlashAuth.slot.port") }}</span>
      <span class="w-24 shrink-0">{{ t("batchFlashAuth.slot.status") }}</span>
      <span class="flex-1">{{ t("batchFlashAuth.slot.progress") }}</span>
      <span class="shrink-0">{{ t("batchFlashAuth.slot.action") }}</span>
    </div>

    <!-- Slot rows -->
    <div v-if="store.slots.length > 0" aria-live="polite" aria-relevant="text">
      <BatchFlashAuthSlotRow
        v-for="(s, index) in store.slots"
        :key="s.port"
        :portSlot="s"
        :rowIndex="index"
        class="border-b border-[var(--ty-border)] last:border-b-0"
        @cancel="onCancel"
        @retry="onRetry"
        @remove="onRemove"
        @block="onBlock"
        @read="onRead"
      />
    </div>

    <!-- Empty state -->
    <div
      v-else
      class="flex flex-col items-center justify-center gap-2 py-10 text-[var(--ty-text-muted)]"
    >
      <FontAwesomeIcon :icon="['fas', 'plug']" class="text-3xl opacity-40" />
      <p class="text-sm">{{ t("batchFlashAuth.slot.empty") }}</p>
      <p class="text-xs">{{ t("batchFlashAuth.slot.emptyHintNoPorts") }}</p>
    </div>
  </div>
</template>
