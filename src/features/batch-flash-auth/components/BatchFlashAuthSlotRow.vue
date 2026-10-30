<!-- src/features/batch-flash/components/BatchFlashSlotRow.vue -->
<script setup lang="ts">
import { computed, ref, nextTick, watch } from "vue";
import { useFloatingPanel } from "@/composables/useFloatingPanel";
import { useI18n } from "vue-i18n";
import { showConfirmDialog } from "@/composables/confirmDialog";
import type { BatchSlotState } from "../types";

defineOptions({ inheritAttrs: false });
const props = defineProps<{
  portSlot: BatchSlotState;
  rowIndex?: number;
  canRead?: boolean;
  retryDisabledReason?: string | null;
}>();
const emit = defineEmits<{
  cancel: [port: string];
  retry: [port: string];
  remove: [port: string];
  block: [port: string];
  read: [port: string];
}>();

const contextMenu = ref<{ x: number; y: number } | null>(null);
const rowRef = ref<HTMLElement | null>(null);
const menuRef = ref<HTMLElement | null>(null);
const { style: menuStyle } = useFloatingPanel(
  menuRef,
  () => contextMenu.value !== null,
  () =>
    contextMenu.value
      ? {
          left: contextMenu.value.x,
          right: contextMenu.value.x,
          top: contextMenu.value.y,
          bottom: contextMenu.value.y,
        }
      : null,
);
watch(contextMenu, async (menu) => {
  if (menu) {
    await nextTick();
    if (contextMenu.value)
      menuRef.value?.querySelector<HTMLButtonElement>("button")?.focus();
  }
});
function closeMenu(restore = false) {
  contextMenu.value = null;
  if (restore) rowRef.value?.focus();
}
function onContextMenu(e: MouseEvent) {
  e.preventDefault();
  contextMenu.value = { x: e.clientX, y: e.clientY };
}
function onRowKey(e: KeyboardEvent) {
  if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
    e.preventDefault();
    const rect = rowRef.value!.getBoundingClientRect();
    contextMenu.value = { x: rect.left + 12, y: rect.bottom };
  }
}
function onBlock() {
  closeMenu(true);
  emit("block", props.portSlot.port);
}

const { t } = useI18n();

const STATUS_LABELS = computed(() => ({
  idle: t("batchFlashAuth.status.idle"),
  reading: t("batchFlashAuth.status.reading"),
  flashing: t("batchFlashAuth.status.flashing"),
  reading_mac: t("batchFlashAuth.status.reading_mac"),
  authorizing: t("batchFlashAuth.status.authorizing"),
  done: t("batchFlashAuth.status.done"),
  failed: t("batchFlashAuth.status.failed"),
  skipped: t("batchFlashAuth.status.skipped"),
  no_code: t("batchFlashAuth.status.no_code"),
}));

// Maps flash sub-phase keys to a coarser status-level label shown in the badge.
const FLASH_PHASE_STATUS_LABELS = computed<Record<string, string>>(() => ({
  handshake: t("batchFlashAuth.status.connecting"),
  connect: t("batchFlashAuth.status.connecting"),
  switch_baud: t("batchFlashAuth.status.connecting"),
  read_flash_id: t("batchFlashAuth.status.connecting"),
  fetch_ram_loader: t("batchFlashAuth.status.connecting"),
  load_ram: t("batchFlashAuth.status.connecting"),
  erase: t("batchFlashAuth.status.erasing"),
  write: t("batchFlashAuth.status.writing"),
  write_segment: t("batchFlashAuth.status.writing"),
  verify: t("batchFlashAuth.status.verifying"),
}));

const isCancelledAfterWrite = computed(
  () => props.portSlot.cancelledAfterWrite === true,
);

const isWriteUncertain = computed(() => props.portSlot.writeUncertain === true);

const isQuarantineRequired = computed(
  () => isCancelledAfterWrite.value || isWriteUncertain.value,
);

