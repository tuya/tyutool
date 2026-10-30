import { defineStore } from "pinia";
import { computed, ref, shallowRef, triggerRef, watch } from "vue";
import {
  CHIP_COLORS,
  COMMON_BAUD_RATES,
  DEFAULT_DATA_BITS,
  DEFAULT_HEX_BYTES_PER_ROW,
  DEFAULT_PARITY,
  DEFAULT_STOP_BITS,
  AUTO_SAVE_FLUSH_MAX_CHARS,
  FILTER_LIVE_REFRESH_MS,
  FILTER_PAGE_SIZE,
  MAX_PENDING_LINE_BYTES,
  MAX_SEND_HISTORY,
  MAX_VISIBLE_LOG_WINDOW_LINES,
  DEFAULT_VISIBLE_LOG_WINDOW_LINES,
  DEFAULT_ARCHIVE_LIMIT_MIB,
  AUTO_SAVE_QUEUE_MAX_CHARS,
  AUTO_SAVE_QUEUE_MAX_LINES,
} from "@/features/serial-debug/constants";
import { useSerialDebugHistory } from "@/features/serial-debug/useSerialDebugHistory";
import {
  chipManifest,
  rustPluginIdForChip,
} from "@/features/firmware-flash/chip-manifests";
import {
  AUTH_ONLY_CHIP_ID,
  normalizeChipId,
} from "@/features/firmware-flash/constants";
import { useFlashStore } from "@/stores/flash";
import { parseHexInput } from "@/features/serial-debug/hex-format";
import { archiveLineToLogLine } from "@/features/serial-debug/utils";
import {
  archiveCapNoticeText,
  chunksDroppedNoticeText,
  localizeArchiveLineText,
} from "@/features/serial-debug/archive-line-text";
import { serialDebugTransport } from "@/features/serial-debug/transport";
import { wsTransport } from "@/transport/ws-transport";
import type {
  DebugChunk,
  DebugConfig,
  DebugLogLine,
  HexBytesPerRow,
  SendMode,
  SerialDebugFilterLinePage,
  SerialDebugFilterStats,
  SerialDebugFilterUpdatePayload,
  WatchChip,
} from "@/features/serial-debug/types";
import { usePortManagerStore } from "@/stores/port-manager";
import { showConfirmDialog } from "@/composables/confirmDialog";
import { i18n } from "@/i18n";
import { isTauriRuntime } from "@/runtime";
import { rLog } from "@/utils/log";

