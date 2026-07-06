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

const authBaudRateStr = computed({
  get: () => String(store.authBaudRate),
  set: (v: string) => {
    store.authBaudRate = Number(v);
  },
});

const firmwareControlsDisabled = computed(
  () => store.isBusy || !store.flashFirmware,
);

async function browseFirmware() {
  if (firmwareControlsDisabled.value) return;
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
  if (firmwareControlsDisabled.value) return;
  store.setFirmwareSource(source);
  if (source === "default" && store.defaultFirmwareEntries.length === 0) {
    await store.loadDefaultFirmwareList();
  }
}

function toggleFlashFirmware() {
  if (store.isBusy) return;
  store.flashFirmware = !store.flashFirmware;
}
</script>

<template>
  <div
    class="rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3"
  >
    <h3 class="mb-3 text-sm font-semibold text-[var(--ty-text)]">
      {{ t("batchFlashAuth.config.sharedConfig") }}
    </h3>
    <div class="space-y-3">
      <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <!-- Chip selector -->
        <div class="flex min-w-0 flex-col gap-1">
          <label class="text-xs text-[var(--ty-text-muted)]">{{
            t("batchFlashAuth.config.chip")
          }}</label>
          <TySelect
            v-model="store.chipId"
            :options="chipOptions"
            :disabled="store.isBusy"
          />
        </div>

        <!-- Flash baud rate -->
        <div class="flex min-w-0 flex-col gap-1">
          <label class="text-xs text-[var(--ty-text-muted)]">{{
            t("batchFlashAuth.config.flashBaud")
          }}</label>
          <TySelect
            v-model="baudRateStr"
            :options="baudOptions"
            :disabled="store.isBusy"
          />
        </div>

        <!-- Auth baud rate -->
        <div class="flex min-w-0 flex-col gap-1">
          <label class="text-xs text-[var(--ty-text-muted)]">{{
            t("batchFlashAuth.config.authBaud")
          }}</label>
          <TySelect
            v-model="authBaudRateStr"
            :options="baudOptions"
            :disabled="store.isBusy"
          />
        </div>

        <!-- Firmware enable switch -->
        <div v-if="store.canFlash" class="flex min-w-0 flex-col gap-1">
          <span class="text-xs text-[var(--ty-text-muted)]">{{
            t("batchFlashAuth.config.flashFirmware")
          }}</span>
          <button
            type="button"
            class="inline-flex h-[2.125rem] w-fit cursor-pointer items-center gap-2 rounded-lg px-0 text-xs font-medium text-[var(--ty-text)] transition-opacity focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ty-primary)]"
            :class="store.isBusy ? 'cursor-not-allowed opacity-60' : undefined"
            :style="{
              color: store.flashFirmware
                ? 'var(--ty-primary, #2563eb)'
                : 'var(--ty-text)',
            }"
            role="switch"
            :aria-checked="store.flashFirmware"
            :disabled="store.isBusy"
            @click="toggleFlashFirmware"
          >
            <span
              class="relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors duration-200"
              :style="{
                backgroundColor: store.flashFirmware
                  ? 'var(--ty-primary, #2563eb)'
                  : 'color-mix(in srgb, var(--ty-text-muted) 28%, var(--ty-surface-muted))',
              }"
              aria-hidden="true"
            >
              <span
                class="absolute left-0.5 size-4 rounded-full shadow-sm transition-transform duration-200"
                :class="store.flashFirmware ? 'translate-x-4' : ''"
                :style="{
                  backgroundColor: '#fff',
                }"
              />
            </span>
            <span class="leading-none">{{
              store.flashFirmware
                ? t("batchFlashAuth.config.flashFirmwareOn")
                : t("batchFlashAuth.config.flashFirmwareOff")
            }}</span>
          </button>
        </div>
      </div>

      <!-- Firmware — only for flash-capable chips (not "other") -->
      <div
        v-if="store.canFlash && store.flashFirmware"
        class="grid gap-3 border-t border-[var(--ty-border)] pt-3 lg:grid-cols-[12rem_minmax(0,1fr)]"
      >
        <!-- Source toggle -->
        <div class="flex min-w-0 flex-col gap-1">
          <span class="text-xs text-[var(--ty-text-muted)]">{{
            t("batchFlashAuth.config.firmware")
          }}</span>
          <div
            class="flex min-h-[2.125rem] flex-col justify-center gap-1 text-xs text-[var(--ty-text)]"
          >
            <label class="inline-flex items-center gap-1.5">
              <input
                type="radio"
                :checked="store.firmwareSource === 'local'"
                :disabled="firmwareControlsDisabled"
                @change="onSelectSource('local')"
              />
              {{ t("batchFlashAuth.config.sourceLocal") }}
            </label>
            <label class="inline-flex items-center gap-1.5">
              <input
                type="radio"
                :checked="store.firmwareSource === 'default'"
                :disabled="firmwareControlsDisabled"
                @change="onSelectSource('default')"
              />
              {{ t("batchFlashAuth.config.sourceDefault") }}
            </label>
          </div>
        </div>

        <!-- Local file picker -->
        <div
          v-if="store.firmwareSource === 'local'"
          class="flex min-w-0 flex-col gap-1"
        >
          <span class="text-xs text-[var(--ty-text-muted)]">{{
            t("batchFlashAuth.config.sourceLocal")
          }}</span>
          <div class="flex min-w-0 gap-2">
            <input
              type="text"
              :value="store.firmwarePath"
              readonly
              :disabled="firmwareControlsDisabled"
              :placeholder="t('batchFlashAuth.config.noFile')"
              class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-2.5 h-[2.125rem] text-xs text-[var(--ty-text)] placeholder:text-[var(--ty-text-muted)]"
            />
            <button
              type="button"
              class="ops-browse-btn inline-flex h-[2.125rem] shrink-0 items-center gap-1.5 rounded-lg px-3 text-xs font-medium whitespace-nowrap"
              :disabled="firmwareControlsDisabled"
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

        <!-- Default firmware: version select + status -->
        <div v-else class="flex flex-col gap-1">
          <span class="text-xs text-[var(--ty-text-muted)]">{{
            t("batchFlashAuth.config.sourceDefault")
          }}</span>
          <TySelect
            v-model="selectedVersion"
            :options="versionOptions"
            :placeholder="t('batchFlashAuth.config.selectVersion')"
            :disabled="
              firmwareControlsDisabled ||
              store.defaultFirmwareStatus === 'loading' ||
              (store.defaultFirmwareStatus === 'error' &&
                store.defaultFirmwareEntries.length === 0)
            "
          />
          <p class="text-xs text-[var(--ty-text-muted)]">
            <span v-if="store.defaultFirmwareStatus === 'loading'">{{
              t("batchFlashAuth.config.firmwareLoading")
            }}</span>
            <span v-else-if="store.defaultFirmwareStatus === 'downloading'">{{
              store.firmwareDownloadProgress !== null
                ? t("batchFlashAuth.config.firmwareDownloadingPct", {
                    pct: store.firmwareDownloadProgress,
                  })
                : t("batchFlashAuth.config.firmwareDownloading")
            }}</span>
            <span
              v-else-if="store.defaultFirmwareStatus === 'ready'"
              class="text-[var(--ty-success,#16a34a)]"
              >{{ t("batchFlashAuth.config.firmwareReady") }}</span
            >
            <template v-else-if="store.defaultFirmwareStatus === 'error'">
              <span class="text-[var(--ty-danger,#dc2626)]">{{
                (store.defaultFirmwareEntries.length === 0
                  ? t("batchFlashAuth.config.firmwareLoadFailed")
                  : t("batchFlashAuth.config.firmwareDownloadFailed")) +
                (store.defaultFirmwareError
                  ? `: ${store.defaultFirmwareError}`
                  : "")
              }}</span>
              <span
                v-if="store.defaultFirmwareEntries.length === 0"
                class="ml-2"
                >{{ t("batchFlashAuth.config.firmwareNetworkHint") }}</span
              >
            </template>
            <span v-else-if="versionOptions.length === 0">{{
              t("batchFlashAuth.config.firmwareNoVersions")
            }}</span>
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
