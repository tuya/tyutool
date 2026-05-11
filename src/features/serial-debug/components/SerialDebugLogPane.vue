<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { formatHexDump } from '@/features/serial-debug/hex-format';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';

const s = useSerialDebugStore();
const { t } = useI18n();

const scrollRef = ref<HTMLDivElement | null>(null);
const lockAutoScroll = ref(false);
const ctxMenu = ref<{ x: number; y: number; selected: string } | null>(null);
const filterOpen = ref(false);

const filterExpanded = computed(() => filterOpen.value || s.filterMode !== 'off');

function formatTs(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const ss = String(d.getSeconds()).padStart(2, '0');
  const mmm = String(d.getMilliseconds()).padStart(3, '0');
  return `${hh}:${mm}:${ss}.${mmm}`;
}

const visibleLines = computed(() => {
  const f = s.filterText.trim();
  if (!f || s.filterMode === 'off') return s.lines;
  const includes = s.filterMode === 'include';
  return s.lines.filter((l) => l.direction === 'sys' || l.text.includes(f) === includes);
});

const hitCount = computed(() => {
  if (s.filterMode === 'off' || !s.filterText.trim()) return null;
  return t('serialDebug.filter.hitCount', { hit: visibleLines.value.length, total: s.lines.length });
});

const hexRendered = computed(() => {
  if (!s.hexView) return null;
  const joined: number[] = [];
  for (const l of visibleLines.value) {
    if (l.rawBytes) joined.push(...l.rawBytes);
    joined.push(0x0a);
  }
  return formatHexDump(new Uint8Array(joined), s.hexBytesPerRow);
});

async function scrollToBottom(): Promise<void> {
  await nextTick();
  const el = scrollRef.value;
  if (!el || lockAutoScroll.value) return;
  el.scrollTop = el.scrollHeight;
}

watch(() => s.lines.length, () => { void scrollToBottom(); });
onMounted(() => { void scrollToBottom(); });

function onScroll(): void {
  const el = scrollRef.value;
  if (!el) return;
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 32;
  lockAutoScroll.value = !atBottom;
}

async function resumeScroll(): Promise<void> {
  lockAutoScroll.value = false;
  await scrollToBottom();
}

function onContextMenu(ev: MouseEvent): void {
  const sel = window.getSelection()?.toString() ?? '';
  if (!sel) { ctxMenu.value = null; return; }
  ev.preventDefault();
  ctxMenu.value = { x: ev.clientX, y: ev.clientY, selected: sel };
}

function copy(): void {
  if (!ctxMenu.value) return;
  void navigator.clipboard.writeText(ctxMenu.value.selected);
  ctxMenu.value = null;
}
function toHex(): void {
  if (!ctxMenu.value) return;
  const bytes = new TextEncoder().encode(ctxMenu.value.selected);
  s.showHexPopup(bytes, 'hex');
  ctxMenu.value = null;
}
function toAscii(): void {
  if (!ctxMenu.value) return;
  const bytes = new TextEncoder().encode(ctxMenu.value.selected);
  s.showHexPopup(bytes, 'ascii');
  ctxMenu.value = null;
}
function dismissCtx(): void { ctxMenu.value = null; }

