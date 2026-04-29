import { computed, nextTick, ref, watch } from 'vue';
import { i18n } from '@/i18n';
import {
  AUTH_BAUD_RATE_DEFAULT,
  BAUD_RATE_OPTIONS,
  CHIP_IDS,
  DEFAULT_CHIP_ID,
  SERIAL_PORT_OPTIONS,
} from '@/features/firmware-flash/constants';
import { chipManifest, rustPluginIdForChip } from '@/features/firmware-flash/chip-manifests';
import { type FlashJobPayload, type FlashProgressPayload, isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import {
  alignedExclusiveEraseRange4K,
  exclusiveEraseRangeNeeds4KAlignment,
  formatAddrHex,
  formatBigIntAddrHex,
  parseHexAddr,
  validateAddrRange,
} from '@/features/firmware-flash/hex';
import {
  formatSerialPortLabel,
  tuyaDualSerialHoverTooltip,
  type SerialPortDropdownOption,
  type TauriSerialPortRow,
} from '@/features/firmware-flash/serial-port-label';
import { addTimestampSuffix, formatDuration } from '@/features/firmware-flash/utils';
import type { ErasePresetKind, FlashPhase, FlashSegment, OpKind } from '@/features/firmware-flash/types';
import { showConfirmDialog } from '@/composables/confirmDialog';
import { APP_VERSION } from '@/config/app';
import { rLog } from '@/utils/log';
import { defineStore } from 'pinia';
import { wsTransport } from '@/features/firmware-flash/ws-transport';
import type { WsProgressEvent } from '@/features/firmware-flash/ws-transport';
import {
  loadFlashWorkspaceFromStorage,
  saveFlashWorkspaceToStorage,
  WORKSPACE_VERSION,
  type FlashWorkspaceSerialized,
} from '@/stores/flash-workspace';

/** Factory-unauthorized placeholder from TuyaOpen firmware (matches `authorize.rs`). */
const AUTHORIZE_PLACEHOLDER_UUID = 'uuidxxxxxxxxxxxxxxxx';

function addrRangeMessage(err: NonNullable<ReturnType<typeof validateAddrRange>>): string {
  const t = i18n.global.t;
  if (err === 'invalid') {
    return t('flash.err.addrInvalid');
  }
  return t('flash.err.startAfterEnd');
}

function mapBackendUserMessage(raw: string | undefined): string {
  return raw?.trim() ?? '';
}

function createDebounced(fn: () => void, ms: number): () => void {
  let t: ReturnType<typeof setTimeout> | null = null;
  return () => {
    if (t !== null) {
      clearTimeout(t);
    }
    t = setTimeout(() => {
      t = null;
      fn();
    }, ms);
  };
}

export const useFlashStore = defineStore('flash', () => {
  const t = i18n.global.t;
  const locale = i18n.global.locale;

  /** When true, `selectedChipId` watch skips side effects (used during workspace restore). */
  const workspaceRestoreMuted = ref(false);
  let workspacePersistStarted = false;

  const activeTab = ref<OpKind>('flash');
  const connected = ref(false);
  const selectedSerialPort = ref('');
  const selectedBaudRate = ref<number>(chipManifest(DEFAULT_CHIP_ID).defaultBaudRate);
  /** Baud rate for TuyaOpen UART authorization — independent of flash/erase/read baud. */
  const selectedAuthBaudRate = ref<number>(AUTH_BAUD_RATE_DEFAULT);
  const selectedChipId = ref<string>(DEFAULT_CHIP_ID);

  const flashSegments = ref<FlashSegment[]>([
    {
      id: Math.random().toString(36).substring(2, 9),
      firmwarePath: '',
      firmwareFile: null,
      startAddr: '0x00000000',
      endAddr: '0x00000000',
    },
  ]);
  const activeSegmentIndex = ref(0);

  const fileInputRef = ref<HTMLInputElement | null>(null);
  const eraseAdvancedOpen = ref(false);

  const flashStartAddr = computed({
    get: () => flashSegments.value[0].startAddr,
    set: val => {
      flashSegments.value[0].startAddr = val;
    },
  });
  const flashEndAddr = computed({
    get: () => flashSegments.value[0].endAddr,
    set: val => {
      flashSegments.value[0].endAddr = val;
    },
  });
  const firmwarePath = computed({
    get: () => flashSegments.value[0].firmwarePath,
    set: val => {
      flashSegments.value[0].firmwarePath = val;
    },
  });
  const firmwareFile = computed({
    get: () => flashSegments.value[0].firmwareFile,
    set: val => {
      flashSegments.value[0].firmwareFile = val;
    },
  });

  const eraseStartAddr = ref('0x00000000');
  const eraseEndAddr = ref('0x00000000');
  const readStartAddr = ref('0x00000000');
  const readEndAddr = ref(chipManifest(DEFAULT_CHIP_ID).flashSize);
  const readDir = ref('');
  const readFileName = ref(`tyutool_read_${selectedChipId.value.toLowerCase()}.bin`);
  const readFileNameModified = ref(false);
  const authorizeUuid = ref('');
  const authorizeAuthKey = ref('');
  /** True while a read-only auth operation started by startAuthRead() is in flight. */
  const authOpIsRead = ref(false);

  const autoConnected = ref(false);

  const flashProgress = ref(0);
  const flashPhase = ref<FlashPhase>('idle');
  const flashMessage = ref('');
  const runningOp = ref<OpKind | null>(null);

  let progressTimer: ReturnType<typeof setInterval> | null = null;
  let unlistenFlash: (() => void) | undefined;
  let operationStartTime: number | null = null;

  const serialPortOptions = ref<SerialPortDropdownOption[]>(SERIAL_PORT_OPTIONS.map(p => ({ ...p })));

  const logLines = ref<string[]>([
    `[${t('flash.log.readyTag')}] ${t('flash.log.appInfo', { version: APP_VERSION })} — ${t('flash.log.waiting')}`,
  ]);
  const logScrollRef = ref<HTMLDivElement | null>(null);
  const lockAutoScroll = ref(false);

  const selectedChipLabel = computed(() => {
    const id = selectedChipId.value;
    return t(`flash.chips.${id}`);
  });

  function appendLog(line: string): void {
    const ts = new Date().toLocaleTimeString([], { hour12: false });
    logLines.value.push(`[${ts}] ${line}`);
    if (logLines.value.length > 500) {
      logLines.value.splice(0, logLines.value.length - 500);
    }
  }

  function logOperationDuration(): void {
    if (operationStartTime !== null) {
      const elapsed = Date.now() - operationStartTime;
      appendLog(t('flash.log.operationDuration', { duration: formatDuration(elapsed) }));
      operationStartTime = null;
    }
  }

  async function scrollLogToBottom(): Promise<void> {
    await nextTick();
    const el = logScrollRef.value;
    if (!el || lockAutoScroll.value) {
      return;
    }
    el.scrollTop = el.scrollHeight;
  }

  watch(
    logLines,
    () => {
      void scrollLogToBottom();
    },
    { deep: true }
  );

  // Auto-update readFileName, readEndAddr and baudRate when chip changes
  watch(selectedChipId, newChipId => {
    if (workspaceRestoreMuted.value) {
      return;
    }
    if (!readFileNameModified.value) {
      readFileName.value = `tyutool_read_${newChipId.toLowerCase()}.bin`;
    }
    const manifest = chipManifest(newChipId);
    readEndAddr.value = manifest.flashSize;
    selectedBaudRate.value = manifest.defaultBaudRate;
    const chipLabel = t(`flash.chips.${newChipId}`);
    appendLog(t('flash.log.chipChanged', { chip: chipLabel }));
    rLog.info(`[Flash] Chip changed to ${newChipId} (${chipLabel}), baud=${manifest.defaultBaudRate}`);
  });

  function onReadFileNameInput(value: string): void {
    readFileName.value = value;
    readFileNameModified.value = true;
  }

  const readFilePath = computed(() => {
    const dir = readDir.value.trim();
    const name = readFileName.value.trim();
    if (!dir || !name) return '';
    // Normalize: ensure exactly one separator between dir and name
    const sep = dir.endsWith('/') || dir.endsWith('\\') ? '' : '/';
    return `${dir}${sep}${name}`;
  });

  function clearLogs(): void {
    logLines.value = [];
    appendLog(t('flash.log.cleared'));
  }

  async function copyLogs(): Promise<void> {
    const text = logLines.value.join('\n');
    try {
      await navigator.clipboard.writeText(text);
      appendLog(t('flash.log.copied'));
    } catch {
      appendLog(t('flash.log.copyFailed'));
    }
  }

  function updateFlashEndAddr(index: number, fileSize: number): void {
    const seg = flashSegments.value[index];
    const startVal = parseHexAddr(seg.startAddr);
    const start = startVal !== null ? Number(startVal) : 0;
    const end = start + fileSize;
    seg.endAddr = `0x${end.toString(16).toUpperCase().padStart(8, '0')}`;
  }

  /** Default start/end for a new row: chain from previous segment's end (same value until a file sets the end). */
  function initialAddrsForNewSegment(): { startAddr: string; endAddr: string } {
    const prev = flashSegments.value[flashSegments.value.length - 1];
    const parsed = parseHexAddr(prev.endAddr);
    if (parsed === null) {
      return { startAddr: '0x00000000', endAddr: '0x00000000' };
    }
    const normalized = `0x${parsed.toString(16).toUpperCase().padStart(8, '0')}`;
    return { startAddr: normalized, endAddr: normalized };
  }

  async function onPickFile(index = 0): Promise<void> {
    activeSegmentIndex.value = index;
    if (isTauriRuntime()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({
          multiple: false,
          filters: [
            {
              name: 'Firmware',
              extensions: ['bin', 'hex', 'elf', 'img'],
            },
          ],
        });
        if (selected !== null) {
          flashSegments.value[index].firmwarePath = selected;
          try {
            const { invoke } = await import('@tauri-apps/api/core');
            const size = await invoke<number>('get_file_size', { path: selected });
            updateFlashEndAddr(index, size);
          } catch {
            /* get_file_size not available, skip auto-calc */
          }
        }
      } catch {
        /* ignore */
      }
      return;
    }
    fileInputRef.value?.click();
  }

  function onFileChange(e: Event): void {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    const index = activeSegmentIndex.value;
    const seg = flashSegments.value[index];
    seg.firmwarePath = file ? file.name : '';
    seg.firmwareFile = file ?? null;
    if (file) {
      updateFlashEndAddr(index, file.size);
    }
    input.value = '';
  }

  function addSegment(): void {
    if (flashSegments.value.length >= 10) return;
    const addrs = initialAddrsForNewSegment();
    flashSegments.value.push({
      id: Math.random().toString(36).substring(2, 9),
      firmwarePath: '',
      firmwareFile: null,
      startAddr: addrs.startAddr,
      endAddr: addrs.endAddr,
    });
    appendLog(t('flash.log.segmentAdded', { n: flashSegments.value.length }));
  }

  function removeSegment(index: number): void {
    if (index === 0 || flashSegments.value.length <= 1) return;
    flashSegments.value.splice(index, 1);
    appendLog(t('flash.log.segmentRemoved', { n: index + 1 }));
  }

  async function onPickReadDir(): Promise<void> {
    if (isTauriRuntime()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({
          directory: true,
          multiple: false,
        });
        if (selected !== null) {
          readDir.value = selected;
        }
      } catch {
        /* ignore */
      }
      return;
    }
    // In web mode, read output is downloaded automatically via browser download trigger.
    // No directory selection is needed; inform the user.
    appendLog(t('flash.log.browserReadNoDir'));
  }

  async function cancelBackendFlash(): Promise<void> {
    if (isTauriRuntime()) {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('flash_cancel');
      } catch {
        /* ignore */
      }
    } else {
      wsTransport.cancelJob();
    }
  }

  function stopFlash(): void {
    if (progressTimer) {
      clearInterval(progressTimer);
      progressTimer = null;
    }
    void cancelBackendFlash();
  }

  function handleFlashProgressPayload(p: FlashProgressPayload): void {
    if (p.kind === 'percent') {
      flashProgress.value = Math.min(100, Math.max(0, p.value));
      return;
    }
    if (p.kind === 'log_line') {
      appendLog(p.line);
      return;
    }
    if (p.kind === 'log_key') {
      if (p.key === 'flash.log.auth.readResult') {
        const uuid = p.params?.uuid ?? '';
        const authkey = p.params?.authkey ?? '';
        const copyText = `UUID:${uuid}\nAuthKey:${authkey}`;
        // Show auth values in a modal after successful read; do not echo secrets into the log.
        void showConfirmDialog({
          title: t('flash.confirm.authReadTitle'),
          message: t('flash.confirm.authReadBody', { uuid, authkey }),
          kind: 'info',
          extraActionLabel: t('flash.confirm.authReadCopyCmd'),
          onExtraAction: async () => {
            try {
              await navigator.clipboard.writeText(copyText);
              appendLog(t('flash.log.authReadCopied'));
            } catch {
              appendLog(t('flash.log.copyFailed'));
            }
          },
          okLabel: t('flash.confirm.authReadOk'),
          showCancel: false,
        });
        appendLog(t('flash.log.authReadShown'));
        return;
      }
      appendLog(t(p.key, p.params ?? {}));
      return;
    }
    if (p.kind === 'phase') {
      const phaseKey = `flash.log.phase.${p.name}`;
      const msg = i18n.global.te(phaseKey) ? t(phaseKey) : `[${p.name}]`;
      appendLog(msg);
      return;
    }
    if (p.kind === 'done') {
      const op = runningOp.value;
      const doneMsg = p.message?.trim() ?? '';
      runningOp.value = null;
      authOpIsRead.value = false;
      if (p.ok === true) {
        flashPhase.value = 'success';
        flashProgress.value = 100;
        if (op === 'flash') {
          flashMessage.value = t('flash.msg.flashDone');
          appendLog(t('flash.log.verifyOk'));
        } else if (op === 'erase') {
          flashMessage.value = t('flash.msg.eraseDone');
          appendLog(t('flash.log.eraseDoneLog'));
        } else if (op === 'read') {
          flashMessage.value = t('flash.msg.readDone');
          appendLog(t('flash.log.readDoneLog'));
        } else if (op === 'authorize') {
          flashMessage.value = t('flash.msg.authDone');
          appendLog(t('flash.log.authOkLog'));
        }
        rLog.info(`[Flash] Operation '${op}' completed successfully`);
      } else {
        flashPhase.value = 'error';
        const raw = doneMsg;
        const displayMsg = raw ? mapBackendUserMessage(raw) : t('flash.err.withMsg', { msg: 'unknown' });
        flashMessage.value = displayMsg;
        appendLog(t('flash.err.withMsg', { msg: displayMsg }));
        rLog.error(`[Flash] Operation '${op}' failed: ${flashMessage.value}`);
      }
      logOperationDuration();
      // Auto-disconnect if this operation was auto-connected
      if (autoConnected.value) {
        autoConnected.value = false;
        connected.value = false;
        appendLog(t('flash.log.autoDisconnected'));
      }
    }
  }

  /** Trigger authorize in read-only mode regardless of form credentials.
   *  Temporarily clears uuid/authkey → startOperation sees both empty → auth-read path.
   *  This reuses the fully-tested startOperation state machine (connects, logs, Done handler). */
  async function startAuthRead(): Promise<void> {
    const savedUuid = authorizeUuid.value;
    const savedKey = authorizeAuthKey.value;
    authorizeUuid.value = '';
    authorizeAuthKey.value = '';
    authOpIsRead.value = true;
    try {
      await startOperation('authorize');
    } finally {
      authorizeUuid.value = savedUuid;
      authorizeAuthKey.value = savedKey;
      // authOpIsRead is cleared by the done handler, not here.
    }
  }

  async function ensureFlashListener(): Promise<void> {
    if (unlistenFlash || !isTauriRuntime()) {
      return;
    }
    const { listen } = await import('@tauri-apps/api/event');
    unlistenFlash = await listen<FlashProgressPayload>('flash-progress', ev => {
      handleFlashProgressPayload(ev.payload);
    });
  }

  /** Fingerprint of last scan result to suppress duplicate log lines. `undefined` = never scanned yet (distinct from empty list `''`). */
  let lastScanFingerprint: string | undefined = undefined;

  async function refreshDevice(): Promise<void> {
    if (isTauriRuntime()) {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const ports = await invoke<TauriSerialPortRow[]>('list_serial_ports_cmd');
        serialPortOptions.value = ports.map(p => {
          const tip = tuyaDualSerialHoverTooltip(p.usbVid, p.usbPid, p.usbInterface, t);
          const row: SerialPortDropdownOption = {
            value: p.path,
            label: formatSerialPortLabel(p, t),
          };
          if (tip) {
            row.optionTooltip = tip;
          }
          return row;
        });

        // Fingerprint includes role/interface so label updates when metadata changes
        const fingerprint = ports.map(p => `${p.path}:${p.portRole ?? ''}:${p.usbInterface ?? ''}`).join(',');

        if (ports.length > 0) {
          const exists = ports.some(p => p.path === selectedSerialPort.value);
          if (!exists) {
            selectedSerialPort.value = ports[0].path;
          }
          // Only log when the port list actually changed
          if (fingerprint !== lastScanFingerprint) {
            appendLog(
              t('flash.log.portsFound', {
                list: serialPortOptions.value
                  .map((x: SerialPortDropdownOption) => x.label)
                  .join(locale.value === 'zh-CN' ? '、' : ', '),
              })
            );
          }
        } else {
          selectedSerialPort.value = '';
          connected.value = false;
          if (fingerprint !== lastScanFingerprint) {
            appendLog(t('flash.log.noPortsFound'));
          }
        }
        lastScanFingerprint = fingerprint;
      } catch {
        appendLog(t('flash.log.portsListFailed'));
        serialPortOptions.value = [];
        selectedSerialPort.value = '';
        connected.value = false;
        lastScanFingerprint = undefined;
      }
    } else {
      // Web mode: ask the local serve process for ports
      try {
        const ports = await wsTransport.listPorts();
        serialPortOptions.value = ports.map(p => {
          const tip = tuyaDualSerialHoverTooltip(p.usbVid, p.usbPid, p.usbInterface, t);
          const row: SerialPortDropdownOption = {
            value: p.path,
            label: formatSerialPortLabel(p, t),
          };
          if (tip) {
            row.optionTooltip = tip;
          }
          return row;
        });
        const fingerprint = ports.map(p => `${p.path}:${p.portRole ?? ''}:${p.usbInterface ?? ''}`).join(',');
        if (ports.length > 0) {
          const exists = ports.some(p => p.path === selectedSerialPort.value);
          if (!exists) selectedSerialPort.value = ports[0].path;
          if (fingerprint !== lastScanFingerprint) {
            appendLog(
              t('flash.log.portsFound', {
                list: serialPortOptions.value
                  .map((x: SerialPortDropdownOption) => x.label)
                  .join(locale.value === 'zh-CN' ? '、' : ', '),
              })
            );
          }
        } else {
          selectedSerialPort.value = '';
          connected.value = false;
          if (fingerprint !== lastScanFingerprint) {
            appendLog(t('flash.log.noPortsFound'));
            appendLog(t('flash.log.noPortsFoundWebServeHint'));
          }
        }
        lastScanFingerprint = fingerprint;
      } catch {
        appendLog('WS: 无法连接到 tyutool-cli serve (ws://127.0.0.1:9527)');
        serialPortOptions.value = [];
        selectedSerialPort.value = '';
        connected.value = false;
        lastScanFingerprint = undefined;
      }
    }
  }

  async function connect(): Promise<void> {
    if (!selectedSerialPort.value) {
      appendLog(t('flash.log.noPortsFound'));
      return;
    }
    if (isTauriRuntime()) {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const result = await invoke<{
          available: boolean;
          errorMessage?: string | null;
          processInfo?: string | null;
          killHint?: string | null;
        }>('check_port_available_cmd', { port: selectedSerialPort.value });

        if (!result.available) {
          appendLog(t('flash.log.portBusy', { port: selectedSerialPort.value }));
          if (result.errorMessage) appendLog(t('flash.log.portBusyDetail', { msg: result.errorMessage }));
          if (result.processInfo) appendLog(t('flash.log.portBusyProcess', { info: result.processInfo }));
          if (result.killHint) appendLog(t('flash.log.portBusyKillHint', { cmd: result.killHint }));
          return;
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        appendLog(t('flash.log.portCheckFailed', { msg }));
        // Continue — let the real operation fail if there's really a problem
      }
    }
    connected.value = true;
    autoConnected.value = false;
    appendLog(t('flash.log.connected'));
    rLog.info(`[Flash] Connected to port: ${selectedSerialPort.value}`);
  }

  function disconnect(): void {
    if (busy.value) {
      // Cancel the running operation before disconnecting
      stopFlash();
      runningOp.value = null;
      flashPhase.value = 'idle';
      flashProgress.value = 0;
      flashMessage.value = '';
      appendLog(t('flash.log.operationCancelled'));
      rLog.info('[Flash] Operation cancelled by user');
    }
    rLog.info(`[Flash] Disconnected from port: ${selectedSerialPort.value}`);
    connected.value = false;
    autoConnected.value = false;
    appendLog(t('flash.log.disconnected'));
  }

  async function deviceReset(): Promise<void> {
    if (!selectedSerialPort.value.trim() || busy.value) {
      return;
    }
    const chipId = rustPluginIdForChip(selectedChipId.value);
    try {
      if (isTauriRuntime()) {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('device_reset_cmd', {
          args: {
            port: selectedSerialPort.value,
            chipId,
          },
        });
      } else {
        await wsTransport.deviceReset(selectedSerialPort.value, chipId);
      }
      appendLog(t('flash.log.deviceResetOk', { port: selectedSerialPort.value }));
      rLog.info(`[Flash] Device reset (DTR/RTS) on ${selectedSerialPort.value}`);
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      if (raw.includes('unknown variant') && raw.includes('device_reset')) {
        appendLog(t('flash.log.deviceResetServeOutdated'));
      } else {
        appendLog(t('flash.log.deviceResetFailed', { msg: raw }));
      }
      rLog.warn(`[Flash] Device reset failed: ${raw}`);
    }
  }

  function opTitle(kind: OpKind): string {
    switch (kind) {
      case 'flash':
        return t('flash.tabs.flash');
      case 'erase':
        return t('flash.tabs.erase');
      case 'read':
        return t('flash.tabs.read');
      case 'authorize':
        return t('flash.tabs.authorize');
      default:
        return '';
    }
  }

  function buildFlashJob(kind: OpKind): FlashJobPayload {
    const chipId = rustPluginIdForChip(selectedChipId.value);
    return {
      mode: kind,
      chipId,
      port: selectedSerialPort.value,
      baudRate: kind === 'authorize' ? selectedAuthBaudRate.value : selectedBaudRate.value,
      flashStartHex: kind === 'flash' ? formatAddrHex(flashStartAddr.value) : null,
      flashEndHex: kind === 'flash' ? formatAddrHex(flashEndAddr.value) : null,
      eraseStartHex: kind === 'erase' ? formatAddrHex(eraseStartAddr.value) : null,
      eraseEndHex: kind === 'erase' ? formatAddrHex(eraseEndAddr.value) : null,
      readStartHex: kind === 'read' ? formatAddrHex(readStartAddr.value) : null,
      readEndHex: kind === 'read' ? formatAddrHex(readEndAddr.value) : null,
      readFilePath: kind === 'read' && readFilePath.value.trim() ? readFilePath.value.trim() : null,
      firmwarePath: kind === 'flash' && firmwarePath.value.trim() ? firmwarePath.value.trim() : null,
      segments:
        kind === 'flash'
          ? flashSegments.value.map(s => ({
              firmwarePath: s.firmwarePath,
              startAddr: formatAddrHex(s.startAddr),
              endAddr: formatAddrHex(s.endAddr),
            }))
          : null,
      authorizeUuid: kind === 'authorize' ? authorizeUuid.value.trim() || null : null,
      authorizeKey: kind === 'authorize' ? authorizeAuthKey.value.trim() || null : null,
    };
  }

  async function startOperationTauri(kind: OpKind): Promise<void> {
    await ensureFlashListener();
    const { invoke } = await import('@tauri-apps/api/core');
    const job = buildFlashJob(kind);
    await invoke('flash_run', { job });
  }

  async function startOperationWs(kind: OpKind): Promise<void> {
    const job = buildFlashJob(kind);

    // For flash mode, send all File objects; server decodes and uses temp paths
    const filesToUpload = kind === 'flash' ? flashSegments.value.map(s => s.firmwareFile) : [firmwareFile.value];

    // For read mode in web, the server saves to a temp path and returns file_content
    if (kind === 'read') {
      job.readFilePath = null; // server uses temp path
    }

    await wsTransport.runJob(job, filesToUpload, (ev: WsProgressEvent) => {
      handleFlashProgressPayload(ev.payload);

      // If server sent a file_content message (read mode), trigger browser download
      if (ev.fileContent) {
        triggerBrowserDownload(ev.fileContent.content, readFileName.value || ev.fileContent.name);
      }
    });
  }

  function triggerBrowserDownload(base64Content: string, filename: string): void {
    const binary = atob(base64Content);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    const blob = new Blob([bytes], { type: 'application/octet-stream' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  }

  function applyErasePreset(kind: ErasePresetKind): void {
    if (busy.value) {
      return;
    }
    const preset = chipManifest(selectedChipId.value).erasePresets[kind];
    if (!preset) {
      return;
    }
    eraseStartAddr.value = preset.start;
    eraseEndAddr.value = preset.end;
    const labelKey =
      kind === 'authInfo'
        ? 'flash.eraseAuthInfo'
        : kind === 'fullChipNoRf'
          ? 'flash.eraseFullChipNoRf'
          : 'flash.eraseFullChip';
    appendLog(
      t('flash.log.erasePresetApplied', {
        chip: selectedChipLabel.value,
        label: t(labelKey),
        start: preset.start,
        end: preset.end,
      })
    );
  }

  async function startOperation(kind: OpKind): Promise<void> {
    if (flashPhase.value === 'running') {
      return;
    }

    // ── 1. Input validation ────────────────────────────────────────
    if (kind === 'flash') {
      const anyEmpty = flashSegments.value.some(s => !s.firmwarePath.trim());
      if (anyEmpty) {
        flashMessage.value = t('flash.err.selectFirmware');
        flashPhase.value = 'error';
        appendLog(t('flash.err.selectFirmwareLog'));
        return;
      }
    }
    if (kind === 'read' && !readDir.value.trim() && isTauriRuntime()) {
      flashMessage.value = t('flash.err.selectReadDir');
      flashPhase.value = 'error';
      appendLog(t('flash.err.selectReadDirLog'));
      return;
    }
    if (kind === 'read' && !readFileName.value.trim()) {
      flashMessage.value = t('flash.err.selectReadFileName');
      flashPhase.value = 'error';
      appendLog(t('flash.err.selectReadFileNameLog'));
      return;
    }
    if (kind === 'authorize') {
      const hasUuid = !!authorizeUuid.value.trim();
      const hasKey = !!authorizeAuthKey.value.trim();
      if (hasUuid !== hasKey) {
        flashMessage.value = t('flash.err.fillAuthLog');
        flashPhase.value = 'error';
        appendLog(t('flash.err.fillAuthLog'));
        return;
      }
      if (hasUuid && authorizeUuid.value.trim().length !== 20) {
        const msg = t('flash.err.authUuidLen');
        flashMessage.value = msg;
        flashPhase.value = 'error';
        appendLog(t('flash.err.withMsg', { msg }));
        return;
      }
      if (hasKey && authorizeAuthKey.value.trim().length !== 32) {
        const msg = t('flash.err.authKeyLen');
        flashMessage.value = msg;
        flashPhase.value = 'error';
        appendLog(t('flash.err.withMsg', { msg }));
        return;
      }
    }
    if (!selectedSerialPort.value) {
      flashMessage.value = t('flash.err.deviceDisconnected');
      flashPhase.value = 'error';
      appendLog(t('flash.err.deviceDisconnectedLog'));
      return;
    }

    if (kind === 'flash') {
      for (let i = 0; i < flashSegments.value.length; i++) {
        const seg = flashSegments.value[i];
        const err = validateAddrRange(seg.startAddr, seg.endAddr);
        if (err) {
          const msg = `${t('flash.segment')} ${i + 1}: ${addrRangeMessage(err)}`;
          flashMessage.value = msg;
          flashPhase.value = 'error';
          appendLog(t('flash.err.withMsg', { msg }));
          return;
        }
      }
    }
    if (kind === 'erase') {
      const err = validateAddrRange(eraseStartAddr.value, eraseEndAddr.value);
      if (err) {
        const msg = addrRangeMessage(err);
        flashMessage.value = msg;
        flashPhase.value = 'error';
        appendLog(t('flash.err.withMsg', { msg }));
        return;
      }
    }
    if (kind === 'read') {
      const err = validateAddrRange(readStartAddr.value, readEndAddr.value);
      if (err) {
        const msg = addrRangeMessage(err);
        flashMessage.value = msg;
        flashPhase.value = 'error';
        appendLog(t('flash.err.withMsg', { msg }));
        return;
      }
    }

    // ── 2. Erase confirmation dialog ───────────────────────────────
    if (kind === 'erase') {
      const start = formatAddrHex(eraseStartAddr.value);
      const end = formatAddrHex(eraseEndAddr.value);
      const chip = selectedChipLabel.value;

      let confirmMsg = t('flash.confirm.eraseBody', { chip, start, end });
      let okLabel = t('flash.confirm.eraseOk');

      if (chipManifest(selectedChipId.value).eraseRequires4KAlignment) {
        const sa = parseHexAddr(eraseStartAddr.value);
        const ea = parseHexAddr(eraseEndAddr.value);
        if (sa !== null && ea !== null && exclusiveEraseRangeNeeds4KAlignment(sa, ea)) {
          const { alignedStart, alignedEndExclusive } = alignedExclusiveEraseRange4K(sa, ea);
          confirmMsg = t('flash.confirm.eraseBodyMisaligned4k', {
            chip,
            start,
            end,
            sectorHex: '0x1000',
            alignedStart: formatBigIntAddrHex(alignedStart),
            alignedEnd: formatBigIntAddrHex(alignedEndExclusive),
          });
          okLabel = t('flash.confirm.eraseOkAlign');
        }
      }

      const confirmed = await showConfirmDialog({
        title: t('flash.confirm.eraseTitle'),
        message: confirmMsg,
        kind: 'warning',
        okLabel,
        cancelLabel: t('flash.confirm.eraseCancel'),
      });

      if (!confirmed) {
        appendLog(t('flash.log.eraseCancelled'));
        return;
      }

      if (chipManifest(selectedChipId.value).eraseRequires4KAlignment) {
        const sa = parseHexAddr(eraseStartAddr.value);
        const ea = parseHexAddr(eraseEndAddr.value);
        if (sa !== null && ea !== null && exclusiveEraseRangeNeeds4KAlignment(sa, ea)) {
          const { alignedStart, alignedEndExclusive } = alignedExclusiveEraseRange4K(sa, ea);
          eraseStartAddr.value = formatBigIntAddrHex(alignedStart);
          eraseEndAddr.value = formatBigIntAddrHex(alignedEndExclusive);
          appendLog(
            t('flash.log.eraseRangeAligned', {
              fromStart: start,
              fromEnd: end,
              toStart: eraseStartAddr.value,
              toEnd: eraseEndAddr.value,
            })
          );
        }
      }
    }

    // ── 3. Read file existence check ───────────────────────────────
    if (kind === 'read' && isTauriRuntime()) {
      try {
        const fullPath = readFilePath.value;
        const { invoke } = await import('@tauri-apps/api/core');
        const exists = await invoke<boolean>('check_file_exists', {
          path: fullPath,
        });
        if (exists) {
          const overwrite = await showConfirmDialog({
            title: t('flash.confirm.readFileExistsTitle'),
            message: t('flash.confirm.readFileExistsBody', { path: fullPath }),
            kind: 'warning',
            okLabel: t('flash.confirm.readOverwrite'),
            cancelLabel: t('flash.confirm.readTimestamp'),
          });
          if (overwrite) {
            appendLog(t('flash.log.readOverwriting', { path: fullPath }));
          } else {
            readFileName.value = addTimestampSuffix(readFileName.value.trim());
            readFileNameModified.value = true;
            appendLog(t('flash.log.readTimestamped', { path: readFilePath.value }));
          }
        }
      } catch {
        /* proceed with original path */
      }
    }

    // ── 4. Auto-connect if not manually connected ──────────────────
    if (!connected.value) {
      appendLog(t('flash.log.autoConnecting', { port: selectedSerialPort.value }));
      await connect();
      if (!connected.value) {
        // connect() failed (port busy etc.) — error already logged
        flashMessage.value = t('flash.err.portUnavailable');
        flashPhase.value = 'error';
        return;
      }
      autoConnected.value = true;
    }

    // ── 4b. Authorize write: probe device auth in UI, confirm overwrite if different ──
    if (kind === 'authorize' && authorizeUuid.value.trim() && authorizeAuthKey.value.trim() && !authOpIsRead.value) {
      const nu = authorizeUuid.value.trim();
      const nk = authorizeAuthKey.value.trim();
      try {
        let existing: { uuid: string; authkey: string } | null = null;
        if (isTauriRuntime()) {
          const { invoke } = await import('@tauri-apps/api/core');
          existing = await invoke<{ uuid: string; authkey: string } | null>('authorize_probe_cmd', {
            port: selectedSerialPort.value,
          });
        } else {
          existing = await wsTransport.authorizeProbe(
            selectedSerialPort.value,
            rustPluginIdForChip(selectedChipId.value),
            selectedAuthBaudRate.value
          );
        }
        if (existing) {
          const eu = existing.uuid.trim();
          const ek = existing.authkey.trim();
          if (eu && ek && eu !== AUTHORIZE_PLACEHOLDER_UUID && (eu !== nu || ek !== nk)) {
            const confirmed = await showConfirmDialog({
              title: t('flash.confirm.authOverwriteTitle'),
              message: t('flash.confirm.authOverwriteBody', {
                existingUuid: eu,
                existingAuthkey: ek,
                newUuid: nu,
                newAuthkey: nk,
              }),
              kind: 'warning',
              okLabel: t('flash.confirm.authOverwriteOk'),
              cancelLabel: t('flash.confirm.authOverwriteCancel'),
            });
            if (!confirmed) {
              appendLog(t('flash.log.authOverwriteCancelled'));
              return;
            }
          }
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        appendLog(t('flash.log.authProbeSkipped', { msg }));
      }
    }

    // ── 5. Start operation ─────────────────────────────────────────
    flashPhase.value = 'running';
    operationStartTime = Date.now();
    runningOp.value = kind;
    flashProgress.value = 0;
    flashMessage.value = '';

    rLog.info(
      `[Flash] Starting '${kind}' — chip=${selectedChipId.value}, port=${selectedSerialPort.value}, baud=${selectedBaudRate.value}`
    );

    const chip = selectedChipLabel.value;
    appendLog(t('flash.log.targetChip', { chip }));
    appendLog(t('flash.log.operation', { op: opTitle(kind) }));

    if (kind === 'flash') {
      flashSegments.value.forEach((seg, i) => {
        appendLog(t('flash.log.segmentLog', { n: i + 1 }));
        appendLog(t('flash.log.firmware', { path: seg.firmwarePath }));
        appendLog(
          t('flash.log.flashRangeLog', {
            start: formatAddrHex(seg.startAddr),
            end: formatAddrHex(seg.endAddr),
          })
        );
      });
      appendLog(t('flash.log.baud', { n: String(selectedBaudRate.value) }));
    } else if (kind === 'erase') {
      appendLog(
        t('flash.log.eraseRangeLog', {
          start: formatAddrHex(eraseStartAddr.value),
          end: formatAddrHex(eraseEndAddr.value),
        })
      );
      appendLog(t('flash.log.erasePrep'));
    } else if (kind === 'read') {
      appendLog(
        t('flash.log.readRangeLog', {
          start: formatAddrHex(readStartAddr.value),
          end: formatAddrHex(readEndAddr.value),
        })
      );
      appendLog(t('flash.log.readSave', { path: readFilePath.value }));
      appendLog(t('flash.log.readPrep'));
    } else if (kind === 'authorize') {
      const hasCredentials = !!authorizeUuid.value.trim();
      appendLog(t(hasCredentials ? 'flash.log.authPrep' : 'flash.log.authReadPrep'));
    }

    if (isTauriRuntime()) {
      try {
        await startOperationTauri(kind);
      } catch (e) {
        runningOp.value = null;
        flashPhase.value = 'error';
        const msg = e instanceof Error ? e.message : String(e);
        flashMessage.value = msg;
        appendLog(t('flash.err.withMsg', { msg }));
        logOperationDuration();
      }
    } else {
      try {
        await startOperationWs(kind);
      } catch (e) {
        runningOp.value = null;
        flashPhase.value = 'error';
        const msg = e instanceof Error ? e.message : String(e);
        flashMessage.value = msg;
        appendLog(t('flash.err.withMsg', { msg }));
        logOperationDuration();
      }
    }
  }

  function resetFlash(): void {
    if (busy.value) {
      // Don't cancel a running operation — only reset idle/success/error state
      return;
    }
    stopFlash();
    runningOp.value = null;
    flashPhase.value = 'idle';
    flashProgress.value = 0;
    flashMessage.value = '';
  }

  /** Call from component's onUnmounted to release timers and listeners. */
  function cleanup(): void {
    stopFlash();
    if (unlistenFlash) {
      unlistenFlash();
      unlistenFlash = undefined;
    }
  }

  function buildWorkspaceSnapshot(): FlashWorkspaceSerialized {
    return {
      v: WORKSPACE_VERSION,
      activeTab: activeTab.value,
      selectedSerialPort: selectedSerialPort.value,
      selectedBaudRate: selectedBaudRate.value,
      selectedChipId: selectedChipId.value,
      flashSegments: flashSegments.value.map(s => ({
        id: s.id,
        firmwarePath: s.firmwarePath,
        startAddr: s.startAddr,
        endAddr: s.endAddr,
      })),
      activeSegmentIndex: activeSegmentIndex.value,
      eraseAdvancedOpen: eraseAdvancedOpen.value,
      eraseStartAddr: eraseStartAddr.value,
      eraseEndAddr: eraseEndAddr.value,
      readStartAddr: readStartAddr.value,
      readEndAddr: readEndAddr.value,
      readDir: readDir.value,
      readFileName: readFileName.value,
      readFileNameModified: readFileNameModified.value,
      authorizeUuid: authorizeUuid.value,
      authorizeAuthKey: authorizeAuthKey.value,
      authBaudRate: selectedAuthBaudRate.value,
    };
  }

  /**
   * Restore last session fields from disk (Tauri) or localStorage (web).
   * Call once after Pinia init and before `refreshDevice`, and before `startWorkspacePersistence`.
   */
  async function loadWorkspace(): Promise<void> {
    const data = await loadFlashWorkspaceFromStorage();
    if (!data) {
      return;
    }
    workspaceRestoreMuted.value = true;
    try {
      selectedChipId.value = data.selectedChipId;
      selectedBaudRate.value = data.selectedBaudRate;
      activeTab.value = data.activeTab;
      flashSegments.value = data.flashSegments.map(s => ({
        id: s.id,
        firmwarePath: s.firmwarePath,
        firmwareFile: null,
        startAddr: s.startAddr,
        endAddr: s.endAddr,
      }));
      activeSegmentIndex.value = data.activeSegmentIndex;
      eraseAdvancedOpen.value = data.eraseAdvancedOpen;
      eraseStartAddr.value = data.eraseStartAddr;
      eraseEndAddr.value = data.eraseEndAddr;
      readStartAddr.value = data.readStartAddr;
      readEndAddr.value = data.readEndAddr;
      readDir.value = data.readDir;
      readFileName.value = data.readFileName;
      readFileNameModified.value = data.readFileNameModified;
      authorizeUuid.value = data.authorizeUuid;
      authorizeAuthKey.value = data.authorizeAuthKey;
      selectedAuthBaudRate.value = data.authBaudRate;
      selectedSerialPort.value = data.selectedSerialPort;
    } finally {
      workspaceRestoreMuted.value = false;
    }
    if (isTauriRuntime()) {
      for (let i = 0; i < flashSegments.value.length; i++) {
        const p = flashSegments.value[i].firmwarePath.trim();
        if (!p) {
          continue;
        }
        try {
          const { invoke } = await import('@tauri-apps/api/core');
          const size = await invoke<number>('get_file_size', { path: p });
          updateFlashEndAddr(i, size);
        } catch {
          /* ignore */
        }
      }
    }
    rLog.info('[Flash] Workspace restored from last session');
  }

  /** Debounced auto-save of workspace; call once at app startup after `loadWorkspace`. */
  function startWorkspacePersistence(): void {
    if (workspacePersistStarted) {
      return;
    }
    workspacePersistStarted = true;
    const debounced = createDebounced(() => {
      void saveFlashWorkspaceToStorage(buildWorkspaceSnapshot());
    }, 450);
    watch(
      () => ({
        activeTab: activeTab.value,
        selectedSerialPort: selectedSerialPort.value,
        selectedBaudRate: selectedBaudRate.value,
        selectedChipId: selectedChipId.value,
        flashSegments: flashSegments.value.map(s => ({
          id: s.id,
          firmwarePath: s.firmwarePath,
          startAddr: s.startAddr,
          endAddr: s.endAddr,
        })),
        activeSegmentIndex: activeSegmentIndex.value,
        eraseAdvancedOpen: eraseAdvancedOpen.value,
        eraseStartAddr: eraseStartAddr.value,
        eraseEndAddr: eraseEndAddr.value,
        readStartAddr: readStartAddr.value,
        readEndAddr: readEndAddr.value,
        readDir: readDir.value,
        readFileName: readFileName.value,
        readFileNameModified: readFileNameModified.value,
        authorizeUuid: authorizeUuid.value,
        authorizeAuthKey: authorizeAuthKey.value,
        authBaudRate: selectedAuthBaudRate.value,
      }),
      debounced,
      { deep: true }
    );
  }

  const busy = computed(() => flashPhase.value === 'running');

  const canFlash = computed(() => {
    if (busy.value || !selectedSerialPort.value) return false;
    return flashSegments.value.every(s => !!s.firmwarePath.trim());
  });

  const canErase = computed(() => !busy.value && !!selectedSerialPort.value);

  const canRead = computed(
    () =>
      !busy.value &&
      (isTauriRuntime() ? !!readDir.value.trim() : true) &&
      !!readFileName.value.trim() &&
      !!selectedSerialPort.value
  );

  const canAuthorize = computed(
    () => !busy.value && !!selectedSerialPort.value && !!authorizeUuid.value.trim() && !!authorizeAuthKey.value.trim()
  );

  /** Read-only auth — only needs a connected port. */
  const canReadAuth = computed(() => !busy.value && !!selectedSerialPort.value);

  const progressCaption = computed(() => {
    if (flashPhase.value !== 'running' || !runningOp.value) {
      return t('flash.progress');
    }
    return t('flash.progressWith', { op: opTitle(runningOp.value) });
  });

  const statusText = computed(() => (connected.value ? t('flash.statusConnected') : t('flash.statusDisconnected')));

  const tabList = computed(() => [
    { id: 'flash' as const, label: t('flash.tabs.flash') },
    { id: 'erase' as const, label: t('flash.tabs.erase') },
    { id: 'read' as const, label: t('flash.tabs.read') },
    // UART-only TuyaOpen authorization — same for all chip platforms in the UI
    { id: 'authorize' as const, label: t('flash.tabs.authorize') },
  ]);

  return {
    CHIP_IDS,
    SERIAL_PORT_OPTIONS: serialPortOptions,
    BAUD_RATE_OPTIONS,
    activeTab,
    connected,
    selectedSerialPort,
    selectedBaudRate,
    selectedAuthBaudRate,
    selectedChipId,
    firmwareFile,
    fileInputRef,
    eraseAdvancedOpen,
    flashStartAddr,
    flashEndAddr,
    eraseStartAddr,
    eraseEndAddr,
    readStartAddr,
    readEndAddr,
    readDir,
    readFileName,
    readFileNameModified,
    readFilePath,
    authorizeUuid,
    authorizeAuthKey,
    flashProgress,
    flashPhase,
    flashMessage,
    runningOp,
    logLines,
    logScrollRef,
    lockAutoScroll,
    selectedChipLabel,
    flashSegments,
    activeSegmentIndex,
    appendLog,
    clearLogs,
    copyLogs,
    onPickFile,
    onFileChange,
    onPickReadDir,
    onReadFileNameInput,
    addSegment,
    removeSegment,
    refreshDevice,
    ensureFlashListener,
    connect,
    disconnect,
    deviceReset,
    applyErasePreset,
    startOperation,
    resetFlash,
    cleanup,
    busy,
    canFlash,
    canErase,
    canRead,
    canAuthorize,
    canReadAuth,
    startAuthRead,
    authOpIsRead,
    autoConnected,
    progressCaption,
    statusText,
    tabList,
    loadWorkspace,
    startWorkspacePersistence,
  };
});
