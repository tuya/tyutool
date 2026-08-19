<script setup lang="ts">
import {
  computed,
  nextTick,
  onActivated,
  onDeactivated,
  onUnmounted,
  reactive,
  ref,
  watch,
  watchEffect,
} from "vue";
import { useI18n } from "vue-i18n";
import { faCircleNotch, faFloppyDisk } from "@fortawesome/free-solid-svg-icons";
import { useSerialDebugStore } from "@/stores/serial-debug";
import { serialDebugTransport } from "@/features/serial-debug/transport";
import { isTauriRuntime } from "@/runtime";
import type { AnsiStyle } from "@/features/serial-debug/ansi-parse";
import {
  SerialDebugLogLineRenderer,
  type RenderedLogLine,
} from "@/features/serial-debug/log-line-renderer";
import { SerialDebugHexViewRenderer } from "@/features/serial-debug/hex-view-renderer";
import { makeStamp, formatTs } from "@/features/serial-debug/utils";
import { formatExportLine } from "@/features/serial-debug/archive-line-text";
import { EXPORT_PAGE_SIZE } from "@/features/serial-debug/constants";
import type {
  DebugLogLine,
  HexBytesPerRow,
} from "@/features/serial-debug/types";
import SerialDebugChipBar from "./SerialDebugChipBar.vue";

const props = withDefaults(
  defineProps<{
    hexView: boolean;
    hexBytesPerRow: HexBytesPerRow;
    ansiEnabled: boolean;
    exportTitle?: string;
  }>(),
  {
    exportTitle: "serial-debug",
  },
);

const emit = defineEmits<{
  clear: [];
}>();

const s = useSerialDebugStore();
const transport = serialDebugTransport();
const { t } = useI18n();
const lineRenderer = new SerialDebugLogLineRenderer();
const hexViewRenderer = new SerialDebugHexViewRenderer();

const activeChip = computed(() =>
  s.activeChipId
    ? (s.watchChips.find((c) => c.id === s.activeChipId) ?? null)
    : null,
);

// A getter, not a computed: the store's live line buffer is a shallowRef whose
// array identity stays the same across appends, and a computed that returns an
// identity-stable value never notifies its subscribers. Calling through makes
// every computed/watcher below depend on the store refs directly.
const displayLines = (): DebugLogLine[] => s.activeDisplayLines();

const scrollRef = ref<HTMLDivElement | null>(null);

// ── viewport virtualization (both views) ────────────────────────────────
// Only the rows inside the viewport — plus OVERSCAN_ROWS above and below so a
// fast scroll never shows a blank band — are turned into DOM. Rows are
// fixed-height by construction (`.line` pins line-height and `.text` is
// `white-space: pre`, so the pane scrolls horizontally instead of wrapping),
// which makes a row's offset exactly `index * rowHeight` — no per-row
// measurement, no reading `scrollHeight`.
//
// Both views share this machinery; only what a "row" is differs. In the ASCII
// view a row is a log line, so the row count is the buffer length. In the hex
// view a row is 16 (or 8 / 32) bytes of the buffer's concatenated byte stream,
// so the row count comes from the byte index in
// `SerialDebugHexViewRenderer` — see its doc comment for why log-line indices
// cannot address hex rows.
const OVERSCAN_ROWS = 10;

// Inset of the hex dump from the top and bottom of its pane. It used to be the
// pane's own `p-3`; the vertical half now lives in the row model (added to the
// spacer height and to the window's translateY) because padding on the scroll
// box would not be part of `contentHeight` and would offset every row by a
// constant the model has to know anyway. The horizontal half stays a plain
// `px-3` on the pane.
const HEX_DUMP_INSET_Y = 12;

const rowProbeRef = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);
const measuredRowHeight = ref(0);

// Used until the probe has been measured (first frame) and in environments
// without a layout engine (happy-dom). ASCII mirrors the `.line` box —
// line-height 1.5 plus 2 x 0.1875rem of vertical padding. A hex row is a bare
// `<pre>` line: no padding, and the line-height it inherits from the pane's
// `text-xs` (4/3, unitless in Tailwind v4, so it tracks the font size).
function estimateRowHeight(fontSize: number): number {
  return props.hexView
    ? Math.round((fontSize * 4) / 3)
    : Math.round(fontSize * 1.5) + 6;
}

const rowHeight = computed(
  () => measuredRowHeight.value || estimateRowHeight(s.logFontSize),
);

// Row height must never be hard-coded: the font size is user-adjustable
// (10–18px). The probe is a real row rendered inside the pane it measures (a
// `.line` for the ASCII view, a one-line `<pre>` for the hex view — the two
// have different boxes), so it picks up every inherited style change. Only one
// of the two panes is ever mounted, so both probes can share the ref.
function measureLayout(): void {
  const paneEl = scrollRef.value;
  if (paneEl && paneEl.clientHeight > 0) {
    viewportHeight.value = paneEl.clientHeight;
  }
  // getBoundingClientRect, not offsetHeight: a fractional row height (e.g.
  // 19.5px at 13px font) rounded to an integer would drift the mounted rows out
  // of their computed slots across the window.
  const probeHeight = rowProbeRef.value?.getBoundingClientRect().height ?? 0;
  if (probeHeight > 0) measuredRowHeight.value = probeHeight;
}

let resizeObserver: ResizeObserver | null = null;
watch(
  [scrollRef, rowProbeRef],
  ([paneEl, probeEl]) => {
    if (typeof ResizeObserver === "undefined") return;
    resizeObserver ??= new ResizeObserver(() => measureLayout());
    resizeObserver.disconnect();
    if (paneEl) resizeObserver.observe(paneEl);
    if (probeEl) resizeObserver.observe(probeEl);
    measureLayout();
  },
  { flush: "post" },
);
onUnmounted(() => resizeObserver?.disconnect());