async function saveLog(): Promise<void> {
  const content = s.lines.map((l) => {
    const dir = l.direction === 'tx' ? 'TX ' : l.direction === 'rx' ? 'RX ' : 'SYS';
    return `[${formatTs(l.tsMs)}] [${dir}] ${l.text}`;
  }).join('\n');
  const now = new Date();
  const stamp = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, '0')}${String(now.getDate()).padStart(2, '0')}_${String(now.getHours()).padStart(2, '0')}${String(now.getMinutes()).padStart(2, '0')}${String(now.getSeconds()).padStart(2, '0')}`;
  const defaultName = `serial-debug-${stamp}.txt`;
  if (isTauriRuntime()) {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const { invoke } = await import('@tauri-apps/api/core');
    const path = await save({ defaultPath: defaultName, filters: [{ name: 'Text', extensions: ['txt'] }] });
    if (path) await invoke('write_text_file', { path, content });
  } else {
    const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = defaultName;
    a.click();
    URL.revokeObjectURL(url);
  }
}
</script>

<template>
  <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-[var(--ty-border)] bg-[var(--ty-canvas)]">
    <!-- toolbar -->
    <div class="log-toolbar flex items-center gap-2 border-b border-[var(--ty-border)] px-3 py-1.5">
      <span class="toolbar-title">{{ t('serialDebug.log.title') }}</span>

      <button
        type="button"
        class="btn-icon"
        :class="{ 'btn-icon-active': filterExpanded }"
        :aria-label="t('serialDebug.log.filterToggle')"
        @click="filterOpen = !filterOpen"
      >
        <FontAwesomeIcon :icon="['fas', 'magnifying-glass']" />
      </button>

      <template v-if="filterExpanded">
        <input
          type="text"
          class="filter-input"
          :placeholder="t('serialDebug.filter.placeholder')"
          v-model="s.filterText"
        />
        <div class="mode-group flex rounded-lg border border-[var(--ty-border)]">
          <button type="button" :class="{ active: s.filterMode === 'off' }" class="mode-btn" @click="s.filterMode = 'off'">{{ t('serialDebug.filter.off') }}</button>
          <button type="button" :class="{ active: s.filterMode === 'include' }" class="mode-btn" @click="s.filterMode = 'include'">{{ t('serialDebug.filter.include') }}</button>
          <button type="button" :class="{ active: s.filterMode === 'exclude' }" class="mode-btn" @click="s.filterMode = 'exclude'">{{ t('serialDebug.filter.exclude') }}</button>
        </div>
        <span v-if="hitCount" class="hit-count">{{ hitCount }}</span>
      </template>

      <div class="ml-auto flex items-center gap-2">
        <button v-if="lockAutoScroll" type="button" class="paused-badge" @click="resumeScroll">
          {{ t('serialDebug.log.pausedScroll') }}
        </button>
        <button type="button" class="btn-icon" :aria-label="t('serialDebug.log.saveLog')" :disabled="s.lines.length === 0" @click="saveLog">
          <FontAwesomeIcon :icon="['fas', 'download']" />
        </button>
        <button type="button" class="btn-icon" :aria-label="t('serialDebug.conn.clear')" @click="s.clear()">
          <FontAwesomeIcon :icon="['fas', 'trash-can']" />
        </button>
      </div>
    </div>

    <div v-if="s.hexView" class="pane flex-1 overflow-auto p-3 font-mono text-xs" ref="scrollRef" @scroll="onScroll">
      <pre class="whitespace-pre">{{ hexRendered }}</pre>
    </div>
    <div v-else ref="scrollRef" class="pane flex-1 overflow-auto font-mono text-xs" @scroll="onScroll" @contextmenu="onContextMenu">
      <div v-for="line in visibleLines" :key="line.id" class="line" :data-dir="line.direction">
        <span class="ts">{{ formatTs(line.tsMs) }}</span>
        <span class="dir">{{ line.direction === 'tx' ? '▶' : line.direction === 'rx' ? '◀' : '●' }}</span>
        <span class="text">{{ line.text }}</span>
      </div>
      <div v-if="visibleLines.length === 0" class="px-3 py-2 text-[var(--ty-text-muted)]">{{ t('serialDebug.log.waitingData') }}</div>
    </div>

    <!-- lightweight right-click menu -->
    <div
      v-if="ctxMenu"
      class="ctx-menu fixed z-50 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface)] py-1 shadow-lg"
      :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
      @click.stop
    >
      <button type="button" class="menu-item" @click="copy">{{ t('serialDebug.hexPopup.copy') }}</button>
      <button type="button" class="menu-item" @click="toHex">{{ t('serialDebug.hexPopup.toHex') }}</button>
      <button type="button" class="menu-item" @click="toAscii">{{ t('serialDebug.hexPopup.toAscii') }}</button>
    </div>
    <div v-if="ctxMenu" class="fixed inset-0 z-40" @click="dismissCtx" @contextmenu.prevent="dismissCtx" />
  </div>
</template>

<style scoped>
/* toolbar */
.log-toolbar { background: var(--ty-surface); }
.toolbar-title { font-size: 0.75rem; font-weight: 600; color: var(--ty-text-muted); white-space: nowrap; }
.filter-input {
  border: 1px solid var(--ty-border);
  background: var(--ty-canvas);
  border-radius: 0.5rem;
  padding: 0.25rem 0.5rem;
  font-size: 0.8125rem;
  width: 10rem;
}
.hit-count { font-size: 0.75rem; color: var(--ty-text-muted); white-space: nowrap; }
.btn-icon {
  padding: 0.375rem 0.5rem;
  border: 1px solid transparent;
  border-radius: 0.5rem;
  background: transparent;
  cursor: pointer;
  color: var(--ty-text-muted);
  transition: background-color 0.15s ease, color 0.15s ease, border-color 0.15s ease;
}
.btn-icon:hover { background: var(--ty-surface-muted); color: var(--ty-text); }
.btn-icon-active { color: var(--ty-primary); border-color: var(--ty-primary); }
.mode-btn {
  padding: 0.2rem 0.5rem;
  font-size: 0.75rem;
  cursor: pointer;
  transition: background-color 0.15s ease, color 0.15s ease;
}
.mode-btn:not(.active):hover { background: var(--ty-surface-muted); }
.mode-btn.active { background: var(--ty-primary); color: white; font-weight: 600; }
.paused-badge {
  font-size: 0.7rem;
  padding: 0.2rem 0.5rem;
  border-radius: 9999px;
  background: color-mix(in srgb, var(--ty-accent, #f97316) 15%, transparent);
  color: var(--ty-accent, #f97316);
  border: 1px solid var(--ty-accent, #f97316);
  cursor: pointer;
  white-space: nowrap;
  transition: background-color 0.15s ease, opacity 0.15s ease;
}
.paused-badge:hover { background: color-mix(in srgb, var(--ty-accent, #f97316) 25%, transparent); }
/* log lines */
.line { padding: 0.125rem 0.75rem; display: grid; grid-template-columns: 7.5rem 1rem 1fr; gap: 0.5rem; }
.line[data-dir="tx"] { color: var(--ty-primary); }
.line[data-dir="sys"] { color: var(--ty-text-muted); font-style: italic; }
.ts { color: var(--ty-text-muted); font-variant-numeric: tabular-nums; }
.line[data-dir="tx"] .ts { color: color-mix(in srgb, var(--ty-primary) 60%, var(--ty-text-muted)); }
.text { white-space: pre-wrap; word-break: break-word; }
.menu-item { display: block; width: 100%; text-align: left; padding: 0.375rem 0.75rem; font-size: 0.8125rem; cursor: pointer; }
.menu-item:hover { background: var(--ty-surface-muted); }
</style>
