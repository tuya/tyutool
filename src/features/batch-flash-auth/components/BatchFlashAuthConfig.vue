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

const versionOptions = computed<TySelectOption[]>(() =>
  store.defaultFirmwareEntries.map((e) => ({
    value: e.version,
    label: e.notes ? `${e.version} — ${e.notes}` : e.version,
  })),
);

// v-model proxy: selecting a version triggers download.
const selectedVersion = computed({
  get: () => store.selectedDefaultVersion,
  set: (v: string) => {
    void store.downloadDefaultFirmware(v);
  },
});

async function onSelectSource(source: "local" | "default") {
  store.setFirmwareSource(source);
  if (source === "default" && store.defaultFirmwareEntries.length === 0) {
    await store.loadDefaultFirmwareList();
  }
}
</script>

<template>
  <div
    class="rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3"
  >
    <h3 class="mb-3 text-sm font-semibold text-[var(--ty-text)]">
      {{ t("batchFlashAuth.config.sharedConfig") }}
    </h3>
    <div class="flex flex-wrap gap-3">
      <!-- Chip selector -->
      <div class="flex min-w-[9rem] flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">{{
          t("batchFlashAuth.config.chip")
        }}</label>
        <TySelect
          v-model="store.chipId"
          :options="chipOptions"
          :disabled="store.isBusy"
        />
      </div>

      <!-- Baud rate -->
      <div class="flex min-w-[8rem] flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">{{
          t("batchFlashAuth.config.baud")
        }}</label>
        <TySelect
          v-model="baudRateStr"
          :options="baudOptions"
          :disabled="store.isBusy"
        />
      </div>

      <!-- Firmware — only for flash-capable chips (not "other") -->
      <div
        v-if="store.canFlash"
        class="flex min-w-[16rem] flex-1 flex-col gap-1"
      >
        <label class="text-xs text-[var(--ty-text-muted)]">{{
          t("batchFlashAuth.config.firmware")
        }}</label>

        <!-- Source toggle -->
        <div class="mb-1 flex gap-3 text-xs text-[var(--ty-text)]">
          <label class="inline-flex items-center gap-1.5">
            <input
              type="radio"
              :checked="store.firmwareSource === 'local'"
              :disabled="store.isBusy"
              @change="onSelectSource('local')"
            />
            {{ t("batchFlashAuth.config.sourceLocal") }}
          </label>
          <label class="inline-flex items-center gap-1.5">
            <input
              type="radio"
              :checked="store.firmwareSource === 'default'"
              :disabled="store.isBusy"
              @change="onSelectSource('default')"
            />
            {{ t("batchFlashAuth.config.sourceDefault") }}
          </label>
        </div>

        <!-- Local file picker -->
        <div v-if="store.firmwareSource === 'local'" class="flex gap-2">
          <input
            type="text"
            :value="store.firmwarePath"
            readonly
            :disabled="store.isBusy"
            :placeholder="t('batchFlashAuth.config.noFile')"
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

        <!-- Default firmware: version select + status -->
        <div v-else class="flex flex-col gap-1">
          <TySelect
            v-model="selectedVersion"
            :options="versionOptions"
            :placeholder="t('batchFlashAuth.config.selectVersion')"
            :disabled="
              store.isBusy || store.defaultFirmwareStatus === 'loading'
            "
          />
          <p class="text-xs text-[var(--ty-text-muted)]">
            <span v-if="store.defaultFirmwareStatus === 'loading'">{{
              t("batchFlashAuth.config.firmwareLoading")
            }}</span>
            <span v-else-if="store.defaultFirmwareStatus === 'downloading'">{{
              t("batchFlashAuth.config.firmwareDownloading")
            }}</span>
            <span
              v-else-if="store.defaultFirmwareStatus === 'ready'"
              class="text-[var(--ty-success,#16a34a)]"
              >{{ t("batchFlashAuth.config.firmwareReady") }}</span
            >
            <span
              v-else-if="store.defaultFirmwareStatus === 'error'"
              class="text-[var(--ty-danger,#dc2626)]"
              >{{
                (store.defaultFirmwareEntries.length === 0
                  ? t("batchFlashAuth.config.firmwareLoadFailed")
                  : t("batchFlashAuth.config.firmwareDownloadFailed")) +
                (store.defaultFirmwareError
                  ? `: ${store.defaultFirmwareError}`
                  : "")
              }}</span
            >
            <span v-else-if="versionOptions.length === 0">{{
              t("batchFlashAuth.config.firmwareNoVersions")
            }}</span>
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
