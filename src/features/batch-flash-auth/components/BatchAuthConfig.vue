<!-- src/features/batch-flash/components/BatchAuthConfig.vue -->
<script setup lang="ts">
import { ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { isTauriRuntime } from "@/runtime";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import type { ExcelStats } from "../types";

const { t } = useI18n();
const store = useBatchFlashAuthStore();

const excelStats = ref<ExcelStats | null>(null);
const excelError = ref<string | null>(null);

async function browseExcel() {
  if (!isTauriRuntime()) return;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const file = await open({
    filters: [{ name: "Excel", extensions: ["xlsx"] }],
  });
  if (typeof file === "string") {
    store.authConfig.excelPath = file;
  }
}

async function validateExcel(path: string) {
  if (!path || !isTauriRuntime()) {
    excelStats.value = null;
    excelError.value = null;
    return;
  }
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const stats = await invoke<ExcelStats>("validate_excel_cmd", { path });
    excelStats.value = stats;
    excelError.value = null;
  } catch (e) {
    excelStats.value = null;
    excelError.value = String(e);
  }
}

watch(() => store.authConfig.excelPath, validateExcel, { immediate: true });
</script>

<template>
  <div
    class="rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3"
    style="border-left: 3px solid var(--ty-accent)"
  >
    <h3 class="mb-3 text-sm font-semibold text-[var(--ty-text)]">
      {{ t("batchFlashAuth.config.authTitle") }}
    </h3>
    <div class="flex flex-col gap-3">
      <!-- Excel file -->
      <div class="flex flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">{{
          t("batchFlashAuth.config.excelFile")
        }}</label>
        <div class="flex gap-2">
          <input
            type="text"
            :value="store.authConfig.excelPath"
            readonly
            :disabled="store.isBusy"
            :placeholder="t('batchFlashAuth.config.noExcel')"
            class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-2.5 h-[2.125rem] text-xs text-[var(--ty-text)] placeholder:text-[var(--ty-text-muted)]"
          />
          <button
            type="button"
            class="ops-browse-btn inline-flex h-[2.125rem] shrink-0 items-center gap-1.5 rounded-lg px-3 text-xs font-medium whitespace-nowrap"
            :disabled="store.isBusy"
            @click="browseExcel"
          >
            <FontAwesomeIcon
              :icon="['fas', 'folder-open']"
              class="size-3.5"
              aria-hidden="true"
            />{{ t("batchFlashAuth.config.browse") }}
          </button>
        </div>

        <!-- Validation feedback -->
        <div
          v-if="excelError"
          class="text-xs"
          :style="{ color: 'var(--ty-danger)' }"
        >
          {{ excelError }}
        </div>
        <div
          v-else-if="excelStats"
          class="flex flex-wrap items-center gap-3 text-xs"
        >
          <span class="text-[var(--ty-text-muted)]">
            {{ t("batchFlashAuth.config.excelTotal") }}
            <strong class="text-[var(--ty-text)]">{{
              excelStats.total
            }}</strong>
          </span>
          <span class="text-[var(--ty-text-muted)]">
            {{ t("batchFlashAuth.config.excelUsed") }}
            <strong class="text-[var(--ty-text)]">{{ excelStats.used }}</strong>
          </span>
          <span
            :style="{
              color:
                excelStats.remaining === 0
                  ? 'var(--ty-danger)'
                  : 'var(--ty-success)',
            }"
          >
            {{ t("batchFlashAuth.config.excelRemaining") }}
            <strong>{{ excelStats.remaining }}</strong>
          </span>
          <span
            v-if="excelStats.remaining === 0"
            class="flex items-center gap-1 font-medium"
            :style="{ color: 'var(--ty-accent)' }"
          >
            <FontAwesomeIcon
              :icon="['fas', 'triangle-exclamation']"
              class="size-3 shrink-0"
              aria-hidden="true"
            />
            {{ t("batchFlashAuth.config.excelExhausted") }}
          </span>
        </div>
      </div>

      <!-- Conflict policy + Storage mode (T5AI): same row when space allows, wraps otherwise -->
      <div class="flex flex-col gap-1.5">
        <div
          class="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-xs text-[var(--ty-text-muted)]"
        >
          <!-- Conflict policy group -->
          <div class="flex shrink-0 items-center gap-4">
            <span>{{ t("batchFlashAuth.config.conflictPolicy") }}：</span>
            <label class="flex cursor-pointer items-center gap-1">
              <input
                type="radio"
                v-model="store.authConfig.conflictPolicy"
                value="skip"
                :disabled="store.isBusy"
              />
              {{ t("batchFlashAuth.config.skip") }}
            </label>
            <label class="flex cursor-pointer items-center gap-1">
              <input
                type="radio"
                v-model="store.authConfig.conflictPolicy"
                value="overwrite"
                :disabled="store.isBusy"
              />
              {{ t("batchFlashAuth.config.overwrite") }}
            </label>
          </div>
          <!-- Storage mode group (T5AI only) -->
          <div
            v-if="store.chipId === 't5ai'"
            class="ml-auto flex shrink-0 items-center gap-4"
          >
            <span>{{ t("batchFlashAuth.config.storageMode") }}：</span>
            <label class="flex cursor-pointer items-center gap-1">
              <input
                type="radio"
                v-model="store.authConfig.authStorage"
                value="kv"
                :disabled="store.isBusy"
              />
              {{ t("batchFlashAuth.config.storageKv") }}
            </label>
            <label class="flex cursor-pointer items-center gap-1">
              <input
                type="radio"
                v-model="store.authConfig.authStorage"
                value="otp"
                :disabled="store.isBusy"
              />
              {{ t("batchFlashAuth.config.storageOtp") }}
            </label>
          </div>
        </div>
        <!-- OTP irreversibility warning -->
        <div
          v-if="
            store.chipId === 't5ai' && store.authConfig.authStorage === 'otp'
          "
          class="flex items-start gap-1.5 rounded-lg border border-[var(--ty-warning,#f59e0b)] bg-[color-mix(in_srgb,var(--ty-warning,#f59e0b)_10%,transparent)] px-2.5 py-2 text-xs"
          :style="{ color: 'var(--ty-warning, #f59e0b)' }"
        >
          <FontAwesomeIcon
            :icon="['fas', 'triangle-exclamation']"
            class="mt-0.5 size-3 shrink-0"
            aria-hidden="true"
          />
          <span>{{ t("batchFlashAuth.config.storageOtpWarning") }}</span>
        </div>
      </div>
    </div>
  </div>
</template>
