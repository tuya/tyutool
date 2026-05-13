<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { faCircleNotch, faFloppyDisk } from '@fortawesome/free-solid-svg-icons';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { formatHexDump } from '@/features/serial-debug/hex-format';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { parseAnsi, stripAnsi, type AnsiStyle } from '@/features/serial-debug/ansi-parse';
import { makeStamp, formatTs } from '@/features/serial-debug/context';
import type { DebugLogLine, HexBytesPerRow } from '@/features/serial-debug/types';
import SerialDebugChipBar from './SerialDebugChipBar.vue';

const props = withDefaults(defineProps<{
  lines: DebugLogLine[];
  hexView: boolean;
  hexBytesPerRow: HexBytesPerRow;
  ansiEnabled: boolean;
  exportTitle?: string;
}>(), {
  exportTitle: 'serial-debug',
});

const emit = defineEmits<{
  clear: [];
}>();

const s = useSerialDebugStore();
const { t } = useI18n();

const activeChip = computed(() =>
  s.activeChipId ? s.watchChips.find((c) => c.id === s.activeChipId) ?? null : null,
);

const displayLines = computed(() => {
  if (!activeChip.value) return props.lines;
  return props.lines.filter((l) => s.matchChipKeyword(l, activeChip.value!));
});

const scrollRef = ref<HTMLDivElement | null>(null);

// Per-tab scroll lock. Key: activeChipId (null = "All" tab).
// Each tab maintains its own pause state independently.
const lockByTab = reactive(new Map<string | null, boolean>());

const lockAutoScroll = computed({
  get: (): boolean => lockByTab.get(s.activeChipId ?? null) ?? false,
  set: (val: boolean) => { lockByTab.set(s.activeChipId ?? null, val); },
});

async function scrollToBottom(): Promise<void> {
  await nextTick();
  const el = scrollRef.value;
  if (!el || lockAutoScroll.value) return;
  el.scrollTop = el.scrollHeight;
}

watch(() => displayLines.value.length, () => { void scrollToBottom(); });

// Tab change: scroll to bottom for the newly active tab (respects its own lock state).
watch(() => s.activeChipId, () => { void scrollToBottom(); });

// hexView toggle swaps the scrollRef DOM node (v-if/v-else); scroll into the new element.
watch(() => props.hexView, () => { void scrollToBottom(); });

// Layout changes (font size, search bar open/close) shift scrollHeight or clientHeight,
// which can fire a spurious scroll event that falsely locks auto-scroll.
// Capture the lock state before the DOM update and restore it after.
function watchLayoutChange(source: () => unknown): void {
  watch(source, () => {
    const wasLocked = lockAutoScroll.value;
    void nextTick().then(() => {
      if (!wasLocked) {
        lockAutoScroll.value = false;
        void scrollToBottom();
      }
    });
  }, { flush: 'sync' });
}

watchLayoutChange(() => s.logFontSize);

function onScroll(): void {
  const el = scrollRef.value;
  if (!el) return;
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  lockAutoScroll.value = !atBottom;
}

async function resumeScroll(): Promise<void> {
  lockAutoScroll.value = false;
  await scrollToBottom();
}

const hexRendered = computed(() => {
  if (!props.hexView) return null;
  const joined: number[] = [];
  for (const l of props.lines) {
    if (l.rawBytes) joined.push(...l.rawBytes);
    joined.push(0x0a);
  }
  return formatHexDump(new Uint8Array(joined), props.hexBytesPerRow);
});

function renderSpans(text: string): Array<{ text: string; style: AnsiStyle }> {
  return props.ansiEnabled ? parseAnsi(text) : [{ text: stripAnsi(text), style: {} }];
}

