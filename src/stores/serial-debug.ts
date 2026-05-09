import { defineStore } from 'pinia';
import { ref, watch } from 'vue';
import {
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
  FilterMode,
  HexBytesPerRow,
  SendMode,
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
  let nextLineId = 1;
  const pending = { tx: '', rx: '' } as Record<'tx' | 'rx', string>;

  // ── send ─────────────────────────────────────────────────────────────
  const sendMode = ref<SendMode>('ascii');
  const sendAppendCrlf = ref(true);
  const sendInput = ref('');
  const sendHistory = ref<string[]>([]);

  // ── filter ───────────────────────────────────────────────────────────
  const filterText = ref('');
  const filterMode = ref<FilterMode>('off');
  const filterWindowOpen = ref(false);

  // ── hex popup (for right-click "to hex/ascii" over selection) ────────
  const hexPopup = ref<{ open: boolean; bytes: Uint8Array; initialMode: 'hex' | 'ascii' }>({
    open: false,
    bytes: new Uint8Array(),
    initialMode: 'hex',
  });

  const transport = serialDebugTransport();
  let unsubscribeChunk: (() => void) | null = null;
  let unsubscribeDisconnect: (() => void) | null = null;

  // Auto-resume: when the port we wanted becomes free again while `pendingResume`
  // is set, reopen our session. This closes the auto-release round-trip triggered
  // by flash (or another owner) asking us to yield.
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
    if (filterWindowOpen.value) {
      void maybeEmitFeedLine(line);
    }
  }

  async function maybeEmitFeedLine(line: DebugLogLine): Promise<void> {
    try {
      const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow');
      const w = await WebviewWindow.getByLabel('serial-debug-filter');
      if (w) await w.emit('serial-debug-filter-feed', { line });
    } catch {
      // window closed or not Tauri — nothing to do
    }
  }

  function appendSysLine(text: string): void {
    pushLine('sys', Date.now(), text);
  }

  function decodeLossy(bytes: Uint8Array): string {
    return new TextDecoder('utf-8', { fatal: false }).decode(bytes);
  }

  function appendChunk(chunk: DebugChunk): void {
    const dir = chunk.direction;
    const rawBytes = Uint8Array.from(chunk.bytes);
    const merged = pending[dir] + decodeLossy(rawBytes);
    const parts = merged.split('\n');
    const tail = parts.pop() ?? '';
    pending[dir] = tail;
    for (const line of parts) {
      // strip a trailing \r from CRLF
      const text = line.endsWith('\r') ? line.slice(0, -1) : line;
      pushLine(dir, chunk.tsMs, text, rawBytes.slice());
    }
  }

  function clear(): void {
    lines.value = [];
    pending.tx = '';
    pending.rx = '';
    if (filterWindowOpen.value) {
      void (async () => {
        try {
          const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow');
          const w = await WebviewWindow.getByLabel('serial-debug-filter');
          if (w) await w.emit('serial-debug-filter-clear', {});
        } catch { /* ignore */ }
      })();
    }
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
    try {
      await transport.open(buildConfig());
      open.value = true;
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

  async function openFilterWindow(): Promise<'native' | 'inline'> {
    const kind = await transport.openFilterWindow();
    filterWindowOpen.value = true;
    if (kind === 'native') {
      // Wait briefly for the child window to attach its listeners, then send snapshot.
      const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow');
      const tries = 10;
      for (let i = 0; i < tries; i++) {
        const w = await WebviewWindow.getByLabel('serial-debug-filter');
        if (w) {
          await new Promise((r) => setTimeout(r, 200));
          await w.emit('serial-debug-filter-init', {
            lines: lines.value,
            filterText: filterText.value,
            filterMode: filterMode.value,
          });
          break;
        }
        await new Promise((r) => setTimeout(r, 100));
      }
    }
    return kind;
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
    sendMode.value = data.sendMode;
    sendAppendCrlf.value = data.sendAppendCrlf;
    sendHistory.value = data.sendHistory;
    filterText.value = data.filterText;
    filterMode.value = data.filterMode;
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
          sendMode: sendMode.value,
          sendAppendCrlf: sendAppendCrlf.value,
          sendHistory: sendHistory.value,
          filterText: filterText.value,
          filterMode: filterMode.value,
        });
      }, 450);
      watch(
        () => [
          port.value, baudRate.value, customBaudRate.value, dataBits.value, parity.value, stopBits.value,
          autoRelease.value, hexView.value, hexBytesPerRow.value, sendMode.value, sendAppendCrlf.value,
          [...sendHistory.value], filterText.value, filterMode.value,
        ],
        save,
        { deep: true },
      );
    });
  }

  return {
    // state
    open, opening, port, baudRate, customBaudRate, dataBits, parity, stopBits, autoRelease,
    pendingResume, lines, hexView, hexBytesPerRow, sendMode, sendAppendCrlf, sendInput,
    sendHistory, filterText, filterMode, filterWindowOpen, hexPopup,
    // actions
    openPort, closePort, send, clear, appendChunk,
    openFilterWindow, showHexPopup, closeHexPopup, appendSysLine,
    loadWorkspace, startWorkspacePersistence,
    // constants for UI
    commonBaudRates: COMMON_BAUD_RATES,
  };
});
