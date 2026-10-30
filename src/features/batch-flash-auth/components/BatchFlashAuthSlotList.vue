<!-- src/features/batch-flash/components/BatchFlashSlotList.vue -->
<script setup lang="ts">
import { computed } from "vue";
import { applyPortFilter } from "../port-filter";
import { usePortManagerStore } from "@/stores/port-manager";
import { useI18n } from "vue-i18n";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import BatchFlashAuthSlotRow from "./BatchFlashAuthSlotRow.vue";

const { t } = useI18n();
const store = useBatchFlashAuthStore();
const portManager = usePortManagerStore();
const allPortsFiltered = computed(
  () =>
    !!store.lastScannedPorts?.length &&
    applyPortFilter(store.lastScannedPorts, store.filterConfig.blockedPorts)
      .length === 0,
);
function retryReason(port: string): string | null {
  if (store.configurationDisabledReason)
    return t(store.configurationDisabledReason);
  const owner = portManager.currentOwner(port);
  return owner && owner !== "batch-auth"
    ? t("batchFlashAuth.errors.port.busyReason", { owner })
    : null;
}

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
    class="flex flex-col overflow-x-auto rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)]"
  >
    <p
      v-if="store.portsLoading"
      role="status"
      class="px-3 py-2 text-xs text-[var(--ty-text-muted)]"
    >
      {{ t("common.loading") }}
    </p>
    <p
      v-else-if="store.portsError"
      role="alert"
      class="px-3 py-2 text-xs text-[var(--ty-danger)]"
    >
      {{ t("serialDebug.conn.portsFailed", { msg: store.portsError }) }}
    </p>
    <div class="min-w-[38rem]">
      <!-- Header row -->
      <div
        class="flex h-8 items-center gap-3 border-b border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-3 text-xs font-medium text-[var(--ty-text-muted)]"
      >
        <span class="w-20 shrink-0">{{ t("batchFlashAuth.slot.port") }}</span>
        <span class="w-28 shrink-0">{{ t("batchFlashAuth.slot.status") }}</span>
        <span class="flex-1">{{ t("batchFlashAuth.slot.progress") }}</span>
        <span class="w-36 shrink-0 text-right">{{
          t("batchFlashAuth.slot.action")
        }}</span>
      </div>

      <!-- Slot rows -->
      <div
        v-if="store.slots.length > 0"
        aria-live="polite"
        aria-relevant="text"
      >
        <BatchFlashAuthSlotRow
          v-for="(s, index) in store.slots"
          :key="s.port"
          :portSlot="s"
          :rowIndex="index"
          :canRead="store.commandPrefixValid"
          :retry-disabled-reason="retryReason(s.port)"
          class="border-b border-[var(--ty-border)] last:border-b-0"
          @cancel="onCancel"
          @retry="onRetry"
          @remove="onRemove"
          @block="onBlock"
          @read="onRead"
        />
      </div>
    </div>
    <!-- Empty state -->
    <div
      v-if="store.slots.length === 0"
      class="flex flex-col items-center justify-center gap-2 py-10 text-[var(--ty-text-muted)]"
    >
      <FontAwesomeIcon :icon="['fas', 'plug']" class="text-3xl opacity-40" />
      <p class="text-sm">{{ t("batchFlashAuth.slot.empty") }}</p>
      <p class="text-xs px-3 text-center">
        {{
          store.portsError
            ? t("batchFlashAuth.slot.scanRetry")
            : allPortsFiltered
              ? t("batchFlashAuth.slot.emptyFiltered")
              : store.lastScannedPorts === null
                ? t("batchFlashAuth.slot.emptyHint")
                : t("batchFlashAuth.slot.emptyHintNoPorts")
        }}
      </p>
      <button
        type="button"
        class="ty-btn-secondary text-xs"
        :disabled="store.portsLoading"
        @click="store.autoAssign()"
      >
        {{ t("batchFlashAuth.toolbar.autoAssign") }}
      </button>
    </div>
  </div>
</template>