function splitByKeyword(text: string, q: string): Array<{ text: string; isMatch: boolean }> {
  const parts: Array<{ text: string; isMatch: boolean }> = [];
  const lower = text.toLowerCase();
  let pos = 0;
  while (pos < text.length) {
    const idx = lower.indexOf(q, pos);
    if (idx === -1) { parts.push({ text: text.slice(pos), isMatch: false }); break; }
    if (idx > pos) parts.push({ text: text.slice(pos, idx), isMatch: false });
    parts.push({ text: text.slice(idx, idx + q.length), isMatch: true });
    pos = idx + q.length;
  }
  return parts;
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

const ctxMenu = ref<{ x: number; y: number; selected: string } | null>(null);

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

function showCtxPopup(mode: 'hex' | 'ascii'): void {
  if (!ctxMenu.value) return;
  const bytes = new TextEncoder().encode(ctxMenu.value.selected);
  s.showHexPopup(bytes, mode);
  ctxMenu.value = null;
}

function dismissCtx(): void { ctxMenu.value = null; }

const containerRef = ref<HTMLDivElement | null>(null);
const searchOpen = ref(false);
watchLayoutChange(() => searchOpen.value);
const searchText = ref('');
const searchIndex = ref(0);
const searchInputRef = ref<HTMLInputElement | null>(null);

const searchQuery = computed(() => searchText.value.trim().toLowerCase());

// Single pass: ordered list of matching IDs; Set derived from it for O(1) template lookup.
const matchingLineIdList = computed<number[]>(() => {
  const q = searchQuery.value;
  if (!q) return [];
  return props.lines
    .filter((l) => stripAnsi(l.text).toLowerCase().includes(q))
    .map((l) => l.id);
});

const matchingLineIds = computed<Set<number>>(() => new Set(matchingLineIdList.value));
const matchCount = computed(() => matchingLineIdList.value.length);

const currentMatchLineId = computed<number | null>(() => {
  const list = matchingLineIdList.value;
  if (!list.length) return null;
  return list[searchIndex.value % list.length];
});

watch(searchText, () => {
  searchIndex.value = 0;
  void scrollToMatch();
});
watch(matchCount, (count) => {
  if (searchIndex.value >= count && count > 0) searchIndex.value = count - 1;
});

async function openSearch(): Promise<void> {
  searchOpen.value = true;
  await nextTick();
  searchInputRef.value?.focus();
}

function closeSearch(): void {
  searchOpen.value = false;
  searchText.value = '';
  searchIndex.value = 0;
  containerRef.value?.focus();
}

async function navigateSearch(delta: number): Promise<void> {
  const count = matchCount.value;
  if (!count) return;
  searchIndex.value = ((searchIndex.value + delta) % count + count) % count;
  await scrollToMatch();
}

async function scrollToMatch(): Promise<void> {
  const id = currentMatchLineId.value;
  if (id === null) return;
  await nextTick();
  const el = scrollRef.value?.querySelector(`[data-line-id="${id}"]`);
  el?.scrollIntoView({ block: 'nearest' });
}

function onContainerKeydown(ev: KeyboardEvent): void {
  if (ev.ctrlKey && ev.key === 'f') {
    ev.preventDefault();
    void openSearch();
  }
}

function onSearchKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'Escape') {
    ev.preventDefault();
    closeSearch();
  } else if (ev.key === 'Enter') {
    ev.preventDefault();
    void navigateSearch(ev.shiftKey ? -1 : 1);
  }
}

async function writeFile(defaultName: string, content: string, ext: string, mimeType: string): Promise<void> {
  if (isTauriRuntime()) {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const { invoke } = await import('@tauri-apps/api/core');
    const path = await save({
      defaultPath: defaultName,
      filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
    });
    if (path) await invoke('write_text_file', { path, content });
  } else {
    const blob = new Blob([content], { type: `${mimeType};charset=utf-8` });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = defaultName;
    a.click();
    URL.revokeObjectURL(url);
  }
}

async function saveLog(): Promise<void> {
  const content = props.lines.map((l) => {
    const dir = l.direction === 'tx' ? 'TX ' : l.direction === 'rx' ? 'RX ' : 'SYS';
    return `[${formatTs(l.tsMs)}] [${dir}] ${stripAnsi(l.text)}`;
  }).join('\n');
  await writeFile(`${props.exportTitle}-${makeStamp()}.txt`, content, 'txt', 'text/plain');
}


</script>