// Font size changed: fall back to the estimate for one frame, then re-measure.
watch(
  () => s.logFontSize,
  () => {
    measuredRowHeight.value = 0;
    void nextTick().then(measureLayout);
  },
);

// Hex rows are addressed by byte offset, so the count comes from the renderer's
// index rather than from the buffer length. Reading it also refreshes that index
// for `hexRendered` below.
const hexRowCount = computed(() =>
  props.hexView
    ? hexViewRenderer.rowCount(displayLines(), props.hexBytesPerRow)
    : 0,
);

const totalRows = computed(() =>
  props.hexView ? hexRowCount.value : displayLines().length,
);
const insetTop = computed(() => (props.hexView ? HEX_DUMP_INSET_Y : 0));
const contentHeight = computed(() =>
  totalRows.value === 0
    ? 0
    : totalRows.value * rowHeight.value + insetTop.value * 2,
);
const maxScrollTop = computed(() =>
  Math.max(0, contentHeight.value - viewportHeight.value),
);

const visibleRange = computed<{ start: number; end: number }>(() => {
  const total = totalRows.value;
  const rh = rowHeight.value;
  // Content space: row 0 starts `insetTop` below the top of the scroll box.
  // Ignoring the inset would misplace the window by less than one row, which
  // the overscan absorbs, but `maxScrollTop` above needs it to be exact.
  const top = Math.min(
    Math.max(scrollTop.value - insetTop.value, 0),
    maxScrollTop.value,
  );
  return {
    start: Math.max(0, Math.floor(top / rh) - OVERSCAN_ROWS),
    end: Math.min(
      total,
      Math.ceil((top + viewportHeight.value) / rh) + OVERSCAN_ROWS,
    ),
  };
});

const windowOffsetY = computed(
  () => visibleRange.value.start * rowHeight.value + insetTop.value,
);

function applyScrollTop(el: HTMLElement, top: number): void {
  // Keep the model in sync with the DOM: a programmatic scrollTop write fires
  // its `scroll` event asynchronously (and never at all in tests), so the
  // visible range would otherwise lag a frame behind.
  //
  // Read the value back instead of storing the requested one: the DOM clamps a
  // write to [0, scrollHeight - clientHeight], and a model holding the
  // unclamped request would compute a window the scrollbar is not actually on.
  // Callers may therefore ask for any number and let the DOM decide. The
  // read-back is not an extra reflow — assigning scrollTop already has to
  // resolve layout to clamp, so the value is there for the taking.
  el.scrollTop = top;
  scrollTop.value = el.scrollTop;
}

// Per-tab scroll lock. Key: activeChipId (null = "All" tab).
// Each tab maintains its own pause state independently.
const lockByTab = reactive(new Map<string | null, boolean>());

const lockAutoScroll = computed({
  get: (): boolean => lockByTab.get(s.activeChipId ?? null) ?? false,
  set: (val: boolean) => {
    lockByTab.set(s.activeChipId ?? null, val);
  },
});

async function scrollToBottom(): Promise<void> {
  await nextTick();
  const el = scrollRef.value;
  if (!el || lockAutoScroll.value) return;
  // Both panes derive the target from the row model: reading scrollHeight would
  // force a synchronous layout on every batch.
  applyScrollTop(el, maxScrollTop.value);
}

// Track the newest line id, not the array length: once the visible window is
// saturated (logWindowLines) the store drops one line off the head for
// every line it appends, so the length stops changing while new lines keep
// arriving — a length watcher goes permanently deaf at that point.
watch(
  () => {
    const current = displayLines();
    return current[current.length - 1]?.id ?? -1;
  },
  () => {
    void scrollToBottom();
  },
);

// Tab change: scroll to bottom for the newly active tab (respects its own lock state).
watch(
  () => s.activeChipId,
  () => {
    void scrollToBottom();
  },
);

// hexView toggle swaps the scrollRef DOM node (v-if/v-else); scroll into the new
// element. The row box differs between the two panes, so drop the measurement
// and let the new pane's probe re-measure (the estimate covers the gap frame).
watch(
  () => props.hexView,
  () => {
    measuredRowHeight.value = 0;
    void nextTick().then(measureLayout);
    void scrollToBottom();
  },
);

// Layout changes (font size, search bar open/close) shift scrollHeight or clientHeight,
// which can fire a spurious scroll event that falsely locks auto-scroll.
// Capture the lock state before the DOM update and restore it after.
function watchLayoutChange(source: () => unknown): void {
  watch(
    source,
    () => {
      const wasLocked = lockAutoScroll.value;
      void nextTick().then(() => {
        if (!wasLocked) {
          lockAutoScroll.value = false;
          void scrollToBottom();
        }
      });
    },
    { flush: "sync" },
  );
}

watchLayoutChange(() => s.logFontSize);
// A different bytesPerRow re-cuts the byte stream into a different number of
// rows, so the whole scroll extent changes under the viewport.
watchLayoutChange(() => props.hexBytesPerRow);

function onScroll(): void {
  const el = scrollRef.value;
  if (!el) return;
  scrollTop.value = el.scrollTop;
  if (el.clientHeight > 0) viewportHeight.value = el.clientHeight;
  lockAutoScroll.value = maxScrollTop.value - scrollTop.value >= 80;
  void onScrollEdge();
}