const statusLabel = computed(() => {
  if (isQuarantineRequired.value) {
    return t("batchFlashAuth.slot.cancelledAfterWriteBadge");
  }
  if (props.portSlot.status === "flashing" && props.portSlot.currentPhase) {
    return (
      FLASH_PHASE_STATUS_LABELS.value[props.portSlot.currentPhase] ??
      STATUS_LABELS.value.flashing
    );
  }
  return STATUS_LABELS.value[props.portSlot.status] ?? props.portSlot.status;
});

const STATUS_COLORS: Record<string, string> = {
  idle: "var(--ty-text-muted)",
  reading: "var(--ty-primary)",
  flashing: "var(--ty-primary)",
  reading_mac: "var(--ty-primary)",
  authorizing: "var(--ty-primary)",
  done: "var(--ty-success)",
  failed: "var(--ty-danger)",
  skipped: "var(--ty-text-muted)",
  no_code: "var(--ty-warning)",
};
const statusColor = computed(() => STATUS_COLORS[props.portSlot.status]);

const BORDER_COLORS: Record<string, string> = {
  idle: "transparent",
  reading: "var(--ty-primary)",
  flashing: "var(--ty-primary)",
  reading_mac: "var(--ty-primary)",
  authorizing: "var(--ty-primary)",
  done: "var(--ty-success)",
  failed: "var(--ty-danger)",
  skipped: "var(--ty-text-muted)",
  no_code: "var(--ty-warning)",
};
const borderColor = computed(() => BORDER_COLORS[props.portSlot.status]);

const rowBg = computed(() => {
  const stripedRowBg =
    "color-mix(in srgb, var(--ty-primary) 10%, var(--ty-surface-muted))";
  const base =
    (props.rowIndex ?? 0) % 2 === 1 ? stripedRowBg : "var(--ty-surface)";
  if (isQuarantineRequired.value)
    return `color-mix(in srgb, var(--ty-danger) 14%, ${base})`;
  if (props.portSlot.status === "failed")
    return `color-mix(in srgb, var(--ty-danger) 8%, ${base})`;
  if (props.portSlot.status === "no_code")
    return `color-mix(in srgb, var(--ty-warning) 10%, ${base})`;
  return base;
});

const isActive = computed(() =>
  ["reading", "flashing", "reading_mac", "authorizing"].includes(
    props.portSlot.status,
  ),
);

const canRead = computed(() => !isActive.value);

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
    onExtraAction: () => navigator.clipboard.writeText(error),
  });
}

function showExcelError(): void {
  const error = props.portSlot.excelError;
  if (!error) return;
  void showConfirmDialog({
    title: `${props.portSlot.port} — ${t("batchFlashAuth.slot.excelErrorTitle")}`,
    message: error,
    kind: "warning",
    showCancel: false,
    okLabel: t("common.closeDialog"),
    extraActionLabel: t("common.copy"),
    onExtraAction: () => navigator.clipboard.writeText(error),
  });
}
</script>

