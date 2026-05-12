import { defineStore } from 'pinia';
import { ref, watch } from 'vue';
import {
  CHIP_COLORS,
  COMMON_BAUD_RATES,
  DEFAULT_BAUD_RATE,
  DEFAULT_DATA_BITS,
  DEFAULT_HEX_BYTES_PER_ROW,
  DEFAULT_PARITY,
  DEFAULT_STOP_BITS,
  MAX_LOG_LINES,
  MAX_SEND_HISTORY,
} from '@/features/serial-debug/constants';
import { parseHexInput } from '@/features/serial-debug/hex-format';
import { serialDebugTransport } from '@/features/serial-debug/transport';
import type {
  DebugChunk,
  DebugConfig,
  DebugLogLine,
  HexBytesPerRow,
  SendMode,
  WatchChip,
} from '@/features/serial-debug/types';
import { usePortManagerStore } from '@/stores/port-manager';
import { showConfirmDialog } from '@/composables/confirmDialog';
import { i18n } from '@/i18n';
import { rLog } from '@/utils/log';

export const useSerialDebugStore = defineStore('serial-debug', () => {
  const t = i18n.global.t;

  // ── runtime ──────────────────────────────────────────────────────────
  const open = ref(false);
  const opening = ref(false);
  const port = ref('');
  const baudRate = ref<number>(DEFAULT_BAUD_RATE);
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

  // ── auto-save ─────────────────────────────────────────────────────────
  const autoSave = ref(false);
  const autoSaveDir = ref('');
  const autoSaveTimestamp = ref(true);
  const sessionAutoSavePath = ref<string | null>(null);

  let nextLineId = 1;
  const pending = {
    tx: { text: '', bytes: [] as number[] },
    rx: { text: '', bytes: [] as number[] },
  };

  // ── send ─────────────────────────────────────────────────────────────
  const sendMode = ref<SendMode>('ascii');
  const sendAppendCrlf = ref(true);
  const sendInput = ref('');
  const sendHistory = ref<string[]>([]);

  // ── hex popup ────────────────────────────────────────────────────────
  const hexPopup = ref<{ open: boolean; bytes: Uint8Array; initialMode: 'hex' | 'ascii' }>({
    open: false,
    bytes: new Uint8Array(),
    initialMode: 'hex',
  });

  // ── watch chips ──────────────────────────────────────────────────────
  const watchChips = ref<WatchChip[]>([]);
  const activeChipId = ref<string | null>(null);
  const chipRegexCache = new Map<string, RegExp>();

  const transport = serialDebugTransport();
  let unsubscribeChunk: (() => void) | null = null;
  let unsubscribeDisconnect: (() => void) | null = null;

  const resumePortManager = usePortManagerStore();
  watch(
    () => resumePortManager.currentOwner(port.value),
    (current) => {
      if (current === null && pendingResume.value && !open.value && !opening.value) {
        pendingResume.value = false;
        void openPort();
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

  function pushLine(direction: DebugLogLine['direction'], tsMs: number, text: string, rawBytes?: Uint8Array): void {
    const line: DebugLogLine = { id: nextLineId++, direction, tsMs, text, rawBytes };
    lines.value.push(line);
    if (lines.value.length > MAX_LOG_LINES) {
      lines.value.splice(0, lines.value.length - MAX_LOG_LINES);
    }
  }

  function appendSysLine(text: string): void {
    pushLine('sys', Date.now(), text);
  }

  function decodeLossy(bytes: Uint8Array): string {
    return new TextDecoder('utf-8', { fatal: false }).decode(bytes);
  }

  function appendChunk(chunk: DebugChunk): void {
    const dir = chunk.direction as 'tx' | 'rx';
    const p = pending[dir];
    for (const b of chunk.bytes) p.bytes.push(b);
    const merged = p.text + decodeLossy(Uint8Array.from(chunk.bytes));
    const parts = merged.split('\n');
    const tail = parts.pop() ?? '';
    let byteOffset = 0;
    for (const part of parts) {
      const text = part.endsWith('\r') ? part.slice(0, -1) : part;
      const lineByteCount = new TextEncoder().encode(part + '\n').length;
      const lineBytes = Uint8Array.from(p.bytes.slice(byteOffset, byteOffset + lineByteCount));
      byteOffset += lineByteCount;
      pushLine(dir, chunk.tsMs, text, lineBytes);
    }
    p.text = tail;
    p.bytes = p.bytes.slice(byteOffset);
  }

  function clear(): void {
    lines.value = [];
    pending.tx = { text: '', bytes: [] };
    pending.rx = { text: '', bytes: [] };
  }

  async function stopBackendSession(): Promise<void> {
    unsubscribeChunk?.();
    unsubscribeDisconnect?.();
    unsubscribeChunk = null;
    unsubscribeDisconnect = null;
    try {
      await transport.close();
    } catch (e) {
      rLog.warn(`[SerialDebug] close error: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  async function openPort(): Promise<void> {
    if (open.value || opening.value) return;
    if (!port.value.trim() || currentBaud() <= 0) {
      appendSysLine(t('serialDebug.err.invalidConfig'));
      return;
    }
    opening.value = true;
    const pm = usePortManagerStore();
    const outcome = await pm.acquire({
      id: 'serial-debug',
      port: port.value,
      onReleaseRequest: async (requester) => {
        if (autoRelease.value) return true;
        return await showConfirmDialog({
          title: t('serialDebug.confirm.releaseForFlashTitle'),
          message: t('serialDebug.confirm.releaseForFlashBody', { requester }),
          okLabel: t('serialDebug.confirm.releaseOk'),
          cancelLabel: t('serialDebug.confirm.releaseCancel'),
          kind: 'warning',
        });
      },
      onReleased: (reason) => {
        opening.value = true;
        open.value = false;
        void stopBackendSession().finally(() => {
          opening.value = false;
        });
        if (reason === 'requested' && autoRelease.value) {
          pendingResume.value = true;
          pm.registerResume(port.value, 'serial-debug');
        }
        if (reason === 'unplugged') {
          appendSysLine(t('serialDebug.log.disconnected'));
        }
      },
    });
    if (outcome === 'denied') {
      opening.value = false;
      appendSysLine(t('serialDebug.err.portDenied'));
      return;
    }
    unsubscribeChunk = transport.onChunk(appendChunk);
    unsubscribeDisconnect = transport.onDisconnect((p) => {
      appendSysLine(t('serialDebug.log.disconnectedWith', { reason: p.reason }));
      pm.notifyUnplugged(port.value);
    });
    const cfg = buildConfig();
    try {
      await transport.open(cfg);
      open.value = true;
      appendSysLine(t('serialDebug.log.connected', { port: port.value, baud: currentBaud() }));
      rLog.info(`[SerialDebug] opened ${port.value} @ ${currentBaud()}`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      appendSysLine(t('serialDebug.err.openFailedWith', { msg }));
      pm.release(port.value, 'serial-debug');
      unsubscribeChunk?.();
      unsubscribeDisconnect?.();
      unsubscribeChunk = null;
      unsubscribeDisconnect = null;
    } finally {
      opening.value = false;
    }
  }

  async function closePort(): Promise<void> {
    if (!open.value) return;
    const pm = usePortManagerStore();
    await stopBackendSession();
    open.value = false;
    pm.release(port.value, 'serial-debug');
  }

  async function send(): Promise<void> {
    if (!open.value) return;
    const raw = sendInput.value;
    if (!raw) return;
    let bytes: Uint8Array;
    if (sendMode.value === 'hex') {
      const r = parseHexInput(raw);
      bytes = r.bytes;
      if (r.ignoredCount > 0) {
        appendSysLine(t('serialDebug.send.hexParseIgnored', { n: r.ignoredCount }));
      }
      if (bytes.length === 0) return;
    } else {
      const withTail = sendAppendCrlf.value ? raw + '\r\n' : raw;
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
      appendSysLine(t('serialDebug.err.sendFailed', { msg }));
    }
  }

  // ── chip actions ──────────────────────────────────────────────────────

  function addChip(keyword: string, useRegex: boolean): 'ok' | 'duplicate' | 'invalid-regex' {
    const trimmed = keyword.trim();
    if (!trimmed) return 'invalid-regex';
    if (watchChips.value.some((c) => c.keyword === trimmed)) return 'duplicate';
    let compiled: RegExp | undefined;
    if (useRegex) {
      try { compiled = new RegExp(trimmed); } catch { return 'invalid-regex'; }
    }
    const id = crypto.randomUUID();
    const color = CHIP_COLORS[watchChips.value.length % CHIP_COLORS.length];
    if (compiled) chipRegexCache.set(id, compiled);
    watchChips.value.push({ id, keyword: trimmed, useRegex, color });
    activeChipId.value = id;
    return 'ok';
  }

  function removeChip(id: string): void {
    const idx = watchChips.value.findIndex((c) => c.id === id);
    if (idx !== -1) {
      watchChips.value.splice(idx, 1);
      chipRegexCache.delete(id);
    }
    if (activeChipId.value === id) {
      activeChipId.value = watchChips.value.length > 0
        ? watchChips.value[Math.max(0, idx - 1)].id
        : null;
    }
  }

  function setActiveChip(id: string | null): void {
    activeChipId.value = id;
  }

  function matchChipKeyword(line: DebugLogLine, chip: WatchChip): boolean {
    if (chip.useRegex) {
      return chipRegexCache.get(chip.id)?.test(line.text) ?? false;
    }
    return line.text.includes(chip.keyword);
  }

  function increaseFontSize(): void {
    if (logFontSize.value < 18) logFontSize.value++;
  }

  function decreaseFontSize(): void {
    if (logFontSize.value > 10) logFontSize.value--;
  }

  function showHexPopup(bytes: Uint8Array, initialMode: 'hex' | 'ascii'): void {
    hexPopup.value = { open: true, bytes, initialMode };
  }

  function closeHexPopup(): void {
    hexPopup.value = { open: false, bytes: new Uint8Array(), initialMode: 'hex' };
  }

  let persistStarted = false;
  function debounce(fn: () => void, ms: number): () => void {
    let timer: ReturnType<typeof setTimeout> | null = null;
    return () => {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => { timer = null; fn(); }, ms);
    };
  }

  async function loadWorkspace(): Promise<void> {
    const { loadSerialDebugWorkspace } = await import('./serial-debug-workspace');
    const data = await loadSerialDebugWorkspace();
    if (!data) return;
    port.value = data.port;
    baudRate.value = data.baudRate;
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
    autoSaveDir.value = data.autoSaveDir ?? '';
    autoSaveTimestamp.value = data.autoSaveTimestamp ?? true;
    sendMode.value = data.sendMode;
    sendAppendCrlf.value = data.sendAppendCrlf;
    sendHistory.value = data.sendHistory;
  }

  function startWorkspacePersistence(): void {
    if (persistStarted) return;
    persistStarted = true;
    void import('./serial-debug-workspace').then(({ saveSerialDebugWorkspace, SD_WORKSPACE_VERSION }) => {
      const save = debounce(() => {
        void saveSerialDebugWorkspace({
          v: SD_WORKSPACE_VERSION,
          port: port.value,
          baudRate: baudRate.value,
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
          sendMode: sendMode.value,
          sendAppendCrlf: sendAppendCrlf.value,
          sendHistory: sendHistory.value,
        });
      }, 450);
      watch(
        () => [
          port.value, baudRate.value, customBaudRate.value, dataBits.value, parity.value, stopBits.value,
          autoRelease.value, hexView.value, hexBytesPerRow.value, ansiEnabled.value, logFontSize.value, sendMode.value, sendAppendCrlf.value,
          autoSave.value, autoSaveDir.value, autoSaveTimestamp.value,
          [...sendHistory.value],
        ],
        save,
        { deep: true },
      );
    });
  }

  async function pickAutoSaveDir(): Promise<void> {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === 'string') {
      autoSaveDir.value = selected;
    } else if (!autoSaveDir.value) {
      // User cancelled and no path was previously set — roll back the switch
      autoSave.value = false;
    }
  }

  return {
    // state
    open, opening, port, baudRate, customBaudRate, dataBits, parity, stopBits, autoRelease,
    pendingResume, lines, hexView, hexBytesPerRow, ansiEnabled, logFontSize, sendMode, sendAppendCrlf, sendInput,
    sendHistory, hexPopup, watchChips, activeChipId,
    autoSave, autoSaveDir, autoSaveTimestamp, sessionAutoSavePath,
    // actions
    openPort, closePort, send, clear, appendChunk,
    showHexPopup, closeHexPopup, appendSysLine,
    addChip, removeChip, setActiveChip, matchChipKeyword,
    increaseFontSize, decreaseFontSize,
    loadWorkspace, startWorkspacePersistence,
    pickAutoSaveDir,
    // constants for UI
    commonBaudRates: COMMON_BAUD_RATES,
  };
});