// The badge is the only exit from a paged-back pane now, so it has to say which
// of the two things it does: unlock the live tail, or leave the older window the
// tab is parked on (the archive window on "All", older matches on a filter tab).
const scrollBadgeKey = computed(() =>
  (activeChip.value ? s.activeFilterPinned : s.historyMode)
    ? "serialDebug.log.backToLive"
    : "serialDebug.log.pausedScroll",
);

async function resumeScroll(): Promise<void> {
  // Without these the badge would scroll to the bottom of the *paged-back*
  // window instead of to the live tail — the buffer under the pane is a
  // different one, and on a filter tab it is pinned against the live refresh.
  if (s.historyMode) s.exitHistoryMode();
  if (s.activeFilterPinned) await s.loadActiveFilterTail();
  lockAutoScroll.value = false;
  await scrollToBottom();
}

// ── full-session scrollback ("All" tab) ─────────────────────────────────
// Scrolling past the top of the live ring buffer switches the pane over to a
// bounded sliding window on the Rust session archive; scrolling back past its
// end switches back. The two buffers are never concatenated — see
// `useSerialDebugHistory` for why that is not merely a simplification.

/**
 * How far the content above the viewport moved when `prepended` lines were
 * added at the head. In the ASCII view a row *is* a log line, so it is the line
 * count; in the hex view a row is `bytesPerRow` bytes of the concatenated byte
 * stream, so the answer is the row the previous first line — now at index
 * `prepended` — starts on.
 *
 * Read off the *new* buffer rather than as a `totalRows` delta on purpose: the
 * same step may also trim the window's tail to stay within budget, and a tail
 * trim shortens contentHeight below the viewport, where it must not be
 * compensated for. When nothing is trimmed the two agree to within the ≤1 row
 * of jitter a non-row-aligned byte prefix causes anyway.
 */
function headRowsForPrepend(prepended: number): number {
  return props.hexView
    ? hexViewRenderer.rowOfLine(displayLines(), props.hexBytesPerRow, prepended)
    : prepended;
}

/** Keep the line that was at `beforeTop` where it is after a head-side change. */
function compensateScroll(
  el: HTMLElement,
  beforeTop: number,
  rows: number,
): void {
  applyScrollTop(el, beforeTop + rows * rowHeight.value);
}

async function enterHistory(): Promise<void> {
  const el = scrollRef.value;
  if (!el) return;
  const wasLocked = lockAutoScroll.value;
  // Explicit, not incidental: the mode switch changes the id of the last line,
  // which is exactly what the follow-tail watcher tracks, so it fires once on
  // the switch. `scrollToBottom` bails out while the lock is set.
  lockAutoScroll.value = true;
  const entered = await s.enterHistoryMode(s.lines.length);
  if (!entered) {
    lockAutoScroll.value = wasLocked;
    return;
  }
  await nextTick();
  const offset = s.historyEntryOffsetLines;
  const row = props.hexView
    ? hexViewRenderer.rowOfLine(displayLines(), props.hexBytesPerRow, offset)
    : offset;
  applyScrollTop(el, row * rowHeight.value + insetTop.value);
}

async function loadOlderHistoryAtTop(): Promise<void> {
  const el = scrollRef.value;
  if (!el) return;
  const beforeTop = scrollTop.value;
  const { prepended, reanchored } = await s.loadOlderHistory();
  if (!prepended && !reanchored) return;
  await nextTick();
  // A re-anchor replaced the window instead of extending it, so there is no
  // old position to preserve; show the top of what was loaded.
  if (reanchored) {
    applyScrollTop(el, 0);
    return;
  }
  compensateScroll(el, beforeTop, headRowsForPrepend(prepended));
}

async function loadNewerHistoryAtBottom(): Promise<void> {
  const el = scrollRef.value;
  if (!el) return;
  const beforeTop = scrollTop.value;
  const before = displayLines();
  const dropped = await s.loadNewerHistory();
  // Appended rows land below the viewport; only what left the head moves it.
  if (dropped <= 0) return;
  const rows = props.hexView
    ? hexViewRenderer.rowOfLine(before, props.hexBytesPerRow, dropped)
    : dropped;
  await nextTick();
  compensateScroll(el, beforeTop, -rows);
}

/**
 * Back at the bottom of a paged-back filter tab: put the window on the newest
 * matches and start following them again. The mirror of the All tab leaving
 * history mode at the same edge.
 *
 * The scroll position is re-applied by hand because the window shrinks back to
 * one page here — the follow-tail watcher only fires when the id of the *last*
 * line changes, and re-anchoring on the tail usually leaves that line untouched.
 */
async function reanchorFilterTailAtBottom(): Promise<void> {
  await s.loadActiveFilterTail();
  await nextTick();
  const el = scrollRef.value;
  if (el) applyScrollTop(el, maxScrollTop.value);
}

async function loadOlderFilterMatchesAtTop(): Promise<void> {
  const el = scrollRef.value;
  const beforeTop = scrollTop.value;
  const prepended = await s.loadOlderActiveFilterMatches();
  if (prepended <= 0 || !el) return;
  await nextTick();
  compensateScroll(el, beforeTop, headRowsForPrepend(prepended));
}

/**
 * Infinite scroll. Re-entry is blocked three ways: by the in-flight read flag
 * (`historyLoading` / `activeFilterLoading`), by the compensation having moved
 * `scrollTop` away from the edge once it lands, and by the end stops
 * (`historyAtSessionStart` / `historyAtArchiveEnd` / `activeFilterFullyLoaded`).
 */