<template>
  <!-- Right-click overlay to dismiss the context menu -->
  <Teleport to="body">
    <div
      v-if="contextMenu"
      class="fixed inset-0 z-40"
      @click="closeMenu()"
      @contextmenu.prevent="closeMenu()"
    />
    <div
      v-if="contextMenu"
      ref="menuRef"
      role="menu"
      :aria-label="`${t('batchFlashAuth.slot.action')}: ${portSlot.port}`"
      @keydown.esc.prevent.stop="closeMenu(true)"
      @focusout="
        (e) => {
          if (e.relatedTarget && !menuRef?.contains(e.relatedTarget as Node))
            closeMenu();
        }
      "
      class="fixed z-50 min-w-[10rem] overflow-y-auto rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface)] py-1 shadow-lg"
      :style="menuStyle"
      @click.stop
    >
      <button
        type="button"
        class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm text-[var(--ty-text)] hover:bg-[var(--ty-surface-muted)]"
        role="menuitem"
        @click="onBlock"
      >
        <FontAwesomeIcon
          :icon="['fas', 'ban']"
          class="size-3.5 text-[var(--ty-text-muted)]"
          aria-hidden="true"
        />
        {{ t("batchFlashAuth.slot.blockPort") }}
      </button>
    </div>
  </Teleport>

  <div
    ref="rowRef"
    :data-port="portSlot.port"
    tabindex="0"
    class="relative flex min-h-10 min-w-0 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-sm transition-colors"
    :class="[
      $attrs.class,
      isQuarantineRequired ? 'border-l-[4px]' : 'border-l-[3px]',
    ]"
    :style="{ borderLeftColor: borderColor, backgroundColor: rowBg }"
    @contextmenu="onContextMenu"
    @keydown="onRowKey"
  >
    <span
      v-if="isQuarantineRequired"
      class="absolute right-1 top-0.5 flex size-4 items-center justify-center"
      :style="{ color: 'var(--ty-danger)' }"
      :title="t('batchFlashAuth.slot.cancelledAfterWriteLabel')"
    >
      <FontAwesomeIcon
        :icon="['fas', 'triangle-exclamation']"
        class="size-3.5"
        aria-hidden="true"
      />
    </span>
    <!-- Port name -->
    <span
      class="w-20 shrink-0 truncate font-mono text-xs text-[var(--ty-text)]"
      :title="portSlot.port"
      >{{ portSlot.port }}</span
    >

    <!-- Status label -->
    <span
      class="w-28 shrink-0 text-xs font-medium leading-tight"
      :style="{ color: statusColor }"
    >
      <span
        v-if="isActive"
        class="mr-1 inline-block h-1.5 w-1.5 animate-pulse rounded-full"
        :style="{ backgroundColor: statusColor }"
      />
      {{ statusLabel }}
    </span>

    <!-- Progress bar + percent (flash stage with percentage) -->
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

    <!-- Active phase text (reading_mac / authorizing with no percentage) -->
    <div
      v-else-if="isActive && portSlot.currentPhase"
      class="flex min-w-0 flex-1 items-center"
    >
      <span class="truncate text-xs text-[var(--ty-text-muted)]">
        {{
          t(
            `batchFlashAuth.phase.${portSlot.currentPhase}`,
            portSlot.currentPhase,
          )
        }}
      </span>
    </div>

    <!-- no_code: show MAC if available, plus a hint -->
    <div
      v-else-if="portSlot.status === 'no_code'"
      class="flex min-w-0 flex-1 items-center gap-2"
    >
      <span class="truncate text-xs" :style="{ color: 'var(--ty-warning)' }">
        {{ t("batchFlashAuth.slot.noCode") }}
      </span>
      <span
        v-if="portSlot.mac"
        class="font-mono text-xs text-[var(--ty-text-muted)]"
        >{{ portSlot.mac }}</span
      >
    </div>

    <!-- Error summary (failed state) -->
    <div
      v-else-if="portSlot.status === 'failed' && portSlot.error"
      class="flex min-w-0 flex-1 flex-col justify-center gap-0.5"
    >
      <div class="flex min-w-0 items-center gap-1">
        <span class="min-w-0 truncate text-xs text-[var(--ty-danger)]">{{
          portSlot.error
        }}</span>
        <button
          type="button"
          class="ml-1 flex size-7 shrink-0 cursor-pointer items-center justify-center rounded text-[var(--ty-text-muted)] transition-colors hover:bg-[var(--ty-surface-muted)] hover:text-[var(--ty-text)]"
          :title="t('batchFlashAuth.slot.viewError')"
          :aria-label="`${t('batchFlashAuth.slot.viewError')}: ${portSlot.port}`"
          @click="showErrorDetail"
        >
          <FontAwesomeIcon
            :icon="['fas', 'circle-info']"
            class="size-3.5"
            aria-hidden="true"
          />
        </button>
      </div>
    </div>

    <!-- Unified result: done / skipped / idle-with-probe -->
    <div
      v-else-if="
        portSlot.mac ||
        portSlot.authUuid ||
        portSlot.isAuthorized === false ||
        portSlot.readError
      "
      class="flex min-w-0 flex-1 items-center gap-2"
    >
      <span
        v-if="portSlot.mac"
        class="shrink-0 font-mono text-xs text-[var(--ty-text-muted)]"
        >{{ portSlot.mac }}</span
      >
      <span
        v-if="portSlot.authUuid"
        class="min-w-0 truncate font-mono text-xs text-[var(--ty-text-muted)]"
        :title="portSlot.authUuid"
        >{{ portSlot.authUuid }}</span
      >
      <span
        v-else-if="portSlot.isAuthorized === false"
        class="shrink-0 text-xs text-[var(--ty-text-muted)]"
        >{{ t("batchFlashAuth.slot.notAuthorized") }}</span
      >
      <button
        v-if="portSlot.excelError"
        type="button"
        class="flex size-7 shrink-0 cursor-pointer items-center justify-center rounded text-[var(--ty-warning)] transition-colors hover:bg-[var(--ty-surface-muted)]"
        :title="t('batchFlashAuth.slot.excelErrorTitle')"
        :aria-label="`${t('batchFlashAuth.slot.excelErrorTitle')}: ${portSlot.port}`"
        @click="showExcelError"
      >
        <FontAwesomeIcon
          :icon="['fas', 'triangle-exclamation']"
          class="size-3"
          aria-hidden="true"
        />
      </button>
      <span
        v-if="portSlot.readError"
        class="min-w-0 truncate text-xs"
        :style="{ color: 'var(--ty-warning)' }"
        >{{ t("batchFlashAuth.slot.readError") }}</span
      >
    </div>

    <div v-else class="flex-1" />

    <!-- Action buttons -->
    <div class="flex w-36 shrink-0 items-center justify-end gap-1">
      <button
        v-if="canRead"
        type="button"
        class="ty-btn-secondary min-h-7 px-2 py-0.5 text-xs"
        :disabled="props.canRead === false"
        :title="
          props.canRead === false
            ? t('batchFlashAuth.config.invalidCommandPrefix')
            : t('batchFlashAuth.slot.readHint')
        "
        :aria-label="`${t('batchFlashAuth.slot.read')}: ${portSlot.port}`"
        @click="$emit('read', portSlot.port)"
      >
        {{ t("batchFlashAuth.slot.read") }}
      </button>
      <button
        v-if="isActive"
        type="button"
        class="ty-btn-secondary min-h-7 px-2 py-0.5 text-xs"
        :aria-label="`${t('batchFlashAuth.slot.cancel')}: ${portSlot.port}`"
        @click="$emit('cancel', portSlot.port)"
      >
        {{ t("batchFlashAuth.slot.cancel") }}
      </button>
      <button
        v-if="
          (portSlot.status === 'failed' || portSlot.status === 'no_code') &&
          !portSlot.cancelledAfterWrite &&
          !portSlot.writeUncertain
        "
        type="button"
        class="ty-btn-secondary min-h-7 px-2 py-0.5 text-xs"
        :disabled="!!retryDisabledReason"
        :title="retryDisabledReason || undefined"
        :aria-label="`${t('batchFlashAuth.slot.retry')}: ${portSlot.port}`"
        :aria-describedby="
          retryDisabledReason ? `retry-${rowIndex}-reason` : undefined
        "
        @click="$emit('retry', portSlot.port)"
      >
        {{ t("batchFlashAuth.slot.retry") }}
      </button>
    </div>
    <p
      v-if="isQuarantineRequired"
      class="w-full pl-0 text-xs font-medium leading-relaxed text-[var(--ty-danger)]"
    >
      {{ t("batchFlashAuth.slot.cancelledAfterWriteLabel") }}
    </p>
    <p
      v-else-if="
        retryDisabledReason &&
        (portSlot.status === 'failed' || portSlot.status === 'no_code')
      "
      :id="`retry-${rowIndex}-reason`"
      class="w-full text-xs text-[var(--ty-text-muted)]"
    >
      {{ retryDisabledReason }}
    </p>
  </div>
</template>