export const useSerialDebugStore = defineStore("serial-debug", () => {
  const t = i18n.global.t;
  const utf8Decoder = new TextDecoder("utf-8", { fatal: false });

  type PendingByteBuffer = {
    chunks: Uint8Array[];
    headChunkIndex: number;
    headOffset: number;
    totalBytes: number;
  };

  function createPendingBuffer(): PendingByteBuffer {
    return {
      chunks: [],
      headChunkIndex: 0,
      headOffset: 0,
      totalBytes: 0,
    };
  }

  function resetPendingBuffer(buffer: PendingByteBuffer): void {
    buffer.chunks.length = 0;
    buffer.headChunkIndex = 0;
    buffer.headOffset = 0;
    buffer.totalBytes = 0;
  }

  function compactPendingBuffer(buffer: PendingByteBuffer): void {
    if (buffer.headChunkIndex === 0) return;
    buffer.chunks.splice(0, buffer.headChunkIndex);
    buffer.headChunkIndex = 0;
  }

  function appendPendingBytes(
    buffer: PendingByteBuffer,
    bytes: readonly number[],
  ): void {
    if (bytes.length === 0) return;
    buffer.chunks.push(Uint8Array.from(bytes));
    buffer.totalBytes += bytes.length;
  }

  function findPendingByte(buffer: PendingByteBuffer, needle: number): number {
    let pos = 0;
    for (let i = buffer.headChunkIndex; i < buffer.chunks.length; i += 1) {
      const chunk = buffer.chunks[i];
      const start = i === buffer.headChunkIndex ? buffer.headOffset : 0;
      for (let j = start; j < chunk.length; j += 1) {
        if (chunk[j] === needle) {
          return pos + (j - start);
        }
      }
      pos += chunk.length - start;
    }
    return -1;
  }

  function takePendingBytes(
    buffer: PendingByteBuffer,
    count: number,
  ): Uint8Array {
    const clampedCount = Math.min(count, buffer.totalBytes);
    const out = new Uint8Array(clampedCount);
    let written = 0;

    while (written < clampedCount) {
      const chunk = buffer.chunks[buffer.headChunkIndex];
      const available = chunk.length - buffer.headOffset;
      const take = Math.min(available, clampedCount - written);
      out.set(
        chunk.subarray(buffer.headOffset, buffer.headOffset + take),
        written,
      );
      written += take;
      buffer.headOffset += take;
      buffer.totalBytes -= take;
      if (buffer.headOffset >= chunk.length) {
        buffer.headChunkIndex += 1;
        buffer.headOffset = 0;
      }
    }

    if (buffer.headChunkIndex >= buffer.chunks.length) {
      resetPendingBuffer(buffer);
    } else {
      compactPendingBuffer(buffer);
    }

    return out;
  }

  function trimTrailingLineEnding(bytes: Uint8Array): Uint8Array {
    let end = bytes.length;
    while (end > 0 && (bytes[end - 1] === 0x0a || bytes[end - 1] === 0x0d)) {
      end -= 1;
    }
    return end === bytes.length ? bytes : bytes.subarray(0, end);
  }

  // ── runtime ──────────────────────────────────────────────────────────
  const open = ref(false);
  const opening = ref(false);
  const port = ref("");
  const flashStore = useFlashStore();
  // null = follow flash chip default; number = user's explicit choice (persisted).
  const baudRateUserOverride = ref<number | null>(null);
  const _baudRateInternal = ref<number>(
    chipManifest(flashStore.selectedChipId).defaultLogBaudRate,
  );
  const baudRate = computed({
    get: () => _baudRateInternal.value,
    set: (value: number) => {
      baudRateUserOverride.value = value;
      _baudRateInternal.value = value;
    },
  });
  const customBaudRate = ref<number | null>(null);
  const dataBits = ref(DEFAULT_DATA_BITS);
  const parity = ref(DEFAULT_PARITY);
  const stopBits = ref(DEFAULT_STOP_BITS);
  const autoRelease = ref(false);
  const pendingResume = ref(false);
  const rebootControlPort = ref("");
  const rebootChipId = ref("");

  // ── display ──────────────────────────────────────────────────────────
  // shallowRef: a deep ref would proxy every element, so the per-frame
  // splice(0, n) that keeps the window at logWindowLines would re-run the
  // Proxy traps over the whole window (~8 ms per flush at 5000 lines, ~35 ms
  // at 20000). DebugLogLine objects are never mutated after creation, so deep
  // reactivity buys nothing. In-place writers must call triggerRef(lines).
  const lines = shallowRef<DebugLogLine[]>([]);
  const pendingPreview = shallowRef<DebugLogLine[]>([]);
  const hexView = ref(false);
  const hexBytesPerRow = ref<HexBytesPerRow>(DEFAULT_HEX_BYTES_PER_ROW);
  const ansiEnabled = ref(true);
  const logFontSize = ref(12);
  const logWindowLines = ref(DEFAULT_VISIBLE_LOG_WINDOW_LINES);
  // Cap for the Rust-side session archive, in MiB. Unrelated to
  // logWindowLines: that bounds what the UI keeps in memory, this bounds what
  // is written to disk.
  const archiveLimitMib = ref(DEFAULT_ARCHIVE_LIMIT_MIB);
  const showTimestamp = ref(true);
  const showDirBadge = ref(true);
  const filterStatsById = ref<Record<string, SerialDebugFilterStats>>({});
  const filterPagesById = ref<Record<string, SerialDebugFilterLinePage>>({});
  const archiveSessionRevision = ref(0);
  const activeFilterLoading = ref(false);
  const activeFilterFullyLoaded = ref(false);
  /**
   * The user paged the active filter window backwards, so it no longer sits on
   * the newest matches. While that holds, the live refresh must not re-anchor it
   * on the tail — that would swap the content out from under someone reading it.
   * Cleared by every path that re-anchors, i.e. by `loadActiveFilterTail`.
   */
  const activeFilterPinned = ref(false);

  // ── auto-save ─────────────────────────────────────────────────────────
  const autoSave = ref(false);
  const autoSaveDir = ref("");
  const autoSaveTimestamp = ref(true);
  const sessionAutoSavePath = ref<string | null>(null);
  type PendingAutoSaveLine = Pick<
    DebugLogLine,
    "direction" | "tsMs" | "text"
  > & {
    seq: number;
    estimatedChars: number;
    /**
     * Archive lines that existed before this line's own archive write; see
     * `DebugChunk.archivedBefore`. `UNARCHIVED` means the line is not in the
     * archive at all (or its position is unknown), so the backfill can never
     * contain it and it must never be discarded.
     */
    archivedBefore: number;
  };
  const UNARCHIVED = Number.POSITIVE_INFINITY;
  /** A display line together with the archive position it was written at. */
  type PreparedLine = { line: DebugLogLine; archivedBefore: number };
  // Only filled while an auto-save session file exists (sessionAutoSavePath).
  // Without that gate nothing ever drains it when auto-save is off, so it grows
  // for the whole session (~11 MiB/min at 921600). This queue is therefore only
  // the *live* half of the auto-save file; lines from before the toggle are
  // backfilled from the Rust-side session archive by
  // `useSerialAutoSave.backfillFromArchive`, which is the full record.
  const pendingAutoSaveLines: PendingAutoSaveLine[] = [];
  let pendingAutoSaveEstimatedChars = 0;
  let autoSaveQueueOverflowed = false;
  let autoSaveQueueReady:
    ((stats: { queueCount: number; estimatedChars: number }) => void) | null =
    null;
  let autoSaveQueueOverflow: (() => void) | null = null;
  let nextAutoSaveSeq = 1;
  const filterRequestGeneration = ref(0);
  // Where the backfilled half of the current auto-save file ends: the archive
  // snapshot the backfill was capped at, or null while it is not known yet.
  // Everything at or after it is the live queue's job, everything before it has
  // already been written — that split is what makes the handoff exact.
  let autoSaveBackfilledUntil: number | null = null;
  // Sys-line archive writes still in flight. The backfill awaits them before
  // discarding, so no queued line can still be missing its position by then.
  const pendingSysArchiveWrites = new Set<Promise<void>>();
  let beforeClearAutoSave: ((throughSeq: number) => Promise<void>) | null =
    null;
  let afterBackendDrainAutoSave:
    ((throughSeq: number) => Promise<void>) | null = null;
  // The watermark is a position in *one* session file's backfill; a new (or
  // closed) file invalidates it. `flush: 'sync'` so this can never be queued
  // behind — and then clobber — a snapshot the backfill has already published.
  watch(
    sessionAutoSavePath,
    () => {
      autoSaveBackfilledUntil = null;
      if (sessionAutoSavePath.value !== null) autoSaveQueueOverflowed = false;
    },
    { flush: "sync" },
  );

  let nextLineId = 1;
  // Display id per archive line number, so reloading a filter page keeps the
  // ids of lines that are already on screen (the log renderers cache parsed
  // lines by id). Reset on session clear, because archive line numbers restart.
  let archiveLineIds = new Map<number, number>();
  // Sized for the largest set of archive-backed lines that can be on screen at
  // once, because dropping the memo costs every one of them a fresh id (renderer
  // caches invalidated, and the follow-tail watcher sees "the last line changed"
  // where nothing did). Two archive-backed buffers are each bounded by
  // `logWindowLines` — the history window and the active filter tab's page — and
  // the remaining filter tabs each keep their last loaded page.
  const MAX_ARCHIVE_LINE_IDS =
    2 * MAX_VISIBLE_LOG_WINDOW_LINES + 4 * FILTER_PAGE_SIZE;
  const pending = {
    tx: createPendingBuffer(),
    rx: createPendingBuffer(),
  };
  const pendingPreviewTsMs: Record<"tx" | "rx", number> = { tx: 0, rx: 0 };
  const queuedChunks: DebugChunk[] = [];
  let chunkFlushTimer: ReturnType<typeof setTimeout> | null = null;
  let activeFilterRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  let activeFilterRefreshPending = false;

  // ── send ─────────────────────────────────────────────────────────────
  const sendMode = ref<SendMode>("ascii");
  const sendAppendCrlf = ref(true);
  const sendInput = ref("");
  const sendHistory = ref<string[]>([]);
  const sending = ref(false);
  const sendError = ref<
    { kind: "invalidHex" } | { kind: "failed"; message: string } | null
  >(null);
  let sendDraftRevision = 0;
  watch(
    [sendInput, sendMode, sendAppendCrlf],
    () => {
      sendDraftRevision += 1;
      sendError.value = null;
    },
    { flush: "sync" },
  );

  // ── hex popup ────────────────────────────────────────────────────────
  const hexPopup = ref<{
    open: boolean;
    bytes: Uint8Array;
    initialMode: "hex" | "ascii";
  }>({
    open: false,
    bytes: new Uint8Array(),
    initialMode: "hex",
  });

  // ── watch chips ──────────────────────────────────────────────────────
  const watchChips = ref<WatchChip[]>([]);
  const activeChipId = ref<string | null>(null);

  const transport = serialDebugTransport();
  let unsubscribeChunk: (() => void) | null = null;
  let unsubscribeDisconnect: (() => void) | null = null;
  let unsubscribeFilterUpdated: (() => void) | null = null;
  let unsubscribeArchiveCapped: (() => void) | null = null;
  let unsubscribeChunksDropped: (() => void) | null = null;
  let backendStopPromise: Promise<void> | null = null;
  let resumeAfterFlashRelease = false;

  // Full-session scrollback for the "All" tab. Owns its own buffer over the Rust
  // session archive; see the composable's header for why it must never be
  // spliced into `lines`. Destructured back into local bindings so the public
  // API below stays one flat `return {}`.
  const {
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
  } = useSerialDebugHistory({
    readSessionPage: (start, limit) => transport.readSessionPage(start, limit),
    archiveLineId: (lineNo) => archiveLineId(lineNo),
    historyBudget: () => logWindowLines.value,
    reportError: (msg) => {
      void appendSysLine(t("serialDebug.log.historyLoadFailed", { msg }));
    },
  });

  const resumePortManager = usePortManagerStore();
  watch(
    () => resumePortManager.currentOwner(port.value),
    (current) => {
      if (
        current === null &&
        pendingResume.value &&
        !open.value &&
        !opening.value
      ) {
        pendingResume.value = false;
        void openPort();
      }
    },
  );

  // Follow flash chip default baud rate when no user override is set
  watch(
    () => flashStore.selectedChipId,
    (chipId) => {
      if (baudRateUserOverride.value === null) {
        _baudRateInternal.value = chipManifest(chipId).defaultLogBaudRate;
      }
    },
  );

  function currentBaud(): number {
    return customBaudRate.value ?? baudRate.value;
  }

  function buildConfig(): DebugConfig {
    return {
      port: port.value,
      baudRate: currentBaud(),
      dataBits: dataBits.value,
      parity: parity.value,
      stopBits: stopBits.value,
    };
  }

  function normalizeRebootChipId(
    chipId: string | null | undefined,
  ): string | null {
    const trimmed = chipId?.trim() ?? "";
    if (!trimmed) return null;
    const normalized = normalizeChipId(trimmed);
    return normalized === AUTH_ONLY_CHIP_ID ? null : normalized;
  }

  function rememberRebootTarget(controlPort: string, chipId: string): void {
    rebootControlPort.value = controlPort.trim();
    rebootChipId.value = normalizeChipId(chipId.trim());
  }

  function clearRebootTarget(): void {
    rebootControlPort.value = "";
    rebootChipId.value = "";
  }

  function resolveRebootTarget(availablePorts: readonly string[]): {
    controlPort: string | null;
    chipId: string | null;
    needsSelection: boolean;
  } {
    const knownPorts = new Set(
      availablePorts.map((candidate) => candidate.trim()).filter(Boolean),
    );
    const sessionControlPort = rebootControlPort.value.trim();
    const sessionChipId = normalizeRebootChipId(rebootChipId.value);
    if (sessionControlPort || rebootChipId.value.trim()) {
      if (
        sessionControlPort &&
        sessionChipId &&
        knownPorts.has(sessionControlPort)
      ) {
        return {
          controlPort: sessionControlPort,
          chipId: sessionChipId,
          needsSelection: false,
        };
      }
      return { controlPort: null, chipId: null, needsSelection: true };
    }

    const flashControlPort = flashStore.selectedSerialPort.trim();
    const flashChipId = normalizeRebootChipId(flashStore.selectedChipId);
    const resolvedPort =
      flashControlPort && knownPorts.has(flashControlPort)
        ? flashControlPort
        : null;
    return {
      controlPort: resolvedPort,
      chipId: flashChipId,
      needsSelection: !(resolvedPort && flashChipId),
    };
  }

  // Does not publish the change: the push paths below trim as their last step
  // and then trigger once, so the trim never needs its own notification.
  function trimVisibleLines(): void {
    if (lines.value.length > logWindowLines.value) {
      lines.value.splice(0, lines.value.length - logWindowLines.value);
    }
  }

  // Lowering the limit must take effect immediately, not on the next batch.
  watch(logWindowLines, () => {
    trimVisibleLines();
    triggerRef(lines);
  });

  // Same shape as settings.applyLogLevel: a setting the frontend owns, pushed
  // to Rust on load and on change. Best-effort — the archive may not be ready
  // yet (or the backend may be a version without the command), and a failed
  // push only means the compile-time default cap stays in force.
  async function applyArchiveLimit(): Promise<void> {
    try {
      await serialDebugTransport().setArchiveLimit(
        archiveLimitMib.value * 1024 * 1024,
      );
    } catch (e) {
      rLog.warn(`[serial-debug] failed to apply archive limit: ${String(e)}`);
    }
  }

  watch(archiveLimitMib, () => {
    void applyArchiveLimit();
  });

  /**
   * Queue one line for the auto-save file, unless it is already in the
   * backfilled half. Returns the queued entry so a caller that only learns the
   * archive position later (`appendSysLine`) can fill it in.
   */
  function enqueueAutoSaveLine(
    line: DebugLogLine,
    archivedBefore: number,
  ): PendingAutoSaveLine | null {
    if (sessionAutoSavePath.value === null || autoSaveQueueOverflowed)
      return null;
    // The backfill's own discard pass only sees what is queued when it runs; a
    // line archived before the snapshot can still be delivered after it (the
    // event and the snapshot read are separate round trips), so the same
    // predicate has to guard the enqueue.
    if (
      autoSaveBackfilledUntil !== null &&
      archivedBefore < autoSaveBackfilledUntil
    ) {
      return null;
    }
    const estimatedChars = line.text.length + 48;
    if (
      pendingAutoSaveLines.length >= AUTO_SAVE_QUEUE_MAX_LINES ||
      pendingAutoSaveEstimatedChars + estimatedChars > AUTO_SAVE_QUEUE_MAX_CHARS
    ) {
      autoSaveQueueOverflowed = true;
      autoSave.value = false;
      autoSaveQueueOverflow?.();
      return null;
    }
    const entry: PendingAutoSaveLine = {
      direction: line.direction,
      tsMs: line.tsMs,
      text: line.text,
      seq: nextAutoSaveSeq++,
      estimatedChars,
      archivedBefore,
    };
    pendingAutoSaveLines.push(entry);
    pendingAutoSaveEstimatedChars += estimatedChars;
    autoSaveQueueReady?.(pendingAutoSaveStats());
    return entry;
  }

  function pushLine(
    direction: DebugLogLine["direction"],
    tsMs: number,
    text: string,
    rawBytes?: Uint8Array,
    archivedBefore: number = UNARCHIVED,
  ): PendingAutoSaveLine | null {
    const line: DebugLogLine = {
      id: nextLineId++,
      direction,
      tsMs,
      text,
      rawBytes,
    };
    lines.value.push(line);
    const entry = enqueueAutoSaveLine(line, archivedBefore);
    trimVisibleLines();
    triggerRef(lines);
    return entry;
  }

  function pushPreparedLines(batch: PreparedLine[]): void {
    if (batch.length === 0) return;
    // One bulk push per frame — a per-line push would re-enter the reactive
    // array once per line.
    lines.value.push(...batch.map((prepared) => prepared.line));
    if (sessionAutoSavePath.value !== null) {
      for (const prepared of batch) {
        enqueueAutoSaveLine(prepared.line, prepared.archivedBefore);
      }
    }
    trimVisibleLines();
    triggerRef(lines);
  }

  async function appendSysLine(text: string): Promise<void> {
    const tsMs = Date.now();
    // Pushed before it is archived, so its position is not known yet — the
    // entry is patched in place below rather than queued late, which keeps the
    // file in the same order as the live view.
    const entry = pushLine("sys", tsMs, text);
    const write = (async () => {
      try {
        const lineNo = await transport.appendSysLine(tsMs, text);
        // `null` = not archived (capped archive, or web mode): leave it
        // UNARCHIVED, because a line the archive never took cannot be
        // backfilled and so must never be discarded.
        if (entry !== null && lineNo !== null)
          entry.archivedBefore = lineNo - 1;
      } catch (e) {
        rLog.warn(
          `[SerialDebug] append sys line failed: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
    })();
    pendingSysArchiveWrites.add(write);
    try {
      await write;
    } finally {
      pendingSysArchiveWrites.delete(write);
    }
  }

  function decodeLossy(bytes: Uint8Array): string {
    return utf8Decoder.decode(bytes);
  }

  function prepareLinesFromChunks(
    chunks: readonly DebugChunk[],
  ): PreparedLine[] {
    const completedLines: PreparedLine[] = [];
    for (const chunk of chunks) {
      // The archive splits lines exactly as this loop does (same `\n` rule, same
      // MAX_PENDING_LINE_BYTES), so a line completed by this chunk is a line the
      // archive completed while appending this chunk — inside
      // (archivedBefore, archivedBefore + appended].
      const archivedBefore = chunk.archivedBefore ?? UNARCHIVED;
      const dir = chunk.direction as "tx" | "rx";
      const p = pending[dir];
      appendPendingBytes(p, chunk.bytes);
      while (true) {
        let lineBytes: Uint8Array | null = null;
        const newlineIdx = findPendingByte(p, 0x0a);
        if (newlineIdx !== -1) {
          lineBytes = takePendingBytes(p, newlineIdx + 1);
        } else if (p.totalBytes >= MAX_PENDING_LINE_BYTES) {
          lineBytes = takePendingBytes(p, MAX_PENDING_LINE_BYTES);
        }
        if (!lineBytes) break;
        const text = decodeLossy(trimTrailingLineEnding(lineBytes));
        completedLines.push({
          line: {
            id: nextLineId++,
            direction: dir,
            tsMs: chunk.tsMs,
            text,
            rawBytes: lineBytes,
          },
          archivedBefore,
        });
      }
    }
    return completedLines;
  }

  function refreshPendingPreview(): void {
    pendingPreview.value = (["tx", "rx"] as const)
      .flatMap((direction) => {
        const buffer = pending[direction];
        if (buffer.totalBytes === 0) return [];
        const bytes = new Uint8Array(buffer.totalBytes);
        let offset = 0;
        for (let i = buffer.headChunkIndex; i < buffer.chunks.length; i += 1) {
          const chunk = buffer.chunks[i];
          const start = i === buffer.headChunkIndex ? buffer.headOffset : 0;
          bytes.set(chunk.subarray(start), offset);
          offset += chunk.length - start;
        }
        return [
          {
            id: nextLineId++,
            direction,
            tsMs: pendingPreviewTsMs[direction],
            text: decodeLossy(bytes),
            rawBytes: bytes,
          },
        ];
      })
      .sort((a, b) => a.tsMs - b.tsMs);
  }

  function appendChunk(chunk: DebugChunk): void {
    if (chunk.bytes.length === 0) return;
    pendingPreviewTsMs[chunk.direction as "tx" | "rx"] = chunk.tsMs;
    const completedLines = prepareLinesFromChunks([chunk]);
    pushPreparedLines(completedLines);
    refreshPendingPreview();
  }

  /**
   * Close the line currently being assembled for `dir` and show it as its own
   * line. Used at a gap in the chunk stream and when the session closes:
   * `prepareLinesFromChunks` only cuts on `\n`, so without this the bytes
   * before the gap and the bytes after it would be joined into one line the
   * device never printed, and whatever the device printed last without a
   * newline would never be shown at all.
   */
  function finalizePendingLine(dir: "tx" | "rx", archivedBefore: number): void {
    const p = pending[dir];
    if (p.totalBytes === 0) return;
    const lineBytes = takePendingBytes(p, p.totalBytes);
    pendingPreview.value = pendingPreview.value.filter(
      (line) => line.direction !== dir,
    );
    pushPreparedLines([
      {
        line: {
          id: nextLineId++,
          direction: dir,
          tsMs: Date.now(),
          text: decodeLossy(trimTrailingLineEnding(lineBytes)),
          rawBytes: lineBytes,
        },
        archivedBefore,
      },
    ]);
  }

  /**
   * Device output was dropped between two chunks. Ordered carefully: everything
   * already queued arrived *before* the gap, so it has to be turned into lines
   * first; then the open line is closed; only then does the notice go in, where
   * the loss actually happened.
   *
   * Only `rx` is finalized — the bounded backend queue only carries the serial
   * reader's Rx chunks, the Tx path writes straight through.
   */
  function handleChunksDropped(
    droppedBytes: number,
    archivedBefore: number,
  ): void {
    if (chunkFlushTimer !== null) {
      clearTimeout(chunkFlushTimer);
    }
    flushQueuedChunks();
    // Both lines come from one `append_gap` call under one archive lock, so they
    // share the position that call was made at.
    finalizePendingLine("rx", archivedBefore);
    pushLine(
      "sys",
      Date.now(),
      chunksDroppedNoticeText(droppedBytes),
      undefined,
      archivedBefore,
    );
  }

  async function clear(): Promise<void> {
    // Invalidate exports and filter reads at the start, even if auto-save or
    // the backend later refuses the clear.
    archiveSessionRevision.value += 1;
    filterRequestGeneration.value += 1;
    activeFilterLoading.value = false;
    cancelActiveFilterRefresh();
    try {
      if (chunkFlushTimer !== null) {
        clearTimeout(chunkFlushTimer);
        chunkFlushTimer = null;
      }
      flushQueuedChunks();
      finalizePendingLine("tx", UNARCHIVED);
      finalizePendingLine("rx", UNARCHIVED);
      const autoSaveBoundary = autoSaveQueueHighWaterMark();
      // Auto-save is a session concern. Let its owner finish writes for the old
      // archive before clear resets the archive line numbers and live queue.
      await beforeClearAutoSave?.(autoSaveBoundary);
      // Chunks can arrive while the old file is being flushed. Capture everything
      // already delivered at this point before resetting the line assemblers;
      // these lines belong to the next save interval, beyond autoSaveBoundary.
      if (chunkFlushTimer !== null) {
        clearTimeout(chunkFlushTimer);
        chunkFlushTimer = null;
      }
      flushQueuedChunks();
      finalizePendingLine("tx", UNARCHIVED);
      finalizePendingLine("rx", UNARCHIVED);
      dropPendingAutoSaveLinesThrough(autoSaveBoundary);
      // Archive line numbers may restart as soon as the backend clear begins.
      // Queue arrivals conservatively until the next backfill establishes its
      // boundary, even if the backend later rejects the clear.
      autoSaveBackfilledUntil = null;
      try {
        await transport.clearSession();
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        await appendSysLine(t("serialDebug.err.sendFailed", { msg }));
        return;
      }

      // Reads may have started while the backend clear was pending. Invalidate
      // those too before the local view is reset to the new archive session.
      filterRequestGeneration.value += 1;
      activeFilterLoading.value = false;
      cancelActiveFilterRefresh();

      // Chunks may have arrived while the backend clear was in flight. Finish
      // them before clearing the visible buffer so auto-save still receives them.
      if (chunkFlushTimer !== null) {
        clearTimeout(chunkFlushTimer);
        chunkFlushTimer = null;
      }
      flushQueuedChunks();
      finalizePendingLine("tx", UNARCHIVED);
      finalizePendingLine("rx", UNARCHIVED);
      dropPendingAutoSaveLinesThrough(autoSaveBoundary);

      lines.value = [];
      pendingPreview.value = [];
      // A clear starts a new archive session whose line numbers restart at 1, so
      // every anchor the history window holds points at the wrong lines.
      exitHistoryMode();
      resetPendingBuffer(pending.tx);
      resetPendingBuffer(pending.rx);
      queuedChunks.length = 0;
      autoSaveBackfilledUntil = null;
      filterStatsById.value = Object.fromEntries(
        Object.entries(filterStatsById.value).map(([id, stats]) => [
          id,
          {
            ...stats,
            status: "complete",
            scannedUntilLineNo: 0,
            totalLinesSnapshot: 0,
            totalMatches: 0,
            error: null,
          },
        ]),
      );
      filterPagesById.value = {};
      archiveLineIds = new Map();
      activeFilterLoading.value = false;
      activeFilterFullyLoaded.value = true;
      activeFilterPinned.value = false;
    } finally {
      // Also invalidates exports started while the backend clear was pending,
      // and makes exports opened during an auto-save failure stale as it exits.
      archiveSessionRevision.value += 1;
    }
  }

  function registerBeforeClearAutoSave(
    callback: ((throughSeq: number) => Promise<void>) | null,
  ): void {
    beforeClearAutoSave = callback;
  }

  function registerAutoSaveQueueReady(
    callback:
      ((stats: { queueCount: number; estimatedChars: number }) => void) | null,
  ): void {
    autoSaveQueueReady = callback;
  }

  function registerAutoSaveOverflowHandler(
    callback: (() => void) | null,
  ): void {
    autoSaveQueueOverflow = callback;
  }

  function pendingAutoSaveStats(): {
    queueCount: number;
    estimatedChars: number;
  } {
    return {
      queueCount: pendingAutoSaveLines.length,
      estimatedChars: pendingAutoSaveEstimatedChars,
    };
  }

  function removePendingAutoSavePrefix(count: number): PendingAutoSaveLine[] {
    const removed = pendingAutoSaveLines.splice(0, count);
    for (const entry of removed)
      pendingAutoSaveEstimatedChars -= entry.estimatedChars;
    return removed;
  }

  function registerAfterBackendDrainAutoSave(
    callback: ((throughSeq: number) => Promise<void>) | null,
  ): void {
    afterBackendDrainAutoSave = callback;
  }

  function peekPendingAutoSaveLines(
    maxChars = AUTO_SAVE_FLUSH_MAX_CHARS,
    throughSeq = Number.POSITIVE_INFINITY,
  ): Array<Pick<DebugLogLine, "direction" | "tsMs" | "text">> {
    let count = 0;
    let totalChars = 0;
    while (
      count < pendingAutoSaveLines.length &&
      pendingAutoSaveLines[count].seq <= throughSeq
    ) {
      const next = pendingAutoSaveLines[count];
      if (
        count > 0 &&
        Number.isFinite(maxChars) &&
        totalChars + next.estimatedChars > maxChars
      )
        break;
      totalChars += next.estimatedChars;
      count += 1;
    }
    return pendingAutoSaveLines
      .slice(0, count)
      .map(({ direction, tsMs, text }) => ({ direction, tsMs, text }));
  }

  function commitPendingAutoSaveLines(count: number): void {
    removePendingAutoSavePrefix(count);
  }

  function autoSaveQueueHighWaterMark(): number {
    return nextAutoSaveSeq - 1;
  }

  function dropPendingAutoSaveLinesThrough(throughSeq: number): void {
    let kept = 0;
    let keptChars = 0;
    for (const entry of pendingAutoSaveLines) {
      if (entry.seq > throughSeq) {
        pendingAutoSaveLines[kept++] = entry;
        keptChars += entry.estimatedChars;
      }
    }
    pendingAutoSaveLines.length = kept;
    pendingAutoSaveEstimatedChars = keptChars;
  }

  /**
   * Settle every sys-line archive write that is already in flight, so the
   * discard pass below cannot run while a queued line's position is still
   * unknown. Writes started *after* this resolves need no wait: their archive
   * append necessarily happens after the snapshot read that preceded the call,
   * so they can only land after the backfilled half.
   */
  async function settleAutoSaveMarkers(): Promise<void> {
    if (pendingSysArchiveWrites.size === 0) return;
    await Promise.all([...pendingSysArchiveWrites]);
  }

  /**
   * The backfill has written archive lines 1..`untilLineNo` to the auto-save
   * file. Drop the queued live lines that are inside that range, and remember
   * the watermark so later arrivals from before it are dropped at enqueue time.
   *
   * Exact in both directions. No duplicate: a queued line is discarded exactly
   * when its archive counterpart is <= `untilLineNo`, because a snapshot can
   * never land inside one archive write (`DebugChunk.archivedBefore`). No gap:
   * everything at or after the snapshot is kept, and lines the archive never
   * took are UNARCHIVED and therefore never discarded.
   */
  function dropBackfilledAutoSaveLines(untilLineNo: number): void {
    autoSaveBackfilledUntil = untilLineNo;
    // Compacted in place: the queue can hold a whole backfill's worth of live
    // output, and a copy would be a second allocation of it.
    let kept = 0;
    let keptChars = 0;
    for (const entry of pendingAutoSaveLines) {
      if (entry.archivedBefore >= untilLineNo) {
        pendingAutoSaveLines[kept] = entry;
        kept += 1;
        keptChars += entry.estimatedChars;
      }
    }
    pendingAutoSaveLines.length = kept;
    pendingAutoSaveEstimatedChars = keptChars;
  }

  function drainPendingAutoSaveLines(
    maxChars = AUTO_SAVE_FLUSH_MAX_CHARS,
  ): Array<Pick<DebugLogLine, "direction" | "tsMs" | "text">> {
    if (pendingAutoSaveLines.length === 0) {
      return [];
    }
    if (maxChars <= 0) {
      return removePendingAutoSavePrefix(1);
    }
    if (!Number.isFinite(maxChars)) {
      return removePendingAutoSavePrefix(pendingAutoSaveLines.length);
    }

    let count = 0;
    let totalChars = 0;
    while (count < pendingAutoSaveLines.length) {
      const next = pendingAutoSaveLines[count];
      if (count > 0 && totalChars + next.estimatedChars > maxChars) {
        break;
      }
      totalChars += next.estimatedChars;
      count += 1;
    }
    return removePendingAutoSavePrefix(count);
  }

  // A plain getter, not a computed: on the "All" tab this returns `lines.value`
  // unchanged, and Vue only notifies a computed's subscribers when the
  // computed's *result* changes — an identity-stable array would swallow every
  // triggerRef. As a getter, callers depend on `lines` / `filterPagesById` /
  // `historyMode` / `historyLines` directly and are notified.
  function activeDisplayLines(): DebugLogLine[] {
    if (activeChipId.value !== null) {
      return filterPagesById.value[activeChipId.value]?.items ?? [];
    }
    // History mode swaps the buffer rather than concatenating: live lines keep
    // flowing into `lines` (and into the auto-save queue) the whole time, they
    // are just not what is on screen.
    if (historyMode.value) return historyLines.value;
    if (!pendingPreview.value.length) return lines.value;
    return [...lines.value, ...pendingPreview.value].slice(
      -logWindowLines.value,
    );
  }

  function flushQueuedChunks(): void {
    chunkFlushTimer = null;
    const chunks = queuedChunks
      .splice(0, queuedChunks.length)
      .filter((chunk) => chunk.bytes.length > 0);
    pushPreparedLines(prepareLinesFromChunks(chunks));
    if (chunks.length > 0) {
      for (const chunk of chunks)
        pendingPreviewTsMs[chunk.direction as "tx" | "rx"] = chunk.tsMs;
      refreshPendingPreview();
    }
  }

  function queueChunk(chunk: DebugChunk): void {
    queuedChunks.push(chunk);
    if (chunkFlushTimer !== null) return;
    chunkFlushTimer = setTimeout(flushQueuedChunks, 16);
  }

  function queueChunkBatch(chunks: DebugChunk[]): void {
    if (chunks.length === 0) return;
    queuedChunks.push(...chunks);
    if (chunkFlushTimer !== null) return;
    chunkFlushTimer = setTimeout(flushQueuedChunks, 16);
  }

  function cancelActiveFilterRefresh(): void {
    if (activeFilterRefreshTimer !== null) {
      clearTimeout(activeFilterRefreshTimer);
      activeFilterRefreshTimer = null;
    }
    activeFilterRefreshPending = false;
  }

  function scheduleActiveFilterRefresh(): void {
    activeFilterRefreshPending = true;
    if (activeFilterRefreshTimer !== null) return;
    activeFilterRefreshTimer = setTimeout(() => {
      activeFilterRefreshTimer = null;
      const shouldRefresh = activeFilterRefreshPending;
      activeFilterRefreshPending = false;
      if (!shouldRefresh || activeChipId.value === null) return;
      if (activeFilterLoading.value) {
        scheduleActiveFilterRefresh();
        return;
      }
      // Paged back: drop the refresh rather than defer it. Returning to the
      // bottom re-anchors on the tail, which *is* this refresh, so nothing is
      // owed here. The match counts on the chips are updated by the caller and
      // stay live either way.
      if (activeFilterPinned.value) return;
      void loadActiveFilterTail().finally(() => {
        if (activeFilterRefreshPending) {
          scheduleActiveFilterRefresh();
        }
      });
    }, FILTER_LIVE_REFRESH_MS);
  }

  async function stopBackendSession(): Promise<void> {
    if (backendStopPromise) return backendStopPromise;
    const stopping = (async () => {
      try {
        // Keep listeners attached until the transport's close acknowledgement
        // guarantees its final batch has been delivered.
        await transport.close();
      } catch (e) {
        rLog.warn(
          `[SerialDebug] close error: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
      if (chunkFlushTimer !== null) clearTimeout(chunkFlushTimer);
      flushQueuedChunks();
      finalizePendingLine("tx", UNARCHIVED);
      finalizePendingLine("rx", UNARCHIVED);
      try {
        await afterBackendDrainAutoSave?.(autoSaveQueueHighWaterMark());
      } catch (e) {
        rLog.warn(
          `[SerialDebug] final auto-save flush failed: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
      unsubscribeChunk?.();
      unsubscribeDisconnect?.();
      unsubscribeFilterUpdated?.();
      unsubscribeArchiveCapped?.();
      unsubscribeChunksDropped?.();
      unsubscribeChunk = null;
      unsubscribeDisconnect = null;
      unsubscribeFilterUpdated = null;
      unsubscribeArchiveCapped = null;
      unsubscribeChunksDropped = null;
      cancelActiveFilterRefresh();
    })();
    backendStopPromise = stopping;
    try {
      await stopping;
    } finally {
      if (backendStopPromise === stopping) backendStopPromise = null;
    }
  }

  /** Confirm-dialog body when another feature requests a port that
   *  serial-debug currently owns. When the requester is flash, appends a hint
   *  pointing to the "auto-release for flash" toggle so the user can avoid
   *  this prompt on every future flash. */
  function portConflictMessage(requester: string): string {
    const body = t("serialDebug.confirm.releaseForFlashBody", { requester });
    if (requester === "flash") {
      return `${body}\n\n${t("serialDebug.confirm.releaseForFlashHint", {
        settings: t("serialDebug.conn.settings"),
        autoRelease: t("serialDebug.conn.autoRelease"),
      })}`;
    }
    return body;
  }

  async function openPort(): Promise<void> {
    if (open.value || opening.value) return;
    if (!port.value.trim() || currentBaud() <= 0) {
      void appendSysLine(t("serialDebug.err.invalidConfig"));
      return;
    }
    opening.value = true;
    resumeAfterFlashRelease = false;
    const pm = usePortManagerStore();
    const outcome = await pm.acquire({
      id: "serial-debug",
      port: port.value,
      onReleaseRequest: async (requester) => {
        const approved = autoRelease.value
          ? true
          : await showConfirmDialog({
              title: t("serialDebug.confirm.releaseForFlashTitle"),
              message: portConflictMessage(requester),
              okLabel: t("serialDebug.confirm.releaseOk"),
              cancelLabel: t("serialDebug.confirm.releaseCancel"),
              kind: "warning",
            });
        resumeAfterFlashRelease = approved && requester === "flash";
        return approved;
      },
      onReleased: async (reason) => {
        opening.value = true;
        const shouldResume = reason === "requested" && resumeAfterFlashRelease;
        resumeAfterFlashRelease = false;
        if (shouldResume) {
          pendingResume.value = true;
          pm.registerResume(port.value, "serial-debug");
        }
        await stopBackendSession().finally(() => {
          // Keep the auto-save session alive until close acknowledgement and
          // final batch draining have completed.
          open.value = false;
          opening.value = false;
        });
        if (reason === "unplugged") {
          void appendSysLine(t("serialDebug.log.disconnected"));
        }
      },
    });
    if (outcome === "denied") {
      opening.value = false;
      void appendSysLine(t("serialDebug.err.portDenied"));
      return;
    }
    const unsubscribeSingleChunk = transport.onChunk(queueChunk);
    const unsubscribeChunkBatch =
      "onChunkBatch" in transport &&
      typeof transport.onChunkBatch === "function"
        ? transport.onChunkBatch(queueChunkBatch)
        : () => {};
    unsubscribeChunk = () => {
      unsubscribeSingleChunk();
      unsubscribeChunkBatch();
    };
    unsubscribeDisconnect = transport.onDisconnect((p) => {
      void appendSysLine(
        t("serialDebug.log.disconnectedWith", { reason: p.reason }),
      );
      pm.notifyUnplugged(port.value);
    });
    // Live view only — deliberately not appendSysLine(): the archive already
    // holds the sentinel this line is the translation of, and writing it back
    // would record the same event twice (and be dropped anyway, the archive
    // being capped).
    unsubscribeArchiveCapped = transport.onArchiveCapped(
      ({ limitMib, archivedBefore }) => {
        pushLine(
          "sys",
          Date.now(),
          archiveCapNoticeText(limitMib),
          undefined,
          archivedBefore ?? UNARCHIVED,
        );
      },
    );
    // Live view only, same as the cap notice: the archive already holds the
    // dropped-chunk sentinel this line is the translation of.
    unsubscribeChunksDropped =
      transport.onChunksDropped?.(({ droppedBytes, archivedBefore }) => {
        handleChunksDropped(droppedBytes, archivedBefore ?? UNARCHIVED);
      }) ?? null;
    unsubscribeFilterUpdated = transport.onFilterUpdated((payload) => {
      filterStatsById.value = {
        ...filterStatsById.value,
        [payload.def.id]: payload.stats,
      };
      const isActive = activeChipId.value === payload.def.id;
      if (isActive && payload.stats.status === "complete") {
        scheduleActiveFilterRefresh();
      }
    });
    const cfg = buildConfig();
    try {
      await transport.open(cfg);
      open.value = true;
      await appendSysLine(
        t("serialDebug.log.connected", {
          port: port.value,
          baud: currentBaud(),
        }),
      );
      rLog.info(`[SerialDebug] opened ${port.value} @ ${currentBaud()}`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      await appendSysLine(t("serialDebug.err.openFailedWith", { msg }));
      pm.release(port.value, "serial-debug");
      unsubscribeChunk?.();
      unsubscribeDisconnect?.();
      unsubscribeFilterUpdated?.();
      unsubscribeArchiveCapped?.();
      unsubscribeChunksDropped?.();
      unsubscribeChunk = null;
      unsubscribeDisconnect = null;
      unsubscribeFilterUpdated = null;
      unsubscribeArchiveCapped = null;
      unsubscribeChunksDropped = null;
    } finally {
      opening.value = false;
    }
  }

  async function closePort(): Promise<void> {
    if (!open.value) return;
    const pm = usePortManagerStore();
    await stopBackendSession();
    open.value = false;
    pm.release(port.value, "serial-debug");
  }

  async function deviceReset(
    chipId: string,
    resetPort?: string,
  ): Promise<void> {
    const normalizedChipId = normalizeRebootChipId(chipId);
    const effectivePort = resetPort?.trim() || port.value;
    if (!effectivePort || !normalizedChipId) return;
    const runtimeChipId = rustPluginIdForChip(normalizedChipId);
    const useInSessionReset = effectivePort === port.value;
    if (useInSessionReset && !open.value) {
      await appendSysLine(t("serialDebug.log.deviceResetCurrentPortClosed"));
      return;
    }
    try {
      if (useInSessionReset) {
        if (isTauriRuntime()) {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("serial_debug_device_reset_cmd", {
            chipId: runtimeChipId,
          });
        } else {
          await wsTransport.serialDebugDeviceReset(runtimeChipId);
        }
      } else if (isTauriRuntime()) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("device_reset_cmd", {
          args: { port: effectivePort, chipId: runtimeChipId },
        });
      } else {
        await wsTransport.deviceReset(effectivePort, runtimeChipId);
      }
      await appendSysLine(
        t("serialDebug.log.deviceResetOk", { port: effectivePort }),
      );
      rLog.info(`[SerialDebug] Device reset (DTR/RTS) on ${effectivePort}`);
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      if (
        raw.includes("unknown variant") &&
        (raw.includes("device_reset") ||
          raw.includes("serial_debug_device_reset"))
      ) {
        await appendSysLine(t("serialDebug.log.deviceResetServeOutdated"));
      } else {
        await appendSysLine(
          t("serialDebug.log.deviceResetFailed", { msg: raw }),
        );
      }
    }
  }

  async function send(): Promise<boolean> {
    if (!open.value || sending.value) return false;
    const raw = sendInput.value;
    if (!raw) return false;
    const draftRevision = sendDraftRevision;
    sending.value = true;
    sendError.value = null;
    try {
      let bytes: Uint8Array;
      if (sendMode.value === "hex") {
        const r = parseHexInput(raw);
        bytes = r.bytes;
        if (r.ignoredCount > 0) {
          await appendSysLine(
            t("serialDebug.send.hexParseIgnored", { n: r.ignoredCount }),
          );
        }
        if (bytes.length === 0) {
          if (sendDraftRevision === draftRevision) {
            sendError.value = { kind: "invalidHex" };
          }
          return false;
        }
      } else {
        const withTail = sendAppendCrlf.value ? raw + "\r\n" : raw;
        bytes = new TextEncoder().encode(withTail);
      }
      await transport.send(bytes);
      const idx = sendHistory.value.indexOf(raw);
      if (idx >= 0) sendHistory.value.splice(idx, 1);
      sendHistory.value.unshift(raw);
      if (sendHistory.value.length > MAX_SEND_HISTORY) {
        sendHistory.value.length = MAX_SEND_HISTORY;
      }
      if (sendDraftRevision === draftRevision) sendInput.value = "";
      return true;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (sendDraftRevision === draftRevision)
        sendError.value = { kind: "failed", message: msg };
      await appendSysLine(t("serialDebug.err.sendFailed", { msg }));
      return false;
    } finally {
      sending.value = false;
    }
  }

  // ── chip actions ──────────────────────────────────────────────────────

  async function addChip(
    keyword: string,
    useRegex: boolean,
  ): Promise<"ok" | "duplicate" | "invalid-regex"> {
    const trimmed = keyword.trim();
    if (!trimmed) return "invalid-regex";
    if (
      watchChips.value.some(
        (c) => c.keyword === trimmed && c.useRegex === useRegex,
      )
    ) {
      return "duplicate";
    }
    if (useRegex) {
      try {
        new RegExp(trimmed);
      } catch {
        return "invalid-regex";
      }
    }
    const color = CHIP_COLORS[watchChips.value.length % CHIP_COLORS.length];
    let payload: SerialDebugFilterUpdatePayload;
    try {
      payload = await transport.addFilter(trimmed, useRegex, color);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes("duplicate")) return "duplicate";
      if (msg.includes("regex")) return "invalid-regex";
      throw e;
    }
    watchChips.value.push(payload.def);
    filterStatsById.value = {
      ...filterStatsById.value,
      [payload.def.id]: payload.stats,
    };
    filterRequestGeneration.value += 1;
    activeChipId.value = payload.def.id;
    activeFilterFullyLoaded.value = false;
    await loadActiveFilterTail();
    return "ok";
  }

  async function removeChip(id: string): Promise<void> {
    const wasActive = activeChipId.value === id;
    if (wasActive) filterRequestGeneration.value += 1;
    const idx = watchChips.value.findIndex((c) => c.id === id);
    if (idx !== -1) {
      watchChips.value.splice(idx, 1);
    }
    delete filterStatsById.value[id];
    delete filterPagesById.value[id];
    if (wasActive) {
      activeChipId.value =
        watchChips.value.length > 0
          ? watchChips.value[Math.max(0, idx - 1)].id
          : null;
      activeFilterLoading.value = false;
    }
    await transport.removeFilter(id);
    if (wasActive && activeChipId.value !== null) {
      await loadActiveFilterTail();
    }
  }

  async function setActiveChip(id: string | null): Promise<void> {
    filterRequestGeneration.value += 1;
    activeChipId.value = id;
    activeFilterLoading.value = false;
    if (id !== null) {
      await loadActiveFilterTail();
    }
  }

  function archiveLineId(lineNo: number): number {
    const existing = archiveLineIds.get(lineNo);
    if (existing !== undefined) return existing;
    // Drop the memo rather than let it grow with the session. Lines then get
    // fresh ids (a one-off re-parse in the renderers); ids stay unique.
    if (archiveLineIds.size >= MAX_ARCHIVE_LINE_IDS) archiveLineIds.clear();
    const id = nextLineId++;
    archiveLineIds.set(lineNo, id);
    return id;
  }

  async function loadFilterPage(
    filterId: string,
    start: number,
    limit: number,
  ): Promise<SerialDebugFilterLinePage> {
    const page = await transport.readFilterMatches(filterId, start, limit);
    return {
      filterId: page.filterId,
      totalMatches: page.totalMatches,
      start: page.start,
      // A filter tab renders straight out of the archive, so this is the third
      // place the cap sentinel could surface — same helper as the export and
      // the auto-save file.
      items: page.items.map((line) =>
        archiveLineToLogLine(
          {
            ...line,
            text: localizeArchiveLineText(line.direction, line.text),
          },
          archiveLineId(line.lineNo),
        ),
      ),
    };
  }

  async function loadActiveFilterTail(): Promise<void> {
    const id = activeChipId.value;
    if (!id) return;
    const stats = filterStatsById.value[id];
    if (!stats) return;
    const generation = ++filterRequestGeneration.value;
    // This is the one operation that puts the window back on the newest
    // matches, so it is where the pin comes off — every caller (tab switch,
    // chip add/remove, live refresh, scrolled back to the bottom) means it.
    activeFilterPinned.value = false;
    activeFilterLoading.value = true;
    try {
      const start = Math.max(0, stats.totalMatches - FILTER_PAGE_SIZE);
      const page = await loadFilterPage(id, start, FILTER_PAGE_SIZE);
      if (
        generation !== filterRequestGeneration.value ||
        activeChipId.value !== id
      )
        return;
      filterPagesById.value = { ...filterPagesById.value, [id]: page };
      activeFilterFullyLoaded.value = page.start === 0;
    } finally {
      if (
        generation === filterRequestGeneration.value &&
        activeChipId.value === id
      )
        activeFilterLoading.value = false;
    }
  }

  /** @returns how many matches were prepended, for the caller's scroll compensation. */
  async function loadOlderActiveFilterMatches(): Promise<number> {
    const id = activeChipId.value;
    if (!id) return 0;
    const existing = filterPagesById.value[id];
    if (!existing || existing.start === 0) {
      activeFilterFullyLoaded.value = true;
      return 0;
    }
    const generation = ++filterRequestGeneration.value;
    activeFilterLoading.value = true;
    try {
      const start = Math.max(0, existing.start - FILTER_PAGE_SIZE);
      const page = await loadFilterPage(id, start, existing.start - start);
      if (
        generation !== filterRequestGeneration.value ||
        activeChipId.value !== id
      )
        return 0;
      const items = [...page.items, ...existing.items];
      // Bounded like the live buffer and the history window: without this,
      // holding the button down pulls every match of the whole session into
      // memory. Trimmed from the tail, which sits below the viewport, so the
      // scroll position is unaffected; `loadActiveFilterTail` (tab switch, live
      // refresh) puts the newest matches back.
      const max = Math.max(logWindowLines.value, FILTER_PAGE_SIZE);
      if (items.length > max) items.length = max;
      filterPagesById.value = {
        ...filterPagesById.value,
        [id]: {
          ...existing,
          start: page.start,
          totalMatches: page.totalMatches,
          items,
        },
      };
      activeFilterFullyLoaded.value = start === 0;
      activeFilterPinned.value = true;
      return page.items.length;
    } finally {
      if (
        generation === filterRequestGeneration.value &&
        activeChipId.value === id
      )
        activeFilterLoading.value = false;
    }
  }

  function increaseFontSize(): void {
    if (logFontSize.value < 18) logFontSize.value++;
  }

  function decreaseFontSize(): void {
    if (logFontSize.value > 10) logFontSize.value--;
  }

  function showHexPopup(bytes: Uint8Array, initialMode: "hex" | "ascii"): void {
    hexPopup.value = { open: true, bytes, initialMode };
  }

  function closeHexPopup(): void {
    hexPopup.value = {
      open: false,
      bytes: new Uint8Array(),
      initialMode: "hex",
    };
  }

  let persistStarted = false;
  function debounce(fn: () => void, ms: number): () => void {
    let timer: ReturnType<typeof setTimeout> | null = null;
    return () => {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        fn();
      }, ms);
    };
  }

  async function loadWorkspace(): Promise<void> {
    const { loadSerialDebugWorkspace } =
      await import("./serial-debug-workspace");
    const data = await loadSerialDebugWorkspace();
    if (!data) return;
    port.value = data.port;
    if (data.baudRate === null) {
      baudRateUserOverride.value = null;
      _baudRateInternal.value = chipManifest(
        flashStore.selectedChipId,
      ).defaultLogBaudRate;
    } else {
      baudRateUserOverride.value = data.baudRate;
      _baudRateInternal.value = data.baudRate;
    }
    customBaudRate.value = data.customBaudRate;
    dataBits.value = data.dataBits;
    parity.value = data.parity;
    stopBits.value = data.stopBits;
    autoRelease.value = data.autoRelease;
    hexView.value = data.hexView;
    hexBytesPerRow.value = data.hexBytesPerRow;
    ansiEnabled.value = data.ansiEnabled ?? true;
    logFontSize.value = data.logFontSize ?? 12;
    logWindowLines.value =
      data.logWindowLines ?? DEFAULT_VISIBLE_LOG_WINDOW_LINES;
    archiveLimitMib.value = data.archiveLimitMib ?? DEFAULT_ARCHIVE_LIMIT_MIB;
    autoSave.value = data.autoSave ?? false;
    autoSaveDir.value = data.autoSaveDir ?? "";
    // Re-authorize the restored auto-save directory so writes work after a
    // workspace restore (the registry is in-memory and does not persist).
    if (autoSaveDir.value && isTauriRuntime()) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("register_dialog_path", { path: autoSaveDir.value });
      } catch {
        // Non-fatal: writes will be rejected and surfaced via autoSave.errWrite.
      }
    }
    autoSaveTimestamp.value = data.autoSaveTimestamp ?? true;
    showTimestamp.value = data.showTimestamp ?? true;
    showDirBadge.value = data.showDirBadge ?? true;
    sendMode.value = data.sendMode;
    sendAppendCrlf.value = data.sendAppendCrlf;
    sendHistory.value = data.sendHistory;
    // The archive is created at app start with the compile-time default, so the
    // restored value has to be pushed down once the workspace is known. The
    // watch above only fires when the value actually changes.
    await applyArchiveLimit();
  }

  function startWorkspacePersistence(): void {
    if (persistStarted) return;
    persistStarted = true;
    void import("./serial-debug-workspace").then(
      ({ saveSerialDebugWorkspace, SD_WORKSPACE_VERSION }) => {
        const save = debounce(() => {
          void saveSerialDebugWorkspace({
            v: SD_WORKSPACE_VERSION,
            port: port.value,
            baudRate: baudRateUserOverride.value,
            customBaudRate: customBaudRate.value,
            dataBits: dataBits.value,
            parity: parity.value,
            stopBits: stopBits.value,
            autoRelease: autoRelease.value,
            hexView: hexView.value,
            hexBytesPerRow: hexBytesPerRow.value,
            ansiEnabled: ansiEnabled.value,
            logFontSize: logFontSize.value,
            logWindowLines: logWindowLines.value,
            archiveLimitMib: archiveLimitMib.value,
            autoSave: autoSave.value,
            autoSaveDir: autoSaveDir.value,
            autoSaveTimestamp: autoSaveTimestamp.value,
            showTimestamp: showTimestamp.value,
            showDirBadge: showDirBadge.value,
            sendMode: sendMode.value,
            sendAppendCrlf: sendAppendCrlf.value,
            sendHistory: sendHistory.value,
          });
        }, 450);
        watch(
          () => [
            port.value,
            baudRate.value,
            customBaudRate.value,
            dataBits.value,
            parity.value,
            stopBits.value,
            autoRelease.value,
            hexView.value,
            hexBytesPerRow.value,
            ansiEnabled.value,
            logFontSize.value,
            logWindowLines.value,
            archiveLimitMib.value,
            sendMode.value,
            sendAppendCrlf.value,
            autoSave.value,
            autoSaveDir.value,
            autoSaveTimestamp.value,
            showTimestamp.value,
            showDirBadge.value,
            [...sendHistory.value],
          ],
          save,
          { deep: true },
        );
      },
    );
  }

  async function pickAutoSaveDir(): Promise<void> {
    if (!isTauriRuntime()) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const { invoke } = await import("@tauri-apps/api/core");
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      // Authorize this dialog-chosen directory (and its descendants) for the
      // auto-save file writes. The backend write commands refuse unregistered
      // paths, so a compromised renderer cannot write arbitrary files.
      await invoke("register_dialog_path", { path: selected });
      autoSaveDir.value = selected;
    } else if (!autoSaveDir.value) {
      // User cancelled and no path was previously set — roll back the switch
      autoSave.value = false;
    }
  }

  /**
   * Toggle auto-save from any surface (settings switch, log toolbar). Enabling
   * without a directory would leave the switch on but inert — the watcher in
   * `useSerialAutoSave` needs both — so ask for one instead.
   */
  async function setAutoSaveEnabled(next: boolean): Promise<void> {
    if (next === autoSave.value) return;
    if (next && !autoSaveDir.value) {
      autoSave.value = true;
      await pickAutoSaveDir();
      return;
    }
    autoSave.value = next;
  }

  return {
    // state
    open,
    opening,
    port,
    baudRate,
    customBaudRate,
    dataBits,
    parity,
    stopBits,
    autoRelease,
    pendingResume,
    rebootControlPort,
    rebootChipId,
    lines,
    hexView,
    hexBytesPerRow,
    ansiEnabled,
    logFontSize,
    logWindowLines,
    archiveLimitMib,
    showTimestamp,
    showDirBadge,
    filterStatsById,
    filterPagesById,
    archiveSessionRevision,
    activeFilterLoading,
    activeFilterFullyLoaded,
    activeFilterPinned,
    historyMode,
    historyLines,
    historyStartLineNo,
    historyEndLineNo,
    historyTotalLines,
    historyLoading,
    historyAtSessionStart,
    historyAtArchiveEnd,
    historyEntryOffsetLines,
    sendMode,
    sendAppendCrlf,
    sendInput,
    sendHistory,
    sending,
    sendError,
    hexPopup,
    watchChips,
    activeChipId,
    autoSave,
    autoSaveDir,
    autoSaveTimestamp,
    sessionAutoSavePath,
    // actions
    activeDisplayLines,
    openPort,
    closePort,
    deviceReset,
    rememberRebootTarget,
    clearRebootTarget,
    resolveRebootTarget,
    send,
    clear,
    appendChunk,
    drainPendingAutoSaveLines,
    peekPendingAutoSaveLines,
    pendingAutoSaveStats,
    commitPendingAutoSaveLines,
    autoSaveQueueHighWaterMark,
    registerBeforeClearAutoSave,
    registerAfterBackendDrainAutoSave,
    registerAutoSaveQueueReady,
    registerAutoSaveOverflowHandler,
    settleAutoSaveMarkers,
    dropBackfilledAutoSaveLines,
    showHexPopup,
    closeHexPopup,
    appendSysLine,
    addChip,
    removeChip,
    setActiveChip,
    loadActiveFilterTail,
    loadOlderActiveFilterMatches,
    enterHistoryMode,
    loadOlderHistory,
    loadNewerHistory,
    jumpToSessionStart,
    exitHistoryMode,
    increaseFontSize,
    decreaseFontSize,
    loadWorkspace,
    startWorkspacePersistence,
    pickAutoSaveDir,
    setAutoSaveEnabled,
    // constants for UI
    commonBaudRates: COMMON_BAUD_RATES,
  };
});