async function onScrollEdge(): Promise<void> {
  if (s.historyLoading) return;
  // Not scrollable at all: there is no "the user scrolled up" to react to, and
  // entering history mode here would lock auto-scroll for no reason.
  if (maxScrollTop.value <= 0) return;
  // Filter tabs page backwards through their own match list, not the archive
  // window, so they stop here — there is no history mode to enter and no
  // bottom-edge case (the tail is reloaded on tab switch and live refresh).
  if (s.activeChipId !== null) {
    if (scrollTop.value <= rowHeight.value) {
      if (!s.activeFilterLoading && !s.activeFilterFullyLoaded) {
        await loadOlderFilterMatchesAtTop();
      }
      return;
    }
    if (
      maxScrollTop.value - scrollTop.value <= rowHeight.value &&
      s.activeFilterPinned &&
      !s.activeFilterLoading
    ) {
      await reanchorFilterTailAtBottom();
    }
    return;
  }
  if (scrollTop.value <= rowHeight.value) {
    if (!s.historyMode) {
      await enterHistory();
    } else if (!s.historyAtSessionStart) {
      await loadOlderHistoryAtTop();
    }
    return;
  }
  if (!s.historyMode) return;
  if (maxScrollTop.value - scrollTop.value > rowHeight.value) return;
  if (s.historyAtArchiveEnd) {
    await resumeScroll();
  } else {
    await loadNewerHistoryAtBottom();
  }
}

/**
 * Ctrl+F keeps its "within the buffer currently on screen" meaning in history
 * mode. Whole-session search already exists as a filter chip (it is archive
 * backed, with an authoritative count and its own tab), so offer that instead of
 * building a second scan pipeline.
 */
async function searchWholeSession(): Promise<void> {
  const keyword = searchText.value.trim();
  if (!keyword) return;
  const existing = s.watchChips.find(
    (c) => c.keyword === keyword && !c.useRegex,
  );
  if (existing) {
    await s.setActiveChip(existing.id);
  } else if ((await s.addChip(keyword, false)) !== "ok") {
    return;
  }
  closeSearch();
}

// Only the windowed rows are formatted, and the result is byte-identical to the
// matching slice of the full-buffer dump — the view stays one continuous dump.
const hexRendered = computed(() => {
  if (!props.hexView) return null;
  const { start, end } = visibleRange.value;
  return hexViewRenderer.renderRows(
    displayLines(),
    props.hexBytesPerRow,
    start,
    end,
  );
});

function spanStyle(style: AnsiStyle): Record<string, string | undefined> {
  return {
    color: style.fg,
    backgroundColor: style.bg,
    fontWeight: style.bold ? "bold" : undefined,
    fontStyle: style.italic ? "italic" : undefined,
    textDecoration: style.underline ? "underline" : undefined,
  };
}

const ctxMenu = ref<{ x: number; y: number; selected: string } | null>(null);

// Known limitation of the virtualized pane: the native selection can only cover
// mounted rows, so a selection dragged past the viewport stops at the rows that
// happen to be in the DOM. Selecting inside the viewport (the copy / to-hex /
// to-ascii actions below) works as before. A selection model spanning unmounted
// rows would need its own text layer and is deliberately out of scope.
function onContextMenu(ev: MouseEvent): void {
  const sel = window.getSelection()?.toString() ?? "";
  if (!sel) {
    ctxMenu.value = null;
    return;
  }
  ev.preventDefault();
  ctxMenu.value = { x: ev.clientX, y: ev.clientY, selected: sel };
}

function copy(): void {
  if (!ctxMenu.value) return;
  void navigator.clipboard.writeText(ctxMenu.value.selected);
  ctxMenu.value = null;
}

function showCtxPopup(mode: "hex" | "ascii"): void {
  if (!ctxMenu.value) return;
  const bytes = new TextEncoder().encode(ctxMenu.value.selected);
  s.showHexPopup(bytes, mode);
  ctxMenu.value = null;
}

function dismissCtx(): void {
  ctxMenu.value = null;
}

let savedScrollTop: number | null = null;

onDeactivated(() => {
  ctxMenu.value = null;
  savedScrollTop = scrollRef.value?.scrollTop ?? null;
});

onActivated(async () => {
  await nextTick();
  const el = scrollRef.value;
  if (!el) return;
  measureLayout();
  if (lockAutoScroll.value && savedScrollTop !== null) {
    applyScrollTop(el, savedScrollTop);
  } else if (!lockAutoScroll.value) {
    applyScrollTop(el, maxScrollTop.value);
  }
  savedScrollTop = null;
});

const containerRef = ref<HTMLDivElement | null>(null);
const searchOpen = ref(false);
watchLayoutChange(() => searchOpen.value);
const searchText = ref("");
const searchIndex = ref(0);
const searchInputRef = ref<HTMLInputElement | null>(null);

const searchQuery = computed(() => searchText.value.trim().toLowerCase());

// Cache eviction must not hang off either template branch. Two paths fill the
// renderer's per-line cache — the span building below (ASCII branch only) and
// the whole-buffer match scan (runs in both branches, since the search bar sits
// above the pane) — so tying eviction to the ASCII render would leak every line
// that ages out of the buffer while the hex view is up with a search active.
// This effect re-runs on every buffer publish regardless of what is mounted,
// and it replaces (not adds to) the per-render prune the old code did: it no
// longer re-scans on a pure scroll, when only the visible slice moves.
watchEffect(() => lineRenderer.retain(displayLines()));

// Spans are built for the mounted slice only. `visibleRange` counts hex rows
// when the hex view is up, which would address the wrong lines — that branch
// mounts no `.line` at all, so bail out rather than build spans nothing reads.
const visibleLineViews = computed<RenderedLogLine[]>(() => {
  if (props.hexView) return [];
  const all = displayLines();
  const { start, end } = visibleRange.value;
  return lineRenderer.render(
    all.slice(start, end),
    props.ansiEnabled,
    searchQuery.value,
  );
});

