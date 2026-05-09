<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import type { DebugLogLine, FilterMode } from './types';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';

const { t } = useI18n();

const lines = ref<DebugLogLine[]>([]);
const filterText = ref('');
const filterMode = ref<FilterMode>('include');

type InitPayload = { lines: DebugLogLine[]; filterText: string; filterMode: FilterMode };
type FeedPayload = { line: DebugLogLine };
type ClearPayload = Record<string, never>;

let unlistenInit: undefined | (() => void);
let unlistenFeed: undefined | (() => void);
let unlistenClear: undefined | (() => void);

onMounted(async () => {
  if (!isTauriRuntime()) {
    // Web mode never navigates to this route; FilterWindow is only rendered inside
    // the Tauri `serial-debug-filter` subwindow. Bail early if somehow reached.
    return;
  }
  const { listen } = await import('@tauri-apps/api/event');
  unlistenInit = await listen<InitPayload>('serial-debug-filter-init', (ev) => {
    lines.value = ev.payload.lines;
    filterText.value = ev.payload.filterText;
    filterMode.value = ev.payload.filterMode;
  });
  unlistenFeed = await listen<FeedPayload>('serial-debug-filter-feed', (ev) => {
    lines.value.push(ev.payload.line);
    if (lines.value.length > 20000) lines.value.splice(0, lines.value.length - 20000);
  });
  unlistenClear = await listen<ClearPayload>('serial-debug-filter-clear', () => {
    lines.value = [];
  });
});

onUnmounted(() => {
  unlistenInit?.();
  unlistenFeed?.();
  unlistenClear?.();
});

const visible = computed(() => {
  const f = filterText.value.trim();
  if (!f || filterMode.value === 'off') return lines.value;
  const includes = filterMode.value === 'include';
  return lines.value.filter((l) => l.text.includes(f) === includes);
});

function fmtTs(ms: number): string {
  const d = new Date(ms);
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, '0'))
    .join(':') + '.' + String(d.getMilliseconds()).padStart(3, '0');
}
</script>

<template>
  <div class="flex h-dvh flex-col bg-[var(--ty-canvas)]">
    <header class="flex items-center gap-2 border-b border-[var(--ty-border)] bg-[var(--ty-surface)] px-3 py-2 text-xs">
      <span class="text-[var(--ty-text-muted)]">{{ t('serialDebug.filter.placeholder') }}:</span>
      <code class="rounded bg-[var(--ty-canvas)] px-2 py-0.5">{{ filterText || '(empty)' }}</code>
      <span class="rounded bg-[var(--ty-surface-muted)] px-2 py-0.5">{{ filterMode }}</span>
      <span class="ml-auto text-[var(--ty-text-muted)]">{{ t('serialDebug.filter.hitCount', { hit: visible.length, total: lines.length }) }}</span>
    </header>
    <div class="flex-1 overflow-auto font-mono text-xs">
      <div v-for="line in visible" :key="line.id" class="line" :data-dir="line.direction">
        <span class="ts">{{ fmtTs(line.tsMs) }}</span>
        <span class="dir">{{ line.direction === 'tx' ? '▶' : line.direction === 'rx' ? '◀' : '●' }}</span>
        <span class="text">{{ line.text }}</span>
      </div>
      <div v-if="lines.length === 0" class="p-3 text-[var(--ty-text-muted)]">{{ t('serialDebug.log.waitingData') }}</div>
    </div>
  </div>
</template>

<style scoped>
.line { padding: 0.125rem 0.75rem; display: grid; grid-template-columns: 7.5rem 1rem 1fr; gap: 0.5rem; }
.line[data-dir="tx"] { background: color-mix(in srgb, var(--ty-primary) 6%, transparent); }
.ts { color: var(--ty-text-muted); font-variant-numeric: tabular-nums; }
.text { white-space: pre-wrap; word-break: break-word; }
</style>
