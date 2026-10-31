<script setup lang="ts">
import { computed, ref, watch, nextTick } from "vue";
import { useI18n } from "vue-i18n";
import { useFirmwareFlashContext } from "../context";
import { PHASE_STYLES } from "../phase-styles";
import { useCopyFeedback } from "@/composables/useCopyFeedback";
import { isTauriRuntime } from "@/runtime";
import { formatDuration } from "../utils";
import type { OpKind } from "../types";

const { t } = useI18n();
const ctx = useFirmwareFlashContext();

// Only destructure actions — ref state accessed via ctx.xxx for reactivity.
const { clearLogs, copyLogs, resetFlash } = ctx;
const emit = defineEmits<{ configure: [op: OpKind] }>();
const hasResult = computed(
  () => ctx.flashPhase === "success" || ctx.flashPhase === "error",
);
const errorCopy = useCopyFeedback();
const pathCopy = useCopyFeedback();
const pathsExpanded = ref(false);
watch(
  () => ctx.operationResult,
  () => {
    pathsExpanded.value = false;
    pathCopy.reset();
  },
);
watch(() => [ctx.operationResult, ctx.flashMessage], errorCopy.reset);
const resultStatus = computed(
  () =>
    ctx.operationResult?.status ??
    (ctx.flashPhase === "success" ? "success" : "error"),
);
const resultOperation = computed(() =>
  ctx.operationResult?.authRead
    ? t("flash.result.authRead")
    : ctx.operationResult
      ? t(`flash.tabs.${ctx.operationResult.op}`)
      : "",
);
const resultDuration = computed(() =>
  ctx.operationResult?.elapsedSecs != null
    ? formatDuration(ctx.operationResult.elapsedSecs * 1000)
    : "",
);
const canOpenOutput = computed(
  () =>
    isTauriRuntime() &&
    ctx.operationResult?.op === "read" &&
    ctx.operationResult.status === "success" &&
    !!ctx.operationResult.outputPath,
);

// Prevent progress bar width from animating backwards when phase resets to 0
const skipWidthTransition = ref(false);
watch(
  () => ctx.currentBackendPhase,
  (newPhase) => {
    if (newPhase === null) return;
    skipWidthTransition.value = true;
    nextTick(() => {
      skipWidthTransition.value = false;
    });
  },
);
</script>

