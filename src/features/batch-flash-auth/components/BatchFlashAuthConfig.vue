<!-- src/features/batch-flash/components/BatchFlashConfig.vue -->
<script setup lang="ts">
import { computed } from "vue";
import { isTauriRuntime } from "@/runtime";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import { BAUD_RATE_OPTIONS } from "@/features/firmware-flash/constants";
import { BATCH_AUTH_TOOL_CHIP_OPTIONS } from "../types";
import TySelect, { type TySelectOption } from "@/components/TySelect.vue";
import { useI18n } from "vue-i18n";

const store = useBatchFlashAuthStore();
const { t } = useI18n();

const chipOptions = computed<TySelectOption[]>(() =>
  (BATCH_AUTH_TOOL_CHIP_OPTIONS as readonly string[]).map((id) => ({
    value: id,
    label: t(`flash.chips.${id}`),
  })),
);

const baudOptions = computed<TySelectOption[]>(() =>
  (BAUD_RATE_OPTIONS as readonly number[]).map((b) => ({
    value: String(b),
    label: String(b),
  })),
);

const baudRateStr = computed({
  get: () => String(store.baudRate),
  set: (v: string) => {
    store.baudRate = Number(v);
  },
});

async function browseFirmware() {
  if (!isTauriRuntime()) return;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const file = await open({
    filters: [{ name: "Binary", extensions: ["bin"] }],
  });
  if (typeof file === "string") store.firmwarePath = file;
}
</script>

<template>
  <div
    class="rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3"
  >
    <h3 class="mb-3 text-sm font-semibold text-[var(--ty-text)]">共享配置</h3>
    <div class="flex flex-wrap gap-3">
      <!-- Chip selector -->
      <div class="flex min-w-[9rem] flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">芯片型号</label>
        <TySelect
          v-model="store.chipId"
          :options="chipOptions"
          :disabled="store.isBusy"
        />
      </div>

      <!-- Baud rate -->
      <div class="flex min-w-[8rem] flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">波特率</label>
        <TySelect
          v-model="baudRateStr"
          :options="baudOptions"
          :disabled="store.isBusy"
        />
      </div>

      <!-- Firmware file — only shown for flash-capable chips (not "other") -->
      <div
        v-if="store.canFlash"
        class="flex min-w-[16rem] flex-1 flex-col gap-1"
      >
        <label class="text-xs text-[var(--ty-text-muted)]">固件文件</label>
        <div class="flex gap-2">
          <input
            type="text"
            :value="store.firmwarePath"
            readonly
            :disabled="store.isBusy"
            placeholder="未选择文件"
            class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-2.5 h-[2.125rem] text-xs text-[var(--ty-text)] placeholder:text-[var(--ty-text-muted)]"
          />
          <button
            type="button"
            class="ops-browse-btn inline-flex h-[2.125rem] shrink-0 items-center gap-1.5 rounded-lg px-3 text-xs font-medium whitespace-nowrap"
            :disabled="store.isBusy"
            @click="browseFirmware"
          >
            <FontAwesomeIcon
              :icon="['fas', 'folder-open']"
              class="size-3.5"
              aria-hidden="true"
            />{{ t("batchFlashAuth.config.browse") }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
