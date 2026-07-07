import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";
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
  VISIBLE_LOG_WINDOW_LINES,
} from "@/features/serial-debug/constants";
import { chipManifest } from "@/features/firmware-flash/chip-manifests";
import { useFlashStore } from "@/stores/flash";
import { parseHexInput } from "@/features/serial-debug/hex-format";
import { serialDebugTransport } from "@/features/serial-debug/transport";
import { wsTransport } from "@/transport/ws-transport";
import type {
  DebugChunk,
  DebugConfig,
  DebugLogLine,
  HexBytesPerRow,
  SendMode,
  SerialDebugFilterPage,
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

  // ── display ──────────────────────────────────────────────────────────
  const lines = ref<DebugLogLine[]>([]);
  const hexView = ref(false);
  const hexBytesPerRow = ref<HexBytesPerRow>(DEFAULT_HEX_BYTES_PER_ROW);
  const ansiEnabled = ref(true);
  const logFontSize = ref(12);
  const showTimestamp = ref(true);
  const showDirBadge = ref(true);
  const filterStatsById = ref<Record<string, SerialDebugFilterStats>>({});
  const filterPagesById = ref<Record<string, SerialDebugFilterPage>>({});
  const activeFilterLoading = ref(false);
  const activeFilterFullyLoaded = ref(false);

  // ── auto-save ─────────────────────────────────────────────────────────
  const autoSave = ref(false);
  const autoSaveDir = ref("");
  const autoSaveTimestamp = ref(true);
  const sessionAutoSavePath = ref<string | null>(null);
  type PendingAutoSaveLine = Pick<
    DebugLogLine,
    "direction" | "tsMs" | "text"
  > & {
    estimatedChars: number;
  };
  const pendingAutoSaveLines: PendingAutoSaveLine[] = [];

  let nextLineId = 1;
  const pending = {
    tx: createPendingBuffer(),
    rx: createPendingBuffer(),
  };
  const queuedChunks: DebugChunk[] = [];
  let chunkFlushTimer: ReturnType<typeof setTimeout> | null = null;
  let activeFilterRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  let activeFilterRefreshPending = false;

  // ── send ─────────────────────────────────────────────────────────────
  const sendMode = ref<SendMode>("ascii");
  const sendAppendCrlf = ref(true);
  const sendInput = ref("");
  const sendHistory = ref<string[]>([]);

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

  function pushLine(
    direction: DebugLogLine["direction"],
    tsMs: number,
    text: string,
    rawBytes?: Uint8Array,
  ): void {
    const line: DebugLogLine = {
      id: nextLineId++,
      direction,
      tsMs,
      text,
      rawBytes,
    };
    lines.value.push(line);
    pendingAutoSaveLines.push({
      direction: line.direction,
      tsMs: line.tsMs,
      text: line.text,
      estimatedChars: line.text.length + 48,
    });
    if (lines.value.length > VISIBLE_LOG_WINDOW_LINES) {
      lines.value.splice(0, lines.value.length - VISIBLE_LOG_WINDOW_LINES);
    }
  }

  function pushPreparedLines(batch: DebugLogLine[]): void {
    if (batch.length === 0) return;
    lines.value.push(...batch);
    pendingAutoSaveLines.push(
      ...batch.map((line) => ({
        direction: line.direction,
        tsMs: line.tsMs,
        text: line.text,
        estimatedChars: line.text.length + 48,
      })),
    );
    if (lines.value.length > VISIBLE_LOG_WINDOW_LINES) {
      lines.value.splice(0, lines.value.length - VISIBLE_LOG_WINDOW_LINES);
    }
  }

  async function appendSysLine(text: string): Promise<void> {
    const tsMs = Date.now();
    pushLine("sys", tsMs, text);
    try {
      await transport.appendSysLine(tsMs, text);
    } catch (e) {
      rLog.warn(
        `[SerialDebug] append sys line failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  }

  function decodeLossy(bytes: Uint8Array): string {
    return utf8Decoder.decode(bytes);
  }

  function prepareLinesFromChunks(
    chunks: readonly DebugChunk[],
  ): DebugLogLine[] {
    const completedLines: DebugLogLine[] = [];
    for (const chunk of chunks) {
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
          id: nextLineId++,
          direction: dir,
          tsMs: chunk.tsMs,
          text,
          rawBytes: lineBytes,
        });
      }
    }
    return completedLines;
  }

  function appendChunk(chunk: DebugChunk): void {
    const completedLines = prepareLinesFromChunks([chunk]);
    pushPreparedLines(completedLines);
  }

  async function clear(): Promise<void> {
    lines.value = [];
    resetPendingBuffer(pending.tx);
    resetPendingBuffer(pending.rx);
    queuedChunks.length = 0;
    if (chunkFlushTimer !== null) {
      clearTimeout(chunkFlushTimer);
      chunkFlushTimer = null;
    }
    pendingAutoSaveLines.length = 0;
    cancelActiveFilterRefresh();
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
    activeFilterLoading.value = false;
    activeFilterFullyLoaded.value = true;
    try {
      await transport.clearSession();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      await appendSysLine(t("serialDebug.err.sendFailed", { msg }));
    }
  }

  function drainPendingAutoSaveLines(
    maxChars = AUTO_SAVE_FLUSH_MAX_CHARS,
  ): Array<Pick<DebugLogLine, "direction" | "tsMs" | "text">> {
    if (pendingAutoSaveLines.length === 0) {
      return [];
    }
    if (maxChars <= 0) {
      return pendingAutoSaveLines.splice(0, 1);
    }
    if (!Number.isFinite(maxChars)) {
      return pendingAutoSaveLines.splice(0, pendingAutoSaveLines.length);
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
    return pendingAutoSaveLines.splice(0, count);
  }

  const activeDisplayLines = computed<DebugLogLine[]>(() => {
    if (activeChipId.value === null) return lines.value;
    return filterPagesById.value[activeChipId.value]?.items ?? [];
  });

  function flushQueuedChunks(): void {
    chunkFlushTimer = null;
    const chunks = queuedChunks.splice(0, queuedChunks.length);
    pushPreparedLines(prepareLinesFromChunks(chunks));
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
      void loadActiveFilterTail().finally(() => {
        if (activeFilterRefreshPending) {
          scheduleActiveFilterRefresh();
        }
      });
    }, FILTER_LIVE_REFRESH_MS);
  }

  async function stopBackendSession(): Promise<void> {
    unsubscribeChunk?.();
    unsubscribeDisconnect?.();
    unsubscribeFilterUpdated?.();
    unsubscribeChunk = null;
    unsubscribeDisconnect = null;
    unsubscribeFilterUpdated = null;
    queuedChunks.length = 0;
    if (chunkFlushTimer !== null) {
      clearTimeout(chunkFlushTimer);
      chunkFlushTimer = null;
    }
    cancelActiveFilterRefresh();
    try {
      await transport.close();
    } catch (e) {
      rLog.warn(
        `[SerialDebug] close error: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  }

  async function openPort(): Promise<void> {
    if (open.value || opening.value) return;
    if (!port.value.trim() || currentBaud() <= 0) {
      void appendSysLine(t("serialDebug.err.invalidConfig"));
      return;
    }
    opening.value = true;
    const pm = usePortManagerStore();
    const outcome = await pm.acquire({
      id: "serial-debug",
      port: port.value,
      onReleaseRequest: async (requester) => {
        if (autoRelease.value) return true;
        return await showConfirmDialog({
          title: t("serialDebug.confirm.releaseForFlashTitle"),
          message: t("serialDebug.confirm.releaseForFlashBody", { requester }),
          okLabel: t("serialDebug.confirm.releaseOk"),
          cancelLabel: t("serialDebug.confirm.releaseCancel"),
          kind: "warning",
        });
      },
      onReleased: (reason) => {
        opening.value = true;
        open.value = false;
        void stopBackendSession().finally(() => {
          opening.value = false;
        });
        if (reason === "requested" && autoRelease.value) {
          pendingResume.value = true;
          pm.registerResume(port.value, "serial-debug");
        }
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
      unsubscribeChunk = null;
      unsubscribeDisconnect = null;
      unsubscribeFilterUpdated = null;
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
    const effectivePort = resetPort?.trim() || port.value;
    if (!effectivePort) return;
    try {
      if (isTauriRuntime()) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("device_reset_cmd", {
          args: { port: effectivePort, chipId },
        });
      } else {
        await wsTransport.deviceReset(effectivePort, chipId);
      }
      await appendSysLine(
        t("serialDebug.log.deviceResetOk", { port: effectivePort }),
      );
      rLog.info(`[SerialDebug] Device reset (DTR/RTS) on ${effectivePort}`);
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      if (raw.includes("unknown variant") && raw.includes("device_reset")) {
        await appendSysLine(t("serialDebug.log.deviceResetServeOutdated"));
      } else {
        await appendSysLine(
          t("serialDebug.log.deviceResetFailed", { msg: raw }),
        );
      }
    }
  }

  async function send(): Promise<void> {
    if (!open.value) return;
    const raw = sendInput.value;
    if (!raw) return;
    let bytes: Uint8Array;
    if (sendMode.value === "hex") {
      const r = parseHexInput(raw);
      bytes = r.bytes;
      if (r.ignoredCount > 0) {
        await appendSysLine(
          t("serialDebug.send.hexParseIgnored", { n: r.ignoredCount }),
        );
      }
      if (bytes.length === 0) return;
    } else {
      const withTail = sendAppendCrlf.value ? raw + "\r\n" : raw;
      bytes = new TextEncoder().encode(withTail);
    }
    try {
      await transport.send(bytes);
      const idx = sendHistory.value.indexOf(raw);
      if (idx >= 0) sendHistory.value.splice(idx, 1);
      sendHistory.value.unshift(raw);
      if (sendHistory.value.length > MAX_SEND_HISTORY) {
        sendHistory.value.length = MAX_SEND_HISTORY;
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      await appendSysLine(t("serialDebug.err.sendFailed", { msg }));
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
    activeChipId.value = payload.def.id;
    activeFilterFullyLoaded.value = false;
    await loadActiveFilterTail();
    return "ok";
  }

  async function removeChip(id: string): Promise<void> {
    const idx = watchChips.value.findIndex((c) => c.id === id);
    if (idx !== -1) {
      watchChips.value.splice(idx, 1);
    }
    delete filterStatsById.value[id];
    delete filterPagesById.value[id];
    await transport.removeFilter(id);
    if (activeChipId.value === id) {
      activeChipId.value =
        watchChips.value.length > 0
          ? watchChips.value[Math.max(0, idx - 1)].id
          : null;
      if (activeChipId.value !== null) {
        await loadActiveFilterTail();
      }
    }
  }

  async function setActiveChip(id: string | null): Promise<void> {
    activeChipId.value = id;
    if (id !== null) {
      await loadActiveFilterTail();
    }
  }

  async function loadFilterPage(
    filterId: string,
    start: number,
    limit: number,
  ): Promise<SerialDebugFilterPage> {
    return await transport.readFilterMatches(filterId, start, limit);
  }

  async function loadActiveFilterTail(): Promise<void> {
    const id = activeChipId.value;
    if (!id) return;
    const stats = filterStatsById.value[id];
    if (!stats) return;
    activeFilterLoading.value = true;
    try {
      const start = Math.max(0, stats.totalMatches - FILTER_PAGE_SIZE);
      const page = await loadFilterPage(id, start, FILTER_PAGE_SIZE);
      filterPagesById.value = { ...filterPagesById.value, [id]: page };
      activeFilterFullyLoaded.value = page.start === 0;
    } finally {
      activeFilterLoading.value = false;
    }
  }

  async function loadOlderActiveFilterMatches(): Promise<void> {
    const id = activeChipId.value;
    if (!id) return;
    const existing = filterPagesById.value[id];
    if (!existing || existing.start === 0) {
      activeFilterFullyLoaded.value = true;
      return;
    }
    activeFilterLoading.value = true;
    try {
      const start = Math.max(0, existing.start - FILTER_PAGE_SIZE);
      const page = await loadFilterPage(id, start, existing.start - start);
      filterPagesById.value = {
        ...filterPagesById.value,
        [id]: {
          ...existing,
          start: page.start,
          totalMatches: page.totalMatches,
          items: [...page.items, ...existing.items],
        },
      };
      activeFilterFullyLoaded.value = start === 0;
    } finally {
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
    autoSave.value = data.autoSave ?? false;
    autoSaveDir.value = data.autoSaveDir ?? "";
    autoSaveTimestamp.value = data.autoSaveTimestamp ?? true;
    showTimestamp.value = data.showTimestamp ?? true;
    showDirBadge.value = data.showDirBadge ?? true;
    sendMode.value = data.sendMode;
    sendAppendCrlf.value = data.sendAppendCrlf;
    sendHistory.value = data.sendHistory;
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
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      autoSaveDir.value = selected;
    } else if (!autoSaveDir.value) {
      // User cancelled and no path was previously set — roll back the switch
      autoSave.value = false;
    }
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
    lines,
    activeDisplayLines,
    hexView,
    hexBytesPerRow,
    ansiEnabled,
    logFontSize,
    showTimestamp,
    showDirBadge,
    filterStatsById,
    filterPagesById,
    activeFilterLoading,
    activeFilterFullyLoaded,
    sendMode,
    sendAppendCrlf,
    sendInput,
    sendHistory,
    hexPopup,
    watchChips,
    activeChipId,
    autoSave,
    autoSaveDir,
    autoSaveTimestamp,
    sessionAutoSavePath,
    // actions
    openPort,
    closePort,
    deviceReset,
    send,
    clear,
    appendChunk,
    drainPendingAutoSaveLines,
    showHexPopup,
    closeHexPopup,
    appendSysLine,
    addChip,
    removeChip,
    setActiveChip,
    loadActiveFilterTail,
    loadOlderActiveFilterMatches,
    increaseFontSize,
    decreaseFontSize,
    loadWorkspace,
    startWorkspacePersistence,
    pickAutoSaveDir,
    // constants for UI
    commonBaudRates: COMMON_BAUD_RATES,
  };
});