// Search stays whole-buffer: matching is a plain substring scan over every
// line, decoupled from the span building above, so the count and the
// prev/next order do not degrade to "whatever happens to be on screen".
const matchingLineIdList = computed<number[]>(() =>
  lineRenderer.matchingLineIds(displayLines(), searchQuery.value),
);

const matchingLineIds = computed<Set<number>>(
  () => new Set(matchingLineIdList.value),
);
const matchCount = computed(() => matchingLineIdList.value.length);

const currentMatchLineId = computed<number | null>(() => {
  const list = matchingLineIdList.value;
  if (!list.length) return null;
  return list[searchIndex.value % list.length];
});

const canSaveLog = computed(() => {
  if (!activeChip.value) {
    return linesAvailableForSession();
  }
  const stats = s.filterStatsById[activeChip.value.id];
  return stats?.status === "complete" && stats.totalMatches > 0;
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
  searchText.value = "";
  searchIndex.value = 0;
  containerRef.value?.focus();
}

async function navigateSearch(delta: number): Promise<void> {
  const count = matchCount.value;
  if (!count) return;
  searchIndex.value = (((searchIndex.value + delta) % count) + count) % count;
  await scrollToMatch();
}

// Scroll by index, not by element lookup: a match outside the mounted window
// has no DOM node to scroll into view.
async function scrollToMatch(): Promise<void> {
  const id = currentMatchLineId.value;
  if (id === null) return;
  await nextTick();
  const el = scrollRef.value;
  if (!el) return;
  const lines = displayLines();
  const index = lines.findIndex((line) => line.id === id);
  if (index < 0) return;
  const rh = rowHeight.value;
  // In the hex view a scroll offset is a byte offset, not a line index: map the
  // matched line to the hex row its first byte lands on.
  const row = props.hexView
    ? hexViewRenderer.rowOfLine(lines, props.hexBytesPerRow, index)
    : index;
  const rowTop = row * rh + insetTop.value;
  const current = scrollTop.value;
  // Equivalent of scrollIntoView({ block: "nearest" }): only move when the row
  // sits outside the viewport.
  let next = current;
  if (rowTop < current) {
    next = rowTop;
  } else if (rowTop + rh > current + viewportHeight.value) {
    next = rowTop + rh - viewportHeight.value;
  }
  if (next === current) return;
  applyScrollTop(el, next);
}

function onContainerKeydown(ev: KeyboardEvent): void {
  if (ev.ctrlKey && ev.key === "f") {
    ev.preventDefault();
    void openSearch();
  }
}

function onSearchKeydown(ev: KeyboardEvent): void {
  if (ev.key === "Escape") {
    ev.preventDefault();
    closeSearch();
  } else if (ev.key === "Enter") {
    ev.preventDefault();
    void navigateSearch(ev.shiftKey ? -1 : 1);
  }
}

async function writeFile(
  defaultName: string,
  chunks: string[],
  _ext: string,
  mimeType: string,
): Promise<void> {
  const content = chunks.join("");
  const blob = new Blob([content], { type: `${mimeType};charset=utf-8` });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = defaultName;
  a.click();
  URL.revokeObjectURL(url);
}

function linesAvailableForSession(): boolean {
  return s.lines.length > 0;
}

async function streamExportChunks(
  onChunk: (chunk: string, isFirstChunk: boolean) => Promise<void>,
): Promise<void> {
  let start = 0;
  let wroteAny = false;
  if (activeChip.value) {
    while (true) {
      const page = await transport.readFilterMatches(
        activeChip.value.id,
        start,
        EXPORT_PAGE_SIZE,
      );
      if (page.items.length === 0) break;
      const chunk =
        (wroteAny ? "\n" : "") + page.items.map(formatExportLine).join("\n");
      await onChunk(chunk, !wroteAny);
      wroteAny = true;
      start += page.items.length;
      if (start >= page.totalMatches) break;
    }
    return;
  }

  while (true) {
    const page = await transport.readSessionPage(start, EXPORT_PAGE_SIZE);
    if (page.items.length === 0) break;
    const chunk =
      (wroteAny ? "\n" : "") + page.items.map(formatExportLine).join("\n");
    await onChunk(chunk, !wroteAny);
    wroteAny = true;
    start += page.items.length;
    if (start >= page.totalLines) break;
  }
}

async function saveLog(): Promise<void> {
  const defaultName = `${props.exportTitle}-${makeStamp()}.txt`;
  if (isTauriRuntime()) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const { invoke } = await import("@tauri-apps/api/core");
    const path = await save({
      defaultPath: defaultName,
      filters: [{ name: "TXT", extensions: ["txt"] }],
    });
    if (!path) return;
    // Authorize this dialog-chosen save path for the chunked writes below.
    await invoke("register_dialog_path", { path });
    await streamExportChunks(async (chunk, isFirstChunk) => {
      if (isFirstChunk) {
        await invoke("write_text_file", { path, content: chunk });
      } else {
        await invoke("append_text_file", { path, content: chunk });
      }
    });
    return;
  }

  const chunks: string[] = [];
  await streamExportChunks(async (chunk) => {
    chunks.push(chunk);
  });
  await writeFile(defaultName, chunks, "txt", "text/plain");
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
    <div
      class="log-toolbar flex items-center gap-2 border-b border-[var(--ty-border)] px-3 py-1.5"
    >
      <span class="toolbar-title">{{ t("serialDebug.log.title") }}</span>

      <!-- display toggles: timestamp & direction badge — left side, next to title -->
      <button
        type="button"
        class="btn-tool"
        :class="{ 'btn-tool-active': s.showTimestamp }"
        :aria-label="t('serialDebug.log.toggleTimestamp')"
        @click="s.showTimestamp = !s.showTimestamp"
      >
        <FontAwesomeIcon :icon="['fas', 'clock']" class="size-3 shrink-0" />
        {{ t("serialDebug.log.toggleTimestamp") }}
      </button>
      <button
        type="button"
        class="btn-tool"
        :class="{ 'btn-tool-active': s.showDirBadge }"
        :aria-label="t('serialDebug.log.toggleDirBadge')"
        @click="s.showDirBadge = !s.showDirBadge"
      >
        <FontAwesomeIcon :icon="['fas', 'tag']" class="size-3 shrink-0" />
        {{ t("serialDebug.log.toggleDirBadge") }}
      </button>

      <button
        v-if="lockAutoScroll"
        type="button"
        class="paused-badge"
        :aria-label="t(scrollBadgeKey)"
        @click="resumeScroll"
      >
        <FontAwesomeIcon
          :icon="['fas', 'arrow-down']"
          class="size-3 shrink-0"
        />
        {{ t(scrollBadgeKey) }}
      </button>

      <div class="ml-auto flex items-center gap-1">
        <!-- auto-save toggle. Green tracks the *setting*, the label tracks
             whether a file is actually being written (needs an open port). -->
        <button
          type="button"
          class="autosave-toggle"
          :class="{ 'autosave-toggle--on': s.autoSave }"
          :disabled="!isTauriRuntime()"
          :aria-pressed="s.autoSave"
          :aria-label="t('serialDebug.autoSave.label')"
          @click="s.setAutoSaveEnabled(!s.autoSave)"
        >
          <FontAwesomeIcon
            :icon="s.sessionAutoSavePath ? faCircleNotch : faFloppyDisk"
            class="size-3 shrink-0"
            :class="{ 'fa-spin': s.sessionAutoSavePath }"
          />
          {{
            s.sessionAutoSavePath
              ? t("serialDebug.autoSave.active")
              : t("serialDebug.autoSave.off")
          }}
        </button>

        <span class="toolbar-divider" />

        <!-- font size stepper -->
        <div class="font-size-stepper">
          <button
            type="button"
            class="stepper-btn"
            :disabled="s.logFontSize <= 10"
            :aria-label="t('serialDebug.log.fontDecrease')"
            @click="s.decreaseFontSize"
          >
            <FontAwesomeIcon
              :icon="['fas', 'magnifying-glass-minus']"
              class="size-3 shrink-0"
            />
          </button>
          <span class="font-size-label"
            >{{ t("serialDebug.log.fontSize") }} {{ s.logFontSize }}</span
          >
          <button
            type="button"
            class="stepper-btn"
            :disabled="s.logFontSize >= 18"
            :aria-label="t('serialDebug.log.fontIncrease')"
            @click="s.increaseFontSize"
          >
            <FontAwesomeIcon
              :icon="['fas', 'magnifying-glass-plus']"
              class="size-3 shrink-0"
            />
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
          <FontAwesomeIcon
            :icon="['fas', 'magnifying-glass']"
            class="size-3 shrink-0"
          />
          {{ t("serialDebug.log.filterToggle") }}
        </button>

        <span class="toolbar-divider" />

        <!-- action group: save & clear -->
        <button
          type="button"
          class="btn-tool"
          :aria-label="t('serialDebug.log.saveLog')"
          :disabled="!canSaveLog"
          @click="saveLog"
        >
          <FontAwesomeIcon
            :icon="['fas', 'download']"
            class="size-3 shrink-0"
          />
          {{ t("serialDebug.log.saveLog") }}
        </button>
        <button
          type="button"
          class="btn-tool"
          :aria-label="t('serialDebug.conn.clear')"
          @click="emit('clear')"
        >
          <FontAwesomeIcon
            :icon="['fas', 'trash-can']"
            class="size-3 shrink-0"
          />
          {{ t("serialDebug.conn.clear") }}
        </button>
      </div>
    </div>

    <!-- chip bar -->
    <SerialDebugChipBar />

    <!-- Older matches load themselves when the pane reaches the top; this only
         says so, and says when a read is in flight. Rendered on the same
         condition as before (not only while loading) so it appears and
         disappears once per tab, never on every page — a strip that came and
         went mid-read would resize the pane under the scroll compensation. -->
    <div
      v-if="activeChip && !s.activeFilterFullyLoaded"
      class="load-hint flex items-center gap-1 border-b border-[var(--ty-border)] px-3 py-1"
    >
      <FontAwesomeIcon
        :icon="s.activeFilterLoading ? faCircleNotch : ['fas', 'arrow-up']"
        class="size-3 shrink-0"
        :class="{ 'fa-spin': s.activeFilterLoading }"
      />
      {{
        s.activeFilterLoading
          ? t("serialDebug.log.loadingOlderMatches")
          : t("serialDebug.log.scrollForOlderMatches")
      }}
    </div>

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
          {{
            t("serialDebug.search.count", {
              current: (searchIndex % matchCount) + 1,
              total: matchCount,
            })
          }}
        </template>
        <template v-else-if="searchText.trim()">
          {{ t("serialDebug.search.noMatch") }}
        </template>
      </span>
      <!-- In history mode the pane holds a window on the archive, not the live
           buffer, so say so rather than let the count read as session-wide. -->
      <span v-if="!activeChip && s.historyMode" class="search-scope-hint">
        {{ t("serialDebug.search.scopeHistory") }}
      </span>
      <button
        v-if="searchText.trim()"
        type="button"
        class="btn-tool"
        :aria-label="t('serialDebug.search.wholeSession')"
        @click="searchWholeSession"
      >
        <FontAwesomeIcon :icon="['fas', 'filter']" class="size-3 shrink-0" />
        {{ t("serialDebug.search.wholeSession") }}
      </button>
      <button
        type="button"
        class="btn-tool"
        :aria-label="t('serialDebug.search.prev')"
        :disabled="matchCount === 0"
        @click="navigateSearch(-1)"
      >
        <FontAwesomeIcon
          :icon="['fas', 'chevron-up']"
          class="size-3 shrink-0"
        />
        {{ t("serialDebug.search.prev") }}
      </button>
      <button
        type="button"
        class="btn-tool"
        :aria-label="t('serialDebug.search.next')"
        :disabled="matchCount === 0"
        @click="navigateSearch(1)"
      >
        <FontAwesomeIcon
          :icon="['fas', 'chevron-down']"
          class="size-3 shrink-0"
        />
        {{ t("serialDebug.search.next") }}
      </button>
      <button
        type="button"
        class="btn-tool"
        :aria-label="t('serialDebug.search.close')"
        @click="closeSearch"
      >
        <FontAwesomeIcon :icon="['fas', 'xmark']" class="size-3 shrink-0" />
        {{ t("serialDebug.search.close") }}
      </button>
    </div>

    <!-- hex view -->
    <div
      v-if="hexView"
      ref="scrollRef"
      class="pane flex-1 overflow-auto px-3 font-mono text-xs"
      :style="{ fontSize: s.logFontSize + 'px' }"
      @scroll="onScroll"
    >
      <!-- Row-height probe: one real dump row, measured so the virtual scroller
           tracks the current font size instead of a hard-coded value. -->
      <pre ref="rowProbeRef" class="hex-probe" aria-hidden="true">00</pre>
      <div class="virt-spacer" :style="{ height: contentHeight + 'px' }">
        <div
          class="virt-window"
          :style="{ transform: `translateY(${windowOffsetY}px)` }"
        >
          <pre class="whitespace-pre">{{ hexRendered }}</pre>
        </div>
      </div>
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
      <!-- Row-height probe: an invisible real row, measured so the virtual
           scroller tracks the current font size instead of a hard-coded value. -->
      <div ref="rowProbeRef" class="line line-probe" aria-hidden="true">
        <span class="prefix">
          <span class="ts">00:00:00.000</span>
          <span class="dir-badge">RX</span>
        </span>
        <span class="text">0</span>
      </div>
      <div class="virt-spacer" :style="{ height: contentHeight + 'px' }">
        <div
          class="virt-window"
          :style="{ transform: `translateY(${windowOffsetY}px)` }"
        >
          <div
            v-for="lineView in visibleLineViews"
            :key="lineView.line.id"
            :data-line-id="lineView.line.id"
            class="line"
            :data-dir="lineView.line.direction"
            :class="{
              'line-search-match':
                matchingLineIds.has(lineView.line.id) &&
                lineView.line.id !== currentMatchLineId,
              'line-search-current': lineView.line.id === currentMatchLineId,
            }"
          >
            <span v-if="s.showTimestamp || s.showDirBadge" class="prefix">
              <span v-if="s.showTimestamp" class="ts">{{
                formatTs(lineView.line.tsMs)
              }}</span>
              <span v-if="s.showDirBadge" class="dir-badge">{{
                lineView.line.direction === "tx"
                  ? "TX"
                  : lineView.line.direction === "rx"
                    ? "RX"
                    : "SYS"
              }}</span>
            </span>
            <span class="text">
              <span
                v-for="(span, si) in lineView.spans"
                :key="si"
                :style="spanStyle(span.style)"
              >
                <template
                  v-if="searchQuery && matchingLineIds.has(lineView.line.id)"
                >
                  <template v-for="(seg, sj) in span.segments" :key="sj">
                    <mark v-if="seg.isMatch" class="search-keyword-mark">{{
                      seg.text
                    }}</mark
                    ><template v-else>{{ seg.text }}</template>
                  </template>
                </template>
                <template v-else>{{ span.text }}</template>
              </span>
            </span>
          </div>
        </div>
      </div>
      <div
        v-if="contentHeight === 0"
        class="px-3 py-2 text-[var(--ty-text-muted)]"
      >
        {{ t("serialDebug.log.waitingData") }}
      </div>
    </div>

    <!-- right-click context menu -->
    <div
      v-if="ctxMenu"
      class="ctx-menu fixed z-50 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface)] py-1 shadow-lg"
      :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
      @click.stop
    >
      <button type="button" class="menu-item" @click="copy">
        {{ t("serialDebug.hexPopup.copy") }}
      </button>
      <button type="button" class="menu-item" @click="showCtxPopup('hex')">
        {{ t("serialDebug.hexPopup.toHex") }}
      </button>
      <button type="button" class="menu-item" @click="showCtxPopup('ascii')">
        {{ t("serialDebug.hexPopup.toAscii") }}
      </button>
    </div>
    <div
      v-if="ctxMenu"
      class="fixed inset-0 z-40"
      @click="dismissCtx"
      @contextmenu.prevent="dismissCtx"
    />
  </div>
