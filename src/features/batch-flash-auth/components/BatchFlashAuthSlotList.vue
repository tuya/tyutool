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
  const s = store.slots.find((sl) => sl.port === port);
  if (s && s.status === "failed") {
    s.status = "idle";
    s.progress = 0;
    s.error = undefined;
  }
  await store.startBatch();
}
function onRemove(port: string) {
  store.removeSlot(port);
}
</script>

<template>
  <div
    class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-[var(--ty-border)]"
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
    <div v-if="store.slots.length > 0" class="min-h-0 flex-1 overflow-y-auto">
      <BatchFlashAuthSlotRow
        v-for="s in store.slots"
        :key="s.port"
        :portSlot="s"
        class="border-b border-[var(--ty-border)] last:border-b-0"
        @cancel="onCancel"
        @retry="onRetry"
        @remove="onRemove"
      />
    </div>

    <!-- Empty state -->
    <div
      v-else
      class="flex flex-1 flex-col items-center justify-center gap-2 py-10 text-[var(--ty-text-muted)]"
    >
      <FontAwesomeIcon :icon="['fas', 'plug']" class="text-3xl opacity-40" />
      <p class="text-sm">{{ t("batchFlashAuth.slot.empty") }}</p>
      <p class="text-xs">{{ t("batchFlashAuth.slot.emptyHint") }}</p>
    </div>
  </div>
</template>