<template>
  <div
    ref="containerRef"
    tabindex="0"
    class="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-[var(--ty-border)] bg-[var(--ty-canvas)] outline-none"
    @keydown="onContainerKeydown"
  >
    <!-- toolbar -->
    <div class="log-toolbar flex items-center gap-2 border-b border-[var(--ty-border)] px-3 py-1.5">
      <span class="toolbar-title">{{ t('serialDebug.log.title') }}</span>

      <!-- display toggles: timestamp & direction badge — left side, next to title -->
      <button
        type="button"
        class="btn-tool"
        :class="{ 'btn-tool-active': s.showTimestamp }"
        :aria-label="t('serialDebug.log.toggleTimestamp')"
        @click="s.showTimestamp = !s.showTimestamp"
      >
        <FontAwesomeIcon :icon="['fas', 'clock']" class="size-3 shrink-0" />
        {{ t('serialDebug.log.toggleTimestamp') }}
      </button>
      <button
        type="button"
        class="btn-tool"
        :class="{ 'btn-tool-active': s.showDirBadge }"
        :aria-label="t('serialDebug.log.toggleDirBadge')"
        @click="s.showDirBadge = !s.showDirBadge"
      >
        <FontAwesomeIcon :icon="['fas', 'tag']" class="size-3 shrink-0" />
        {{ t('serialDebug.log.toggleDirBadge') }}
      </button>

      <button v-if="lockAutoScroll" type="button" class="paused-badge" :aria-label="t('serialDebug.log.pausedScroll')" @click="resumeScroll">
        <FontAwesomeIcon :icon="['fas', 'arrow-down']" class="size-3 shrink-0" />
        {{ t('serialDebug.log.pausedScroll') }}
      </button>

      <div class="ml-auto flex items-center gap-1">
        <!-- status: autosave -->
        <span class="autosave-indicator" :class="{ 'autosave-indicator--active': s.sessionAutoSavePath }">
          <FontAwesomeIcon
            :icon="s.sessionAutoSavePath ? faCircleNotch : faFloppyDisk"
            class="size-3 shrink-0"
            :class="{ 'fa-spin': s.sessionAutoSavePath }"
          />
          {{ s.sessionAutoSavePath ? t('serialDebug.autoSave.active') : t('serialDebug.autoSave.off') }}
        </span>

        <span class="toolbar-divider" />

        <!-- font size stepper -->
        <div class="font-size-stepper">
          <button type="button" class="stepper-btn" :disabled="s.logFontSize <= 10" :aria-label="t('serialDebug.log.fontDecrease')" @click="s.decreaseFontSize">
            <FontAwesomeIcon :icon="['fas', 'magnifying-glass-minus']" class="size-3 shrink-0" />
          </button>
          <span class="font-size-label">{{ t('serialDebug.log.fontSize') }} {{ s.logFontSize }}</span>
          <button type="button" class="stepper-btn" :disabled="s.logFontSize >= 18" :aria-label="t('serialDebug.log.fontIncrease')" @click="s.increaseFontSize">
            <FontAwesomeIcon :icon="['fas', 'magnifying-glass-plus']" class="size-3 shrink-0" />
          </button>
        </div>

        <span class="toolbar-divider" />

        <!-- search -->
        <button
          type="button"
          class="btn-tool"
          :class="{ 'btn-tool-active': searchOpen }"
          :aria-label="t('serialDebug.log.filterToggle')"
          @click="openSearch"
        >
          <FontAwesomeIcon :icon="['fas', 'magnifying-glass']" class="size-3 shrink-0" />
          {{ t('serialDebug.log.filterToggle') }}
        </button>

        <span class="toolbar-divider" />

        <!-- action group: save & clear -->
        <button
          type="button"
          class="btn-tool"
          :aria-label="t('serialDebug.log.saveLog')"
          :disabled="lines.length === 0"
          @click="saveLog"
        >
          <FontAwesomeIcon :icon="['fas', 'download']" class="size-3 shrink-0" />
          {{ t('serialDebug.log.saveLog') }}
        </button>
        <button
          type="button"
          class="btn-tool"
          :aria-label="t('serialDebug.conn.clear')"
          @click="emit('clear')"
        >
          <FontAwesomeIcon :icon="['fas', 'trash-can']" class="size-3 shrink-0" />
          {{ t('serialDebug.conn.clear') }}
        </button>
      </div>
    </div>

    <!-- chip bar -->
    <SerialDebugChipBar />

    <!-- Ctrl+F search bar -->
    <div
      v-if="searchOpen"
      class="search-bar flex items-center gap-2 border-b border-[var(--ty-border)] bg-[var(--ty-surface)] px-3 py-0.5"
    >
      <input
        ref="searchInputRef"
        type="text"
        class="search-input"
        :placeholder="t('serialDebug.search.placeholder')"
        v-model="searchText"
        @keydown="onSearchKeydown"
      />
      <span class="match-count">
        <template v-if="matchCount > 0">
          {{ t('serialDebug.search.count', { current: (searchIndex % matchCount) + 1, total: matchCount }) }}
        </template>
        <template v-else-if="searchText.trim()">
          {{ t('serialDebug.search.noMatch') }}
        </template>
      </span>
      <button
        type="button"
        class="btn-tool"
        :aria-label="t('serialDebug.search.prev')"
        :disabled="matchCount === 0"
        @click="navigateSearch(-1)"
      >
        <FontAwesomeIcon :icon="['fas', 'chevron-up']" class="size-3 shrink-0" />
        {{ t('serialDebug.search.prev') }}
      </button>
      <button
        type="button"
        class="btn-tool"
        :aria-label="t('serialDebug.search.next')"
        :disabled="matchCount === 0"
        @click="navigateSearch(1)"
      >
        <FontAwesomeIcon :icon="['fas', 'chevron-down']" class="size-3 shrink-0" />
        {{ t('serialDebug.search.next') }}
      </button>
      <button type="button" class="btn-tool" :aria-label="t('serialDebug.search.close')" @click="closeSearch">
        <FontAwesomeIcon :icon="['fas', 'xmark']" class="size-3 shrink-0" />
        {{ t('serialDebug.search.close') }}
      </button>
    </div>

    <!-- hex view -->
    <div
      v-if="hexView"
      ref="scrollRef"
      class="pane flex-1 overflow-auto p-3 font-mono text-xs"
      :style="{ fontSize: s.logFontSize + 'px' }"
      @scroll="onScroll"
    >
      <pre class="whitespace-pre">{{ hexRendered }}</pre>
    </div>

    <!-- ASCII line view -->
    <div
      v-else
      ref="scrollRef"
      class="pane flex-1 overflow-auto font-mono text-xs"
      :style="{ fontSize: s.logFontSize + 'px' }"
      @scroll="onScroll"
      @contextmenu="onContextMenu"
    >
      <div
        v-for="line in displayLines"
        :key="line.id"
        :data-line-id="line.id"
        class="line"
        :data-dir="line.direction"
        :class="{
          'line-search-match': matchingLineIds.has(line.id) && line.id !== currentMatchLineId,
          'line-search-current': line.id === currentMatchLineId,
        }"
      >
        <span v-if="s.showTimestamp || s.showDirBadge" class="prefix">
          <span v-if="s.showTimestamp" class="ts">{{ formatTs(line.tsMs) }}</span>
          <span
            v-if="s.showDirBadge"
            class="dir-badge"
          >{{ line.direction === 'tx' ? 'TX' : line.direction === 'rx' ? 'RX' : 'SYS' }}</span>
        </span>
        <span class="text">
          <span
            v-for="(span, si) in renderSpans(line.text)"
            :key="si"
            :style="spanStyle(span.style)"
          >
            <template v-if="searchQuery && matchingLineIds.has(line.id)">
              <template v-for="(seg, sj) in splitByKeyword(span.text, searchQuery)" :key="sj">
                <mark v-if="seg.isMatch" class="search-keyword-mark">{{ seg.text }}</mark><template v-else>{{ seg.text }}</template>
              </template>
            </template>
            <template v-else>{{ span.text }}</template>
          </span>
        </span>
      </div>
      <div v-if="displayLines.length === 0" class="px-3 py-2 text-[var(--ty-text-muted)]">
        {{ t('serialDebug.log.waitingData') }}
      </div>
    </div>

    <!-- right-click context menu -->
    <div
      v-if="ctxMenu"
      class="ctx-menu fixed z-50 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface)] py-1 shadow-lg"
      :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
      @click.stop
    >
      <button type="button" class="menu-item" @click="copy">{{ t('serialDebug.hexPopup.copy') }}</button>
      <button type="button" class="menu-item" @click="showCtxPopup('hex')">{{ t('serialDebug.hexPopup.toHex') }}</button>
      <button type="button" class="menu-item" @click="showCtxPopup('ascii')">{{ t('serialDebug.hexPopup.toAscii') }}</button>
    </div>
    <div v-if="ctxMenu" class="fixed inset-0 z-40" @click="dismissCtx" @contextmenu.prevent="dismissCtx" />
  </div>
