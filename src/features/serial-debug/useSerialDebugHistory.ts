/**
 * Full-session scrollback for the serial-debug "All" tab.
 *
 * The live view is a ring buffer capped at `logWindowLines`; everything older
 * only exists in the Rust session archive. This composable owns a *second*,
 * completely separate buffer: a bounded sliding window over that archive,
 * anchored by archive `lineNo`. The two never mix — archive lines are never
 * pushed into the live ring (`trimVisibleLines` would eat them off the head on
 * the next batch anyway), and the live ring is never paged from the archive.
 *
 * They are kept apart because the two line streams provably diverge, so
 * "frontend line index == archive line_no" is not a usable anchor:
 *
 *  - a dropped chunk (bounded backend queue) is written to the archive but never
 *    reaches the frontend, so the archive gains lines the live view never saw;
 *  - once the archive hits its size cap it stops recording and `totalLines`
 *    freezes while the live view keeps growing;
 *  - the archive never contains the still-unterminated trailing line, so it is
 *    permanently 0-2 lines short of the live view.
 *
 * Splicing the two on that assumption would silently show wrong content exactly
 * in the sessions where scrollback matters most (fast port, long run, dropped
 * bytes). Switching between them instead makes each mode internally consistent;
 * the only approximation left is where the viewport lands on the frame the mode
 * changes, which is a presentation detail the position readout compensates for.
 *
 * Because the window is one contiguous `DebugLogLine[]`, every consumer — ASCII
 * virtualization, the hex byte index, Ctrl+F, the renderer cache eviction — sees
 * the exact shape a filter tab already hands it and needs no change.
 */
import { computed, ref, shallowRef } from "vue";
import { HISTORY_ENTRY_PAGES, HISTORY_PAGE_SIZE } from "./constants";
import { localizeArchiveLineText } from "./archive-line-text";
import { archiveLineToLogLine } from "./utils";
import type {
  DebugLogLine,
  SerialDebugLine,
  SerialDebugSessionPage,
} from "./types";

export interface SerialDebugHistoryDeps {
  /** Reads `[start, start + limit)` of the session archive (0-based `start`). */
  readSessionPage: (
    start: number,
    limit: number,
  ) => Promise<SerialDebugSessionPage>;
  /** Store-owned memo handing out a stable display id per archive line number. */
  archiveLineId: (lineNo: number) => number;
  /** Upper bound on window size — reuses the `logWindowLines` setting. */
  historyBudget: () => number;
  /** Surfaces a read failure to the user; receives the raw error message. */
  reportError: (message: string) => void;
}

/** Outcome of one "load older" step, as the scroll compensation needs it. */
export interface HistoryOlderResult {
  /** Lines prepended to the head of the window (0 when nothing was added). */
  prepended: number;
  /** The window was re-anchored on the archive tail instead of being extended. */
  reanchored: boolean;
}

