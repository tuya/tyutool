<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { formatHexDump } from '@/features/serial-debug/hex-format';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { parseAnsi, stripAnsi, type AnsiStyle } from '@/features/serial-debug/ansi-parse';

const s = useSerialDebugStore();
const { t } = useI18n();
const isNative = isTauriRuntime();

const scrollRef = ref<HTMLDivElement | null>(null);
const lockAutoScroll = ref(false);
const ctxMenu = ref<{ x: number; y: number; selected: string } | null>(null);
const searchOpen = ref(false);
const filterInputRef = ref<HTMLInputElement | null>(null);

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

const hexRendered = computed(() => {
  if (!s.hexView) return null;
  const joined: number[] = [];
  for (const l of visibleLines.value) {
    if (l.rawBytes) joined.push(...l.rawBytes);
    joined.push(0x0a);
  }
  return formatHexDump(new Uint8Array(joined), s.hexBytesPerRow);
});

const hitCount = computed(() => {
  if (s.filterMode === 'off' || !s.filterText.trim()) return null;
  return t('serialDebug.filter.hitCount', {
    hit: visibleLines.value.length,
    total: s.lines.length,
  });
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

async function toggleSearch(): Promise<void> {
  searchOpen.value = !searchOpen.value;
  if (searchOpen.value) {
    if (s.filterMode === 'off') s.filterMode = 'include';
    await nextTick();
    filterInputRef.value?.focus();
  } else {
    closeSearch();
  }
}

function closeSearch(): void {
  searchOpen.value = false;
  s.filterText = '';
  s.filterMode = 'off';
}

function toggleFilterMode(): void {
  s.filterMode = s.filterMode === 'exclude' ? 'include' : 'exclude';
}

async function openFilterWindow(): Promise<void> {
  await s.openFilterWindow();
}

function renderSpans(text: string): Array<{ text: string; style: AnsiStyle }> {
  return s.ansiEnabled ? parseAnsi(text) : [{ text: stripAnsi(text), style: {} }];
}

function spanStyle(style: AnsiStyle): Record<string, string | undefined> {
  return {
    color: style.fg,
    backgroundColor: style.bg,
    fontWeight: style.bold ? 'bold' : undefined,
    fontStyle: style.italic ? 'italic' : undefined,
    textDecoration: style.underline ? 'underline' : undefined,
  };
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
    return `[${formatTs(l.tsMs)}] [${dir}] ${stripAnsi(l.text)}`;
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

      <div class="ml-auto flex items-center gap-2">
        <button v-if="lockAutoScroll" type="button" class="paused-badge" @click="resumeScroll">
          {{ t('serialDebug.log.pausedScroll') }}
        </button>
        <button
          type="button"
          class="btn-icon"
          :class="{ 'btn-icon-active': searchOpen }"
          :aria-label="t('serialDebug.log.filterToggle')"
          @click="toggleSearch"
        >
          <FontAwesomeIcon :icon="['fas', 'magnifying-glass']" />
        </button>
        <button
          v-if="isNative"
          type="button"
          class="btn-icon"
          :aria-label="t('serialDebug.filter.openInWindow')"
          @click="openFilterWindow"
        >
          <FontAwesomeIcon :icon="['fas', 'up-right-from-square']" />
        </button>
        <button type="button" class="btn-icon" :aria-label="t('serialDebug.log.saveLog')" :disabled="s.lines.length === 0" @click="saveLog">
          <FontAwesomeIcon :icon="['fas', 'download']" />
        </button>
        <button type="button" class="btn-icon" :aria-label="t('serialDebug.conn.clear')" @click="s.clear()">
          <FontAwesomeIcon :icon="['fas', 'trash-can']" />
        </button>
      </div>
    </div>

    <!-- inline search bar (Ctrl+F style) -->
    <div v-if="searchOpen" class="search-bar flex items-center gap-2 border-b border-[var(--ty-border)] bg-[var(--ty-surface)] px-3 py-1.5">
      <input
        ref="filterInputRef"
        type="text"
        class="filter-input"
        :placeholder="t('serialDebug.filter.placeholder')"
        v-model="s.filterText"
        @keydown.esc="closeSearch"
      />
      <button
        type="button"
        class="mode-toggle"
        :class="{ 'mode-toggle-exclude': s.filterMode === 'exclude' }"
        @click="toggleFilterMode"
      >
        {{ s.filterMode === 'exclude' ? t('serialDebug.filter.exclude') : t('serialDebug.filter.include') }}
      </button>
      <span v-if="hitCount" class="hit-count">{{ hitCount }}</span>
      <button type="button" class="btn-icon" :aria-label="t('serialDebug.log.closeSearch')" @click="closeSearch">
        <FontAwesomeIcon :icon="['fas', 'xmark']" />
      </button>
    </div>

    <div v-if="s.hexView" class="pane flex-1 overflow-auto p-3 font-mono text-xs" ref="scrollRef" @scroll="onScroll">
      <pre class="whitespace-pre">{{ hexRendered }}</pre>
    </div>
    <div v-else ref="scrollRef" class="pane flex-1 overflow-auto font-mono text-xs" @scroll="onScroll" @contextmenu="onContextMenu">
      <div v-for="line in visibleLines" :key="line.id" class="line" :data-dir="line.direction">
        <span class="prefix">
          <span class="ts">{{ formatTs(line.tsMs) }}</span>
          <span class="dir-badge">{{ line.direction === 'tx' ? 'TX' : line.direction === 'rx' ? 'RX' : 'SYS' }}</span>
        </span>
        <span class="text">
          <span
            v-for="(span, si) in renderSpans(line.text)"
            :key="si"
            :style="spanStyle(span.style)"
          >{{ span.text }}</span>
        </span>
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
.btn-icon:disabled { cursor: not-allowed; opacity: 0.4; }
.btn-icon-active { color: var(--ty-primary); border-color: var(--ty-primary); }
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
/* search bar */
.search-bar { background: var(--ty-surface); }
.filter-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--ty-border);
  background: var(--ty-canvas);
  border-radius: 0.5rem;
  padding: 0.25rem 0.5rem;
  font-size: 0.8125rem;
  outline: none;
}
.filter-input:focus { border-color: var(--ty-primary); }
.mode-toggle {
  padding: 0.2rem 0.625rem;
  font-size: 0.75rem;
  font-weight: 600;
  border-radius: 0.375rem;
  cursor: pointer;
  background: color-mix(in srgb, var(--ty-primary) 15%, transparent);
  color: var(--ty-primary);
  border: 1px solid color-mix(in srgb, var(--ty-primary) 40%, transparent);
  transition: background-color 0.15s ease;
  white-space: nowrap;
}
.mode-toggle-exclude {
  background: color-mix(in srgb, var(--ty-error, #ef4444) 15%, transparent);
  color: var(--ty-error, #ef4444);
  border-color: color-mix(in srgb, var(--ty-error, #ef4444) 40%, transparent);
}
.hit-count { font-size: 0.75rem; color: var(--ty-text-muted); white-space: nowrap; }
/* log lines */
.line {
  display: flex;
  align-items: baseline;
  gap: 0.625rem;
  padding: 0.1875rem 0.625rem;
  font-size: 0.75rem;
}
.line[data-dir="tx"] { background: color-mix(in srgb, var(--ty-primary) 6%, transparent); }
.line[data-dir="rx"] { background: color-mix(in srgb, var(--ty-success) 6%, transparent); }
.line[data-dir="sys"] { color: var(--ty-text-muted); font-style: italic; }
.prefix { display: flex; align-items: baseline; gap: 0.25rem; flex-shrink: 0; white-space: nowrap; }
.ts { color: var(--ty-text-muted); font-size: 0.625rem; font-variant-numeric: tabular-nums; letter-spacing: -0.01em; }
.dir-badge {
  font-size: 8px;
  font-weight: 700;
  font-family: system-ui, sans-serif;
  letter-spacing: 0.05em;
  padding: 0.0625rem 0;
  border-radius: 0.1875rem;
  width: 2rem;
  text-align: center;
}
.line[data-dir="tx"] .dir-badge { background: color-mix(in srgb, var(--ty-primary) 20%, transparent); color: var(--ty-primary); }
.line[data-dir="rx"] .dir-badge { background: color-mix(in srgb, var(--ty-success) 20%, transparent); color: var(--ty-success); }
.line[data-dir="sys"] .dir-badge { background: color-mix(in srgb, var(--ty-text-muted) 15%, transparent); color: var(--ty-text-muted); }
.text { flex: 1; min-width: 0; white-space: pre-wrap; word-break: break-word; }
.menu-item { display: block; width: 100%; text-align: left; padding: 0.375rem 0.75rem; font-size: 0.8125rem; cursor: pointer; }
.menu-item:hover { background: var(--ty-surface-muted); }
</style>