<template>
  <aside
    class="flex min-h-0 w-full flex-1 flex-col gap-3"
    :aria-label="t('flash.asideAria')"
  >
    <section
      v-if="hasResult"
      class="flash-result aside-alert flex min-h-0 shrink-0 flex-col overflow-hidden rounded-xl p-3 text-xs lg:max-h-[55%]"
      :class="
        resultStatus === 'success'
          ? 'aside-alert-success'
          : resultStatus === 'cancelled'
            ? 'aside-alert-cancelled'
            : 'aside-alert-error'
      "
      :role="resultStatus === 'error' ? 'alert' : 'status'"
    >
      <div class="flex shrink-0 flex-wrap items-center gap-2">
        <FontAwesomeIcon
          :icon="[
            'fas',
            resultStatus === 'success'
              ? 'circle-check'
              : resultStatus === 'cancelled'
                ? 'circle-info'
                : 'circle-exclamation',
          ]"
          class="size-4 shrink-0"
          aria-hidden="true"
        />
        <h2 class="min-w-0 flex-1 text-sm font-semibold">
          {{ t(`flash.result.${resultStatus}`) }}
        </h2>
        <p v-if="resultOperation" class="text-[var(--ty-text-muted)]">
          {{ resultOperation
          }}<span v-if="resultDuration" class="tabular-nums">
            · {{ resultDuration }}</span
          >
        </p>
      </div>
      <div
        class="min-h-0 overflow-auto overscroll-contain"
        tabindex="0"
        :aria-label="t('flash.result.details')"
      >
        <dl
          v-if="ctx.operationResult"
          class="mt-2 grid gap-x-3 gap-y-2 text-[var(--ty-text)]"
          :class="pathsExpanded ? 'grid-cols-2' : 'grid-cols-3'"
        >
          <div class="min-w-0">
            <dt class="text-[var(--ty-text-muted)]">{{ t("flash.chip") }}</dt>
            <dd class="mt-0.5 font-medium">
              {{ t(`flash.chips.${ctx.operationResult.chipId}`) }}
            </dd>
          </div>
          <div
            class="min-w-0"
            :class="pathsExpanded ? 'col-span-2 order-2' : ''"
          >
            <dt class="text-[var(--ty-text-muted)]">
              {{ t("serialDebug.conn.port") }}
            </dt>
            <dd
              class="mt-0.5 font-mono"
              :class="pathsExpanded ? 'break-all' : 'truncate'"
              :title="ctx.operationResult.port"
            >
              {{ ctx.operationResult.port }}
            </dd>
          </div>
          <div class="min-w-0">
            <dt class="text-[var(--ty-text-muted)]">{{ t("flash.baud") }}</dt>
            <dd class="mt-0.5 tabular-nums">{{ ctx.operationResult.baud }}</dd>
          </div>
          <div
            v-if="ctx.operationResult.outputPath"
            class="grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-start gap-2"
            :class="pathsExpanded ? 'col-span-2 order-3' : 'col-span-3'"
          >
            <dt class="text-[var(--ty-text-muted)]">
              {{ t("flash.result.outputFile") }}
            </dt>
            <dd
              class="select-text font-mono"
              :class="pathsExpanded ? 'break-all' : 'truncate'"
              :title="ctx.operationResult.outputPath"
            >
              {{ ctx.operationResult.outputPath }}
            </dd>
          </div>
        </dl>
        <p class="result-message mt-2 leading-relaxed">
          {{ ctx.operationResult?.message ?? ctx.flashMessage }}
        </p>
        <p
          v-if="
            ctx.operationResult?.op === 'read' &&
            resultStatus === 'success' &&
            !isTauriRuntime()
          "
          class="mt-1 text-[var(--ty-text-muted)]"
        >
          {{ t("flash.result.browserDownload") }}
        </p>
        <p v-if="ctx.readOutputError" class="mt-2 break-words" role="alert">
          {{ ctx.readOutputError }}
        </p>
      </div>
      <div class="mt-2 flex shrink-0 flex-wrap gap-2">
        <button
          v-if="ctx.operationResult"
          type="button"
          class="ty-btn-secondary min-h-8 rounded-lg px-2.5 py-1.5 font-medium"
          :aria-expanded="pathsExpanded"
          @click="pathsExpanded = !pathsExpanded"
        >
          {{
            pathsExpanded
              ? t("flash.result.collapsePaths")
              : t("flash.result.expandPaths")
          }}
        </button>
        <button
          v-if="ctx.operationResult?.outputPath"
          type="button"
          class="ty-btn-secondary min-h-8 rounded-lg px-2.5 py-1.5 font-medium"
          :aria-label="t('flash.result.copyPath')"
          :disabled="pathCopy.state.value === 'copying'"
          @click="pathCopy.copy(ctx.operationResult.outputPath)"
        >
          {{
            pathCopy.state.value === "copied"
              ? t("common.copied")
              : t("flash.result.copyPath")
          }}
        </button>
        <button
          v-if="canOpenOutput"
          type="button"
          class="ty-btn-primary min-h-8 rounded-lg px-2.5 py-1.5 font-medium"
          :disabled="ctx.readOutputOpening"
          :aria-busy="ctx.readOutputOpening"
          @click="ctx.openReadOutput"
        >
          {{
            t(
              ctx.readOutputOpening
                ? "flash.result.opening"
                : "flash.result.openOutput",
            )
          }}
        </button>
        <button
          type="button"
          class="ty-btn-secondary min-h-8 rounded-lg px-2.5 py-1.5 font-medium"
          @click="emit('configure', ctx.operationResult?.op ?? ctx.activeTab)"
        >
          {{ t("flash.result.configure") }}
        </button>
        <button
          v-if="resultStatus === 'error'"
          type="button"
          class="ty-btn-secondary min-h-8 rounded-lg px-2.5 py-1.5 font-medium"
          :aria-label="t('flash.result.copyError')"
          :disabled="errorCopy.state.value === 'copying'"
          @click="
            errorCopy.copy(ctx.operationResult?.message ?? ctx.flashMessage)
          "
        >
          {{
            errorCopy.state.value === "copied"
              ? t("common.copied")
              : t("flash.result.copyError")
          }}
        </button>
        <button
          type="button"
          class="ty-btn-secondary min-h-8 rounded-lg px-2.5 py-1.5 font-medium"
          @click="resetFlash"
        >
          {{ t("flash.clearResult") }}
        </button>
      </div>
      <p v-if="errorCopy.message.value" class="mt-2" role="status">
        {{ errorCopy.message.value }}
      </p>
      <p
        v-if="pathCopy.message.value"
        :class="pathCopy.state.value === 'copied' ? 'sr-only' : 'mt-2 shrink-0'"
        role="status"
      >
        {{ pathCopy.message.value }}
      </p>
    </section>

    <!-- 进度条 -->
    <section
      v-if="ctx.flashPhase === 'running'"
      class="aside-progress-card min-w-0 shrink-0 rounded-xl p-3"
      :aria-label="ctx.progressCaption"
    >
      <div class="mb-2 flex items-center justify-between text-xs">
        <!-- 阶段名 + 活跃点 -->
        <span
          class="flex items-center gap-1.5 font-medium text-[var(--ty-text-muted)]"
        >
          <span
            v-if="
              ctx.currentBackendPhase && PHASE_STYLES[ctx.currentBackendPhase]
            "
            class="aside-progress-dot"
            :style="{
              background: PHASE_STYLES[ctx.currentBackendPhase].glowColor,
            }"
          />
          {{
            ctx.currentBackendPhase && PHASE_STYLES[ctx.currentBackendPhase]
              ? t(PHASE_STYLES[ctx.currentBackendPhase].labelKey)
              : ctx.progressCaption
          }}
        </span>
        <span
          v-if="!ctx.phaseIndeterminate"
          class="aside-progress-pct tabular-nums font-bold"
          :style="
            ctx.currentBackendPhase && PHASE_STYLES[ctx.currentBackendPhase]
              ? { color: PHASE_STYLES[ctx.currentBackendPhase].glowColor }
              : {}
          "
        >
          {{ ctx.phaseProgress }}%
        </span>
      </div>
      <div
        class="aside-progress-track h-1.5 overflow-hidden rounded-full"
        role="progressbar"
        :aria-busy="ctx.phaseIndeterminate"
        :aria-valuenow="ctx.phaseIndeterminate ? undefined : ctx.phaseProgress"
        aria-valuemin="0"
        aria-valuemax="100"
      >
        <div
          v-if="!ctx.phaseIndeterminate"
          class="aside-progress-fill relative h-full overflow-hidden rounded-full ease-out"
          :class="
            skipWidthTransition
              ? 'transition-[background-color,box-shadow] duration-300'
              : 'transition-[width,background-color,box-shadow] duration-300'
          "
          :style="{
            width: `${ctx.phaseProgress}%`,
            background: 'var(--ty-primary)',
          }"
        ></div>
        <div
          v-else
          class="aside-progress-indeterminate h-full w-full rounded-full"
        />
      </div>
    </section>

    <!-- 输出日志 -->
    <section
      class="aside-log-card flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-xl"
      :class="hasResult ? 'lg:min-h-[8rem]' : 'lg:min-h-[11rem]'"
      :aria-label="t('flash.logPanelAria')"
    >
      <!-- 日志头部工具栏 -->
      <div
        class="aside-log-header flex min-w-0 shrink-0 flex-wrap items-center justify-between gap-x-3 gap-y-2 px-3 py-3"
      >
        <div
          class="flex min-w-0 items-center gap-1.5 text-sm font-semibold text-[var(--ty-text)]"
        >
          <FontAwesomeIcon
            :icon="['fas', 'scroll']"
            class="size-3.5 shrink-0 text-[var(--ty-primary)]"
            aria-hidden="true"
          />
          {{ t("flash.logTitle") }}
        </div>
        <div class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
          <label
            class="flex cursor-pointer items-center gap-1.5 text-xs text-[var(--ty-text-muted)]"
          >
            <input
              v-model="ctx.lockAutoScroll"
              type="checkbox"
              class="size-3 rounded border-[var(--ty-border)]"
            />
            {{ t("flash.lockScroll") }}
          </label>
          <button
            type="button"
            class="aside-log-btn inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-1 text-xs font-medium"
            :aria-label="t('flash.ariaClearLog')"
            @click="clearLogs"
          >
            <FontAwesomeIcon
              :icon="['fas', 'trash']"
              class="size-3"
              aria-hidden="true"
            />
            {{ t("flash.clearLog") }}
          </button>
          <button
            type="button"
            class="aside-log-btn inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-1 text-xs font-medium"
            :aria-label="t('flash.ariaCopyLog')"
            :disabled="ctx.logCopyState === 'copying'"
            :aria-busy="ctx.logCopyState === 'copying'"
            @click="copyLogs"
          >
            <FontAwesomeIcon
              :icon="['fas', 'copy']"
              class="size-3"
              aria-hidden="true"
            />
            {{
              ctx.logCopyState === "copied"
                ? t("common.copied")
                : t("flash.copyLog")
            }}
          </button>
        </div>
        <p
          v-if="ctx.logCopyMessage"
          class="log-copy-feedback w-full text-xs text-[var(--ty-text-muted)]"
          role="status"
        >
          {{ ctx.logCopyMessage }}
        </p>
      </div>

      <!-- 日志内容 -->
      <div
        :ref="
          (el) => {
            ctx.logScrollRef = el as HTMLDivElement | null;
          }
        "
        class="aside-log-body min-h-[4.5rem] min-w-0 flex-1 overflow-auto overscroll-contain p-3 font-mono text-xs leading-relaxed lg:min-h-0 select-text"
        role="log"
        aria-live="polite"
        aria-relevant="additions"
      >
        <div v-for="(line, i) in ctx.logLines" :key="i" class="aside-log-line">
          {{ line }}
        </div>
      </div>
    </section>
  </aside>
</template>