</template>

<style scoped>
/* toolbar */
.log-toolbar { background: var(--ty-surface); }
.toolbar-title { font-size: 0.8125rem; font-weight: 600; color: var(--ty-text-muted); white-space: nowrap; }
.toolbar-divider { width: 1px; height: 1rem; background: var(--ty-border); flex-shrink: 0; margin: 0 0.125rem; }
.font-size-stepper {
  display: inline-flex;
  align-items: center;
  border: 1px solid var(--ty-border);
  border-radius: 0.375rem;
  overflow: hidden;
}
.stepper-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0.125rem 0.4rem;
  background: transparent;
  border: none;
  cursor: pointer;
  color: var(--ty-text-muted);
  transition: background-color 0.15s ease, color 0.15s ease;
}
.stepper-btn:hover { background: var(--ty-surface-muted); color: var(--ty-text); }
.stepper-btn:disabled { cursor: not-allowed; opacity: 0.4; }
.font-size-label { font-size: 0.75rem; color: var(--ty-text-muted); min-width: 4rem; text-align: center; font-variant-numeric: tabular-nums; border-left: 1px solid var(--ty-border); border-right: 1px solid var(--ty-border); padding: 0 0.375rem; white-space: nowrap; }
.btn-tool {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.125rem 0.5rem;
  border: 1px solid transparent;
  border-radius: 0.375rem;
  background: transparent;
  cursor: pointer;
  font-size: 0.8125rem;
  color: var(--ty-text-muted);
  white-space: nowrap;
  transition: background-color 0.15s ease, color 0.15s ease, border-color 0.15s ease;
}
.btn-tool:hover { background: var(--ty-surface-muted); color: var(--ty-text); }
.btn-tool:disabled { cursor: not-allowed; opacity: 0.4; }
.btn-tool-active { color: var(--ty-primary); border-color: var(--ty-primary); }.paused-badge {
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
.autosave-indicator {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.125rem 0.5rem;
  border-radius: 0.375rem;
  font-size: 0.8125rem;
  white-space: nowrap;
  color: var(--ty-text-muted);
  opacity: 0.55;
  user-select: none;
}
.autosave-indicator--active {
  color: var(--ty-success);
  opacity: 1;
}
/* search bar */
.search-bar { background: var(--ty-surface); }
.search-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--ty-border);
  background: var(--ty-canvas);
  border-radius: 0.5rem;
  padding: 0.25rem 0.5rem;
  font-size: 0.8125rem;
  outline: none;
}
.search-input:focus { border-color: var(--ty-primary); }
.match-count { font-size: 0.75rem; color: var(--ty-text-muted); white-space: nowrap; min-width: 5rem; }
/* log lines */
.line {
  display: flex;
  align-items: baseline;
  gap: 0.625rem;
  padding: 0.1875rem 0.625rem;
  font-size: inherit;
}
.line[data-dir="tx"] { }
.line[data-dir="rx"] { }
.line[data-dir="sys"] { color: var(--ty-text-muted); font-style: italic; }
.line-search-match { background: color-mix(in srgb, #eab308 20%, transparent) !important; }
.line-search-current { background: color-mix(in srgb, #f97316 35%, transparent) !important; }
.prefix { display: flex; align-items: baseline; gap: 0.25rem; flex-shrink: 0; white-space: nowrap; }
.ts { color: var(--ty-text-muted); font-size: 0.85em; font-variant-numeric: tabular-nums; letter-spacing: -0.01em; }
.dir-badge {
  font-size: 0.7em;
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
mark.search-keyword-mark { background: color-mix(in srgb, #eab308 55%, transparent); color: inherit; border-radius: 0.125rem; padding: 0 0.05rem; }
.menu-item { display: block; width: 100%; text-align: left; padding: 0.375rem 0.75rem; font-size: 0.8125rem; cursor: pointer; }
.menu-item:hover { background: var(--ty-surface-muted); }
</style>
