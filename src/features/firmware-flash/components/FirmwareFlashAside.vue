<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { useFirmwareFlashContext } from '../context';

const { t } = useI18n();
const ctx = useFirmwareFlashContext();

// Only destructure actions — ref state accessed via ctx.xxx for reactivity.
const { clearLogs, copyLogs, resetFlash } = ctx;
</script>

<template>
  <aside class="flex min-h-0 w-full flex-col gap-2 lg:max-w-[22rem] lg:shrink-0" :aria-label="t('flash.asideAria')">
    <!-- 错误提示 -->
    <div
      v-if="ctx.flashPhase === 'error'"
      class="aside-alert aside-alert-error flex gap-2 rounded-xl p-3 text-xs"
      role="alert"
    >
      <FontAwesomeIcon :icon="['fas', 'circle-exclamation']" class="size-4 shrink-0 mt-0.5" aria-hidden="true" />
      <div class="min-w-0 leading-snug">{{ ctx.flashMessage }}</div>
    </div>

    <!-- 成功提示 -->
    <div
      v-else-if="ctx.flashPhase === 'success'"
      class="aside-alert aside-alert-success flex gap-2 rounded-xl p-3 text-xs"
      role="status"
    >
      <FontAwesomeIcon
        :icon="['fas', 'circle-check']"
        class="size-4 shrink-0 mt-0.5 text-[var(--ty-success)]"
        aria-hidden="true"
      />
      <div class="min-w-0 leading-snug">{{ ctx.flashMessage }}</div>
    </div>

    <!-- 清除结果按钮 -->
    <button
      v-if="ctx.flashPhase !== 'idle'"
      type="button"
      class="ty-btn-secondary inline-flex min-h-9 w-full shrink-0 justify-center rounded-xl px-3 py-2 text-xs font-medium"
      @click="resetFlash"
    >
      {{ t('flash.clearResult') }}
    </button>

    <!-- 进度条 -->
    <section
      v-if="ctx.flashPhase === 'running' || ctx.flashProgress > 0"
      class="aside-progress-card min-w-0 shrink-0 rounded-xl p-3"
      :aria-label="ctx.progressCaption"
    >
      <div class="mb-2 flex items-center justify-between text-xs">
        <span class="font-medium text-[var(--ty-text-muted)]">{{ ctx.progressCaption }}</span>
        <span class="aside-progress-pct tabular-nums font-bold">{{ ctx.flashProgress }}%</span>
      </div>
      <div
        class="aside-progress-track h-2 overflow-hidden rounded-full"
        role="progressbar"
        :aria-valuenow="ctx.flashProgress"
        aria-valuemin="0"
        aria-valuemax="100"
      >
        <div
          class="aside-progress-fill h-full rounded-full transition-[width] duration-300 ease-out"
          :style="{ width: `${ctx.flashProgress}%` }"
        />
      </div>
    </section>

    <!-- 输出日志 -->
    <section
      class="aside-log-card flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-xl lg:min-h-[11rem]"
      :aria-label="t('flash.logPanelAria')"
    >
      <!-- 日志头部工具栏 -->
      <div
        class="aside-log-header flex min-w-0 shrink-0 flex-col gap-1 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
      >
        <div class="flex min-w-0 items-center gap-1.5 text-sm font-semibold text-[var(--ty-text)]">
          <FontAwesomeIcon
            :icon="['fas', 'scroll']"
            class="size-3.5 shrink-0 text-[var(--ty-primary)]"
            aria-hidden="true"
          />
          {{ t('flash.logTitle') }}
        </div>
        <div class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
          <label class="flex cursor-pointer items-center gap-1.5 text-xs text-[var(--ty-text-muted)]">
            <input v-model="ctx.lockAutoScroll" type="checkbox" class="size-3 rounded border-[var(--ty-border)]" />
            {{ t('flash.lockScroll') }}
          </label>
          <button
            type="button"
            class="aside-log-btn inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-1 text-xs font-medium"
            :aria-label="t('flash.ariaClearLog')"
            @click="clearLogs"
          >
            <FontAwesomeIcon :icon="['fas', 'trash']" class="size-3" aria-hidden="true" />
            {{ t('flash.clearLog') }}
          </button>
          <button
            type="button"
            class="aside-log-btn inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-1 text-xs font-medium"
            :aria-label="t('flash.ariaCopyLog')"
            @click="copyLogs"
          >
            <FontAwesomeIcon :icon="['fas', 'copy']" class="size-3" aria-hidden="true" />
            {{ t('flash.copyLog') }}
          </button>
        </div>
      </div>

      <!-- 日志内容 -->
      <div
        :ref="
          el => {
            ctx.logScrollRef = el as HTMLDivElement | null;
          }
        "
        class="aside-log-body min-h-[4.5rem] min-w-0 flex-1 overflow-auto overscroll-contain p-3 font-mono text-xs leading-relaxed max-lg:max-h-[12rem] lg:min-h-0 select-text"
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
