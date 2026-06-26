<!-- src/features/batch-flash/components/BatchFlashDashboard.vue -->
<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from "vue";
import { useI18n } from "vue-i18n";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import BatchFlashAuthDonutChart from "./BatchFlashAuthDonutChart.vue";

const { t } = useI18n();
const store = useBatchFlashAuthStore();

// Elapsed time ticker
const elapsedDisplay = ref("--:--:--");
let ticker: ReturnType<typeof setInterval> | undefined;

function formatElapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return [h, m, sec].map((n) => String(n).padStart(2, "0")).join(":");
}

onMounted(() => {
  ticker = setInterval(() => {
    if (store.batchStartTime !== null) {
      const end = store.batchEndTime ?? Date.now();
      elapsedDisplay.value = formatElapsed(end - store.batchStartTime);
    }
  }, 1000);
});
onUnmounted(() => clearInterval(ticker));

const bannerBg = computed(() => {
  const k = store.completionBanner?.kind;
  if (k === "all-success")
    return "color-mix(in srgb, var(--ty-success) 10%, transparent)";
  if (k === "all-failed")
    return "color-mix(in srgb, var(--ty-danger) 10%, transparent)";
  return "color-mix(in srgb, var(--ty-accent) 10%, transparent)";
});

const bannerTextColor = computed(() => {
  const k = store.completionBanner?.kind;
  if (k === "all-success") return "var(--ty-success)";
  if (k === "all-failed") return "var(--ty-danger)";
  return "var(--ty-accent)";
});

const bannerText = computed(() => {
  const b = store.completionBanner;
  if (!b) return "";
  switch (b.kind) {
    case "all-skipped":
      return t("batchFlashAuth.completion.allSkipped", { count: b.count });
    case "all-success":
      return t("batchFlashAuth.completion.allSuccess", { count: b.count });
    case "all-failed":
      return t("batchFlashAuth.completion.allFailed");
    case "partial":
      return t("batchFlashAuth.completion.partial", {
        done: b.done,
        failed: b.failed,
      });
  }
});
</script>

<template>
  <div class="flex flex-col gap-2">
    <!-- Completion banner -->
    <div
      v-if="store.completionBanner"
      class="flex items-center justify-between rounded-lg px-3 py-2 text-sm font-medium"
      :style="{ backgroundColor: bannerBg, color: bannerTextColor }"
    >
      <span>{{ bannerText }}</span>
      <button
        type="button"
        class="ml-3 flex size-6 shrink-0 cursor-pointer items-center justify-center rounded opacity-60 transition-opacity hover:opacity-100"
        :aria-label="t('common.closeDialog')"
        @click="store.dismissBanner()"
      >
        <FontAwesomeIcon
          :icon="['fas', 'xmark']"
          class="size-3.5"
          aria-hidden="true"
        />
      </button>
    </div>

    <!-- Stats row -->
    <div class="flex flex-wrap gap-3">
      <!-- Flash cumulative stats -->
      <div
        class="flex min-w-0 flex-1 items-center gap-4 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3"
      >
        <BatchFlashAuthDonutChart
          :total="store.cumulativeStats.flash.total"
          :success="store.cumulativeStats.flash.success"
          :fail="store.cumulativeStats.flash.fail"
        />
        <div class="flex min-w-0 flex-1 flex-col gap-1 text-sm">
          <div class="flex items-center justify-between">
            <span class="font-medium text-[var(--ty-text)]">{{
              t("batchFlashAuth.dashboard.flashStats")
            }}</span>
            <button
              type="button"
              class="cursor-pointer text-xs text-[var(--ty-danger)] hover:underline"
              @click="store.resetFlashStats()"
            >
              {{ t("batchFlashAuth.dashboard.reset") }}
            </button>
          </div>
          <div class="flex gap-4 text-xs text-[var(--ty-text-muted)]">
            <span
              >{{ t("batchFlashAuth.dashboard.total") }}
              <strong class="text-[var(--ty-text)]">{{
                store.cumulativeStats.flash.total
              }}</strong></span
            >
            <span :style="{ color: 'var(--ty-success)' }"
              ><FontAwesomeIcon
                :icon="['fas', 'circle-check']"
                class="mr-0.5"
              />{{ store.cumulativeStats.flash.success }}</span
            >
            <span :style="{ color: 'var(--ty-danger)' }"
              ><FontAwesomeIcon
                :icon="['fas', 'circle-xmark']"
                class="mr-0.5"
              />{{ store.cumulativeStats.flash.fail }}</span
            >
          </div>
        </div>
      </div>

      <!-- Auth cumulative stats -->
      <div
        class="flex min-w-0 flex-1 items-center gap-4 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3"
      >
        <BatchFlashAuthDonutChart
          :total="store.cumulativeStats.auth.total"
          :success="store.cumulativeStats.auth.success"
          :fail="store.cumulativeStats.auth.fail"
        />
        <div class="flex min-w-0 flex-1 flex-col gap-1 text-sm">
          <div class="flex items-center justify-between">
            <span class="font-medium text-[var(--ty-text)]">{{
              t("batchFlashAuth.dashboard.authStats")
            }}</span>
            <button
              type="button"
              class="cursor-pointer text-xs text-[var(--ty-danger)] hover:underline"
              @click="store.resetAuthStats()"
            >
              {{ t("batchFlashAuth.dashboard.reset") }}
            </button>
          </div>
          <div class="flex gap-4 text-xs text-[var(--ty-text-muted)]">
            <span
              >{{ t("batchFlashAuth.dashboard.total") }}
              <strong class="text-[var(--ty-text)]">{{
                store.cumulativeStats.auth.total
              }}</strong></span
            >
            <span :style="{ color: 'var(--ty-success)' }"
              ><FontAwesomeIcon
                :icon="['fas', 'circle-check']"
                class="mr-0.5"
              />{{ store.cumulativeStats.auth.success }}</span
            >
            <span :style="{ color: 'var(--ty-danger)' }"
              ><FontAwesomeIcon
                :icon="['fas', 'circle-xmark']"
                class="mr-0.5"
              />{{ store.cumulativeStats.auth.fail }}</span
            >
          </div>
        </div>
      </div>

      <!-- Current batch stats -->
      <div
        class="flex min-w-0 flex-1 flex-col justify-center gap-1.5 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3 text-sm"
      >
        <span class="font-medium text-[var(--ty-text)]">{{
          t("batchFlashAuth.dashboard.currentBatch")
        }}</span>
        <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
          <span class="text-[var(--ty-primary)]"
            >{{ t("batchFlashAuth.dashboard.active") }}
            {{ store.currentStats.active }}</span
          >
          <span :style="{ color: 'var(--ty-success)' }"
            ><FontAwesomeIcon
              :icon="['fas', 'circle-check']"
              class="mr-0.5"
            />{{ t("batchFlashAuth.dashboard.success") }}
            {{ store.currentStats.done }}</span
          >
          <span :style="{ color: 'var(--ty-danger)' }"
            ><FontAwesomeIcon
              :icon="['fas', 'circle-xmark']"
              class="mr-0.5"
            />{{ t("batchFlashAuth.dashboard.fail") }}
            {{ store.currentStats.failed }}</span
          >
          <span class="text-[var(--ty-text-muted)]"
            ><FontAwesomeIcon :icon="['fas', 'clock']" class="mr-0.5" />{{
              t("batchFlashAuth.dashboard.elapsed")
            }}
            {{ elapsedDisplay }}</span
          >
        </div>
      </div>
    </div>
  </div>
</template>
