<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { formatHexDump } from '@/features/serial-debug/hex-format';

const s = useSerialDebugStore();
const { t } = useI18n();

const scrollRef = ref<HTMLDivElement | null>(null);
const lockAutoScroll = ref(false);
const ctxMenu = ref<{ x: number; y: number; selected: string } | null>(null);

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
  return s.lines.filter((l) => l.text.includes(f) === includes);
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
</script>

<template>
  <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-[var(--ty-border)] bg-[var(--ty-canvas)]">
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
.line { padding: 0.125rem 0.75rem; display: grid; grid-template-columns: 7.5rem 1rem 1fr; gap: 0.5rem; }
.line[data-dir="tx"] { background: color-mix(in srgb, var(--ty-primary) 6%, transparent); }
.line[data-dir="rx"] { background: transparent; }
.line[data-dir="sys"] { color: var(--ty-text-muted); font-style: italic; }
.ts { color: var(--ty-text-muted); font-variant-numeric: tabular-nums; }
.text { white-space: pre-wrap; word-break: break-word; }
.menu-item { display: block; width: 100%; text-align: left; padding: 0.375rem 0.75rem; font-size: 0.8125rem; }
.menu-item:hover { background: var(--ty-surface-muted); }
</style>