export function useSerialDebugHistory(deps: SerialDebugHistoryDeps) {
  const historyMode = ref(false);
  // shallowRef for the same reason the live buffer is one: these arrays hold up
  // to `logWindowLines` never-mutated objects, and they are always replaced
  // wholesale here (never spliced in place), so no `triggerRef` is needed.
  const historyLines = shallowRef<DebugLogLine[]>([]);
  /** Archive line number of `historyLines[0]`; 0 when the window is empty. */
  const historyStartLineNo = ref(0);
  const historyTotalLines = ref(0);
  const historyLoading = ref(false);
  const historyAtSessionStart = ref(false);
  const historyAtArchiveEnd = ref(true);
  /**
   * Where to put the viewport on the frame history mode is entered, as an index
   * into the window. The store produces a line index (pure data); turning it
   * into a `scrollTop` needs the row height and the hex/ASCII row model, so that
   * half stays in the component.
   */
  const historyEntryOffsetLines = ref(0);

  const historyEndLineNo = computed(() =>
    historyLines.value.length === 0
      ? 0
      : historyStartLineNo.value + historyLines.value.length - 1,
  );

  // Bumped whenever the window is torn down, so an in-flight read that resolves
  // after `clear()` (new session, line numbers restart at 1) cannot resurrect a
  // window anchored in the old session.
  let generation = 0;

  function budget(): number {
    return Math.max(deps.historyBudget(), HISTORY_PAGE_SIZE);
  }

  function toDisplayLines(items: readonly SerialDebugLine[]): DebugLogLine[] {
    return items.map((line) =>
      archiveLineToLogLine(
        { ...line, text: localizeArchiveLineText(line.direction, line.text) },
        deps.archiveLineId(line.lineNo),
      ),
    );
  }

  /**
   * Reads a range in `HISTORY_PAGE_SIZE` pieces. One big request would hold the
   * Rust-side archive lock for the whole range (the `limit` is unbounded on
   * every layer), which at a fast baud rate is a visible write stall.
   */
  async function readRange(
    start: number,
    limit: number,
  ): Promise<{ items: SerialDebugLine[]; totalLines: number }> {
    const items: SerialDebugLine[] = [];
    let totalLines = 0;
    while (items.length < limit) {
      const page = await deps.readSessionPage(
        start + items.length,
        Math.min(HISTORY_PAGE_SIZE, limit - items.length),
      );
      totalLines = page.totalLines;
      if (page.items.length === 0) break;
      items.push(...page.items);
    }
    return { items, totalLines };
  }

  function applyWindow(
    items: readonly SerialDebugLine[],
    totalLines: number,
    atArchiveEnd: boolean,
  ): void {
    historyLines.value = toDisplayLines(items);
    historyStartLineNo.value = items[0].lineNo;
    historyTotalLines.value = totalLines;
    // `page.start` is useless for this: an out-of-range `start` is silently
    // clamped and the clamped value is what comes back. The line numbers are
    // the only thing that says where we actually are.
    historyAtSessionStart.value = items[0].lineNo === 1;
    historyAtArchiveEnd.value = atArchiveEnd;
  }

  /** Loads the last `HISTORY_ENTRY_PAGES` pages of the archive into the window. */
  async function loadTailWindow(): Promise<boolean> {
    // One cheap read just to learn `totalLines`; the range read follows.
    const probe = await deps.readSessionPage(0, 1);
    const total = probe.totalLines;
    historyTotalLines.value = total;
    if (total === 0) return false;
    const want = Math.min(
      HISTORY_PAGE_SIZE * HISTORY_ENTRY_PAGES,
      total,
      budget(),
    );
    const { items, totalLines } = await readRange(total - want, want);
    if (items.length === 0) return false;
    // Deliberately `true` even if the archive grew during the reads: the window
    // ends where the archive ended when the user asked for it, and "scroll back
    // down to leave history mode" must stay reachable on a live port.
    applyWindow(items, Math.max(totalLines, total), true);
    return true;
  }

  function fail(e: unknown): void {
    exitHistoryMode();
    deps.reportError(e instanceof Error ? e.message : String(e));
  }

  /**
   * @param liveLineCount how many lines the live buffer currently holds; the
   * window is positioned so the viewport starts roughly where the live buffer
   * did, i.e. `windowLength - liveLineCount` lines from its head.
   */
  async function enterHistoryMode(liveLineCount: number): Promise<boolean> {
    if (historyMode.value || historyLoading.value) return false;
    const gen = generation;
    historyLoading.value = true;
    try {
      const ok = await loadTailWindow();
      if (gen !== generation || !ok) return false;
      const windowLength = historyLines.value.length;
      historyEntryOffsetLines.value = Math.min(
        Math.max(windowLength - liveLineCount, 0),
        Math.max(windowLength - 1, 0),
      );
      historyMode.value = true;
      return true;
    } catch (e) {
      fail(e);
      return false;
    } finally {
      historyLoading.value = false;
    }
  }

  async function loadOlderHistory(): Promise<HistoryOlderResult> {
    const none: HistoryOlderResult = { prepended: 0, reanchored: false };
    if (
      !historyMode.value ||
      historyLoading.value ||
      historyAtSessionStart.value
    ) {
      return none;
    }
    const firstLineNo = historyStartLineNo.value;
    const want = Math.min(HISTORY_PAGE_SIZE, firstLineNo - 1);
    if (want <= 0) {
      historyAtSessionStart.value = true;
      return none;
    }
    const gen = generation;
    historyLoading.value = true;
    try {
      const page = await deps.readSessionPage(firstLineNo - 1 - want, want);
      if (gen !== generation) return none;
      historyTotalLines.value = Math.max(
        page.totalLines,
        historyTotalLines.value,
      );
      const items = page.items;
      if (items.length === 0) {
        historyAtSessionStart.value = true;
        return none;
      }
      // Continuity self-check. Today the archive never renumbers (`stopWriting`
      // when full, never `dropOldest`), so this cannot fire; keeping it means a
      // future head-dropping policy degrades this feature to "re-anchors once"
      // rather than "silently shows the wrong lines".
      if (items[items.length - 1].lineNo !== firstLineNo - 1) {
        const ok = await loadTailWindow();
        if (gen !== generation) return none;
        if (!ok) exitHistoryMode();
        return { prepended: 0, reanchored: ok };
      }
      const merged = [...toDisplayLines(items), ...historyLines.value];
      const max = budget();
      if (merged.length > max) {
        // Trim the *tail*: it sits below the viewport, so dropping it shortens
        // contentHeight without moving anything the user is looking at. Trimming
        // the head would need its own scroll compensation.
        merged.length = max;
        historyAtArchiveEnd.value = false;
      }
      historyLines.value = merged;
      historyStartLineNo.value = items[0].lineNo;
      historyAtSessionStart.value = items[0].lineNo === 1;
      return { prepended: items.length, reanchored: false };
    } catch (e) {
      fail(e);
      return none;
    } finally {
      historyLoading.value = false;
    }
  }

  /**
   * Extends the window forwards. Returns how many lines were dropped off the
   * *head* to stay within budget — the only part of the change that moves what
   * is above the viewport, so it is what the scroll compensation needs.
   */
  async function loadNewerHistory(): Promise<number> {
    if (
      !historyMode.value ||
      historyLoading.value ||
      historyAtArchiveEnd.value
    ) {
      return 0;
    }
    const lastLineNo = historyEndLineNo.value;
    if (lastLineNo === 0) return 0;
    const gen = generation;
    historyLoading.value = true;
    try {
      // `lastLineNo` as a 0-based offset is the line right after the window.
      const page = await deps.readSessionPage(lastLineNo, HISTORY_PAGE_SIZE);
      if (gen !== generation) return 0;
      historyTotalLines.value = Math.max(
        page.totalLines,
        historyTotalLines.value,
      );
      const items = page.items;
      if (items.length === 0) {
        historyAtArchiveEnd.value = true;
        return 0;
      }
      if (items[0].lineNo !== lastLineNo + 1) {
        const ok = await loadTailWindow();
        if (gen === generation && !ok) exitHistoryMode();
        return 0;
      }
      const merged = [...historyLines.value, ...toDisplayLines(items)];
      const dropped = Math.max(0, merged.length - budget());
      if (dropped > 0) {
        merged.splice(0, dropped);
        historyAtSessionStart.value = false;
      }
      historyLines.value = merged;
      historyStartLineNo.value += dropped;
      // A short page means the archive was exhausted at read time. Checking that
      // as well as the line number matters on a live port, where `totalLines`
      // keeps moving and the second test alone would never become true — leaving
      // "scroll back down to return to live" permanently out of reach.
      historyAtArchiveEnd.value =
        items.length < HISTORY_PAGE_SIZE ||
        items[items.length - 1].lineNo >= historyTotalLines.value;
      return dropped;
    } catch (e) {
      fail(e);
      return 0;
    } finally {
      historyLoading.value = false;
    }
  }

  async function jumpToSessionStart(): Promise<boolean> {
    if (!historyMode.value || historyLoading.value) return false;
    const gen = generation;
    historyLoading.value = true;
    try {
      const want = Math.min(HISTORY_PAGE_SIZE * HISTORY_ENTRY_PAGES, budget());
      const { items, totalLines } = await readRange(0, want);
      if (gen !== generation) return false;
      if (items.length === 0) return false;
      applyWindow(
        items,
        totalLines,
        items[items.length - 1].lineNo >= totalLines,
      );
      return true;
    } catch (e) {
      fail(e);
      return false;
    } finally {
      historyLoading.value = false;
    }
  }

  function exitHistoryMode(): void {
    generation += 1;
    historyMode.value = false;
    historyLines.value = [];
    historyStartLineNo.value = 0;
    historyTotalLines.value = 0;
    historyAtSessionStart.value = false;
    historyAtArchiveEnd.value = true;
    historyEntryOffsetLines.value = 0;
  }

  return {
    historyMode,
    historyLines,
    historyStartLineNo,
    historyEndLineNo,
    historyTotalLines,
    historyLoading,
    historyAtSessionStart,
    historyAtArchiveEnd,
    historyEntryOffsetLines,
    enterHistoryMode,
    loadOlderHistory,
    loadNewerHistory,
    jumpToSessionStart,
    exitHistoryMode,
  };
}