</template>

<style scoped>
/* toolbar */
.log-toolbar {
  background: var(--ty-surface);
}
.toolbar-title {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--ty-text-muted);
  white-space: nowrap;
}
.toolbar-divider {
  width: 1px;
  height: 1rem;
  background: var(--ty-border);
  flex-shrink: 0;
  margin: 0 0.125rem;
}
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
  transition:
    background-color 0.15s ease,
    color 0.15s ease;
}
.stepper-btn:hover {
  background: var(--ty-surface-muted);
  color: var(--ty-text);
}
.stepper-btn:disabled {
  cursor: not-allowed;
  opacity: 0.4;
}
.font-size-label {
  font-size: 0.75rem;
  color: var(--ty-text-muted);
  min-width: 4rem;
  text-align: center;
  font-variant-numeric: tabular-nums;
  border-left: 1px solid var(--ty-border);
  border-right: 1px solid var(--ty-border);
  padding: 0 0.375rem;
  white-space: nowrap;
}
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
  transition:
    background-color 0.15s ease,
    color 0.15s ease,
    border-color 0.15s ease;
}
.btn-tool:hover {
  background: var(--ty-surface-muted);
  color: var(--ty-text);
}
.btn-tool:disabled {
  cursor: not-allowed;
  opacity: 0.4;
}
.btn-tool-active {
  color: var(--ty-primary);
  border-color: var(--ty-primary);
}
.paused-badge {
  font-size: 0.7rem;
  padding: 0.2rem 0.5rem;
  border-radius: 9999px;
  background: color-mix(in srgb, var(--ty-accent, #f97316) 15%, transparent);
  color: var(--ty-accent, #f97316);
  border: 1px solid var(--ty-accent, #f97316);
  cursor: pointer;
  white-space: nowrap;
  transition:
    background-color 0.15s ease,
    opacity 0.15s ease;
}
.paused-badge:hover {
  background: color-mix(in srgb, var(--ty-accent, #f97316) 25%, transparent);
}
.autosave-toggle {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.125rem 0.5rem;
  border: 1px solid transparent;
  border-radius: 0.375rem;
  background: transparent;
  cursor: pointer;
  font-size: 0.8125rem;
  white-space: nowrap;
  color: var(--ty-text-muted);
  opacity: 0.55;
  transition:
    background-color 0.15s ease,
    color 0.15s ease,
    border-color 0.15s ease,
    opacity 0.15s ease;
}
.autosave-toggle:hover:not(:disabled) {
  background: var(--ty-surface-muted);
  opacity: 1;
}
.autosave-toggle:disabled {
  cursor: not-allowed;
}
.autosave-toggle--on {
  color: var(--ty-success);
  border-color: var(--ty-success);
  opacity: 1;
}
/* search bar */
.search-bar {
  background: var(--ty-surface);
}
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
.search-input:focus {
  border-color: var(--ty-primary);
}
.match-count {
  font-size: 0.75rem;
  color: var(--ty-text-muted);
  white-space: nowrap;
  min-width: 5rem;
}
.search-scope-hint {
  font-size: 0.75rem;
  color: var(--ty-text-muted);
  white-space: nowrap;
}
/* "scroll up for more" / "loading" strip above the pane */
.load-hint {
  background: var(--ty-surface);
  font-size: 0.75rem;
  color: var(--ty-text-muted);
  white-space: nowrap;
  user-select: none;
}
/* virtualized line window: the spacer carries the full scroll height, the
   window holds the mounted slice and is offset to the slice's first row. */
.virt-spacer {
  position: relative;
}
.virt-window {
  position: absolute;
  top: 0;
  left: 0;
  width: max-content;
  min-width: 100%;
}
.line-probe,
.hex-probe {
  position: absolute;
  top: 0;
  visibility: hidden;
  pointer-events: none;
  user-select: none;
}
/* log lines */
.line {
  display: flex;
  align-items: baseline;
  gap: 0.625rem;
  padding: 0.1875rem 0.625rem;
  font-size: inherit;
  /* Fixed row height is what makes index-based virtual scrolling correct;
     keep this in sync with estimateRowHeight() in the script block. */
  line-height: 1.5;
}
.line[data-dir="tx"] {
}
.line[data-dir="rx"] {
}
.line[data-dir="sys"] {
  color: var(--ty-text-muted);
  font-style: italic;
}
.line-search-match {
  background: color-mix(in srgb, var(--ty-primary) 12%, transparent) !important;
}
.line-search-current {
  background: color-mix(in srgb, var(--ty-primary) 28%, transparent) !important;
  outline: 1px solid color-mix(in srgb, var(--ty-primary) 45%, transparent);
  outline-offset: -1px;
}
.prefix {
  display: flex;
  align-items: baseline;
  gap: 0.25rem;
  flex-shrink: 0;
  white-space: nowrap;
}
.ts {
  color: var(--ty-text-muted);
  font-size: 0.85em;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.01em;
}
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
.line[data-dir="tx"] .dir-badge {
  background: color-mix(in srgb, var(--ty-primary) 20%, transparent);
  color: var(--ty-primary);
}
.line[data-dir="rx"] .dir-badge {
  background: color-mix(in srgb, var(--ty-success) 20%, transparent);
  color: var(--ty-success);
}
.line[data-dir="sys"] .dir-badge {
  background: color-mix(in srgb, var(--ty-text-muted) 15%, transparent);
  color: var(--ty-text-muted);
}
.text {
  /* `pre`, not `pre-wrap`: soft wrapping would make row heights vary and break
     the fixed-height row model. Long lines scroll horizontally instead. */
  white-space: pre;
}
mark.search-keyword-mark {
  background: #fbbf24;
  color: #1c1917;
  border-radius: 0.125rem;
  padding: 0 0.1rem;
}
.menu-item {
  display: block;
  width: 100%;
  text-align: left;
  padding: 0.375rem 0.75rem;
  font-size: 0.8125rem;
  cursor: pointer;
}
.menu-item:hover {
  background: var(--ty-surface-muted);
}
</style>
