<!-- src/features/batch-flash/components/BatchFlashSlotRow.vue -->
<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { showConfirmDialog } from "@/composables/confirmDialog";
import type { BatchSlotState } from "../types";

const props = defineProps<{ portSlot: BatchSlotState }>();
defineEmits<{
  cancel: [port: string];
  retry: [port: string];
  remove: [port: string];
}>();

const { t } = useI18n();

const STATUS_LABELS = computed(() => ({
  idle: t("batchFlashAuth.status.idle"),
  flashing: t("batchFlashAuth.status.flashing"),
  reading_mac: t("batchFlashAuth.status.reading_mac"),
  authorizing: t("batchFlashAuth.status.authorizing"),
  done: t("batchFlashAuth.status.done"),
  failed: t("batchFlashAuth.status.failed"),
  skipped: t("batchFlashAuth.status.skipped"),
}));

const statusLabel = computed(
  () => STATUS_LABELS.value[props.portSlot.status] ?? props.portSlot.status,
);

const STATUS_COLORS: Record<string, string> = {
  idle: "var(--ty-text-muted)",
  flashing: "var(--ty-primary)",
  reading_mac: "var(--ty-primary)",
  authorizing: "var(--ty-primary)",
  done: "var(--ty-success)",
  failed: "var(--ty-danger)",
  skipped: "var(--ty-text-muted)",
};
const statusColor = computed(() => STATUS_COLORS[props.portSlot.status]);

const BORDER_COLORS: Record<string, string> = {
  idle: "transparent",
  flashing: "var(--ty-primary)",
  reading_mac: "var(--ty-primary)",
  authorizing: "var(--ty-primary)",
  done: "var(--ty-success)",
  failed: "var(--ty-danger)",
  skipped: "var(--ty-text-muted)",
};
const borderColor = computed(() => BORDER_COLORS[props.portSlot.status]);

const rowBg = computed(() =>
  props.portSlot.status === "failed"
    ? "color-mix(in srgb, var(--ty-danger) 6%, transparent)"
    : "transparent",
);

const isActive = computed(() =>
  ["flashing", "reading_mac", "authorizing"].includes(props.portSlot.status),
);

const showProgress = computed(
  () => isActive.value && props.portSlot.progress > 0,
);

function showErrorDetail(): void {
  const error = props.portSlot.error;
  if (!error) return;
  void showConfirmDialog({
    title: `${props.portSlot.port} — ${t("batchFlashAuth.slot.errorTitle")}`,
    message: error,
    kind: "danger",
    showCancel: false,
    okLabel: t("common.closeDialog"),
    extraActionLabel: t("common.copy"),
    onExtraAction: () => navigator.clipboard?.writeText(error),
  });
}
</script>

<template>
  <div
    class="flex h-10 min-w-0 items-center gap-3 border-l-[3px] px-3 text-sm transition-colors"
    :style="{ borderLeftColor: borderColor, backgroundColor: rowBg }"
  >
    <!-- Port name -->
    <span class="w-20 shrink-0 font-mono text-xs text-[var(--ty-text)]">{{
      portSlot.port
    }}</span>

    <!-- Status label -->
    <span
      class="w-24 shrink-0 text-xs font-medium"
      :style="{ color: statusColor }"
    >
      <span
        v-if="isActive"
        class="mr-1 inline-block h-1.5 w-1.5 animate-pulse rounded-full"
        :style="{ backgroundColor: statusColor }"
      />
      {{ statusLabel }}
    </span>

    <!-- Progress bar + percent (active states) -->
    <div v-if="showProgress" class="flex min-w-0 flex-1 items-center gap-2">
      <div
        class="h-1 flex-1 overflow-hidden rounded-full"
        :style="{ backgroundColor: 'var(--ty-border)' }"
      >
        <div
          class="h-full rounded-full transition-all duration-200"
          :style="{
            width: portSlot.progress + '%',
            backgroundColor: 'var(--ty-primary)',
          }"
        />
      </div>
      <span class="w-9 shrink-0 text-right text-xs text-[var(--ty-text-muted)]">
        {{ portSlot.progress }}%
      </span>
    </div>

    <!-- Error summary (failed state) -->
    <div
      v-else-if="portSlot.status === 'failed' && portSlot.error"
      class="flex min-w-0 flex-1 items-center gap-1"
    >
      <span class="min-w-0 truncate text-xs text-[var(--ty-danger)]">{{
        portSlot.error
      }}</span>
      <button
        type="button"
        class="ml-1 flex size-5 shrink-0 cursor-pointer items-center justify-center rounded text-[var(--ty-text-muted)] transition-colors hover:bg-[var(--ty-surface-muted)] hover:text-[var(--ty-text)]"
        :title="t('batchFlashAuth.slot.viewError')"
        :aria-label="t('batchFlashAuth.slot.viewError')"
        @click="showErrorDetail"
      >
        <FontAwesomeIcon
          :icon="['fas', 'circle-info']"
          class="size-3.5"
          aria-hidden="true"
        />
      </button>
    </div>

    <div v-else class="flex-1" />

    <!-- Action buttons -->
    <div class="flex shrink-0 items-center gap-1">
      <button
        v-if="isActive"
        type="button"
        class="ty-btn-secondary min-h-7 px-2 py-0.5 text-xs"
        @click="$emit('cancel', portSlot.port)"
      >
        {{ t("batchFlashAuth.slot.cancel") }}
      </button>
      <button
        v-if="portSlot.status === 'failed'"
        type="button"
        class="ty-btn-secondary min-h-7 px-2 py-0.5 text-xs"
        @click="$emit('retry', portSlot.port)"
      >
        {{ t("batchFlashAuth.slot.retry") }}
      </button>
      <button
        v-if="
          portSlot.status === 'idle' ||
          portSlot.status === 'done' ||
          portSlot.status === 'skipped'
        "
        type="button"
        class="ty-btn-secondary min-h-7 cursor-pointer px-2 py-0.5 text-xs text-[var(--ty-text-muted)]"
        @click="$emit('remove', portSlot.port)"
      >
        {{ t("batchFlashAuth.slot.remove") }}
      </button>
    </div>
  </div>
</template>
