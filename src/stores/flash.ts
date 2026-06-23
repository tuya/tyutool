import { computed, ref, watch } from "vue";
import { i18n } from "@/i18n";
import {
  AUTH_CHIP_IDS,
  AUTH_ONLY_CHIP_ID,
  BAUD_RATE_OPTIONS,
  CHIP_IDS,
  DEFAULT_CHIP_ID,
} from "@/features/firmware-flash/constants";
import {
  chipManifest,
  rustPluginIdForChip,
} from "@/features/firmware-flash/chip-manifests";
import { useFlashLog } from "@/features/firmware-flash/useFlashLog";
import { useFlashProgress } from "@/features/firmware-flash/useFlashProgress";
import { useFlashConnection } from "@/features/firmware-flash/useFlashConnection";
import { validateOperation } from "@/features/firmware-flash/validate-operation";
import { triggerBrowserDownload } from "@/features/firmware-flash/browser-download";
import { usePortManagerStore } from "@/stores/port-manager";
import type {
  FlashJobPayload,
  FlashProgressPayload,
} from "@/features/firmware-flash/flash-ipc-types";
import { getRuntime, isTauriRuntime } from "@/runtime";
import { platform } from "../platform";
import {
  alignedExclusiveEraseRange4K,
  exclusiveEraseRangeNeeds4KAlignment,
  formatAddrHex,
  formatBigIntAddrHex,
  parseHexAddr,
} from "@/features/firmware-flash/hex";
import {
  addTimestampSuffix,
  formatDuration,
} from "@/features/firmware-flash/utils";
import type {
  ErasePresetKind,
  FlashSegment,
  OpKind,
} from "@/features/firmware-flash/types";
import { showConfirmDialog } from "@/composables/confirmDialog";
import { rLog } from "@/utils/log";
import { defineStore } from "pinia";
import { wsTransport } from "@/transport/ws-transport";
import type { WsProgressEvent } from "@/transport/ws-transport";
import {
  loadFlashWorkspaceFromStorage,
  saveFlashWorkspaceToStorage,
  WORKSPACE_VERSION,
  type FlashWorkspaceSerialized,
} from "@/stores/flash-workspace";

/** Factory-unauthorized placeholder from TuyaOpen firmware (matches `authorize.rs`). */
const AUTHORIZE_PLACEHOLDER_UUID = "uuidxxxxxxxxxxxxxxxx";

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

export const useFlashStore = defineStore("flash", () => {
  const t = i18n.global.t;

  /** When true, `selectedChipId` watch skips side effects (used during workspace restore). */
  const workspaceRestoreMuted = ref(false);
  let workspacePersistStarted = false;

  const activeTab = ref<OpKind>("flash");
  const connected = ref(false);
  const selectedSerialPort = ref("");
  const selectedBaudRate = ref<number>(
    chipManifest(DEFAULT_CHIP_ID).defaultBaudRate,
  );
  /** Baud rate for TuyaOpen UART authorization — independent of flash/erase/read baud. */
  const selectedAuthBaudRate = ref<number>(
    chipManifest(DEFAULT_CHIP_ID).defaultAuthBaudRate,
  );
  const selectedChipId = ref<string>(DEFAULT_CHIP_ID);
  /** Last flash-capable chip — restored when leaving authorize tab with "other" selected. */
  const lastFlashChipId = ref<string>(DEFAULT_CHIP_ID);

  const flashSegments = ref<FlashSegment[]>([
    {
      id: Math.random().toString(36).substring(2, 9),
      firmwarePath: "",
      firmwareFile: null,
      startAddr: "0x00000000",
      endAddr: "0x00000000",
    },
  ]);
  const activeSegmentIndex = ref(0);

  const fileInputRef = ref<HTMLInputElement | null>(null);
  const eraseAdvancedOpen = ref(false);

  const flashStartAddr = computed({
    get: () => flashSegments.value[0].startAddr,
    set: (val) => {
      flashSegments.value[0].startAddr = val;
    },
  });
  const flashEndAddr = computed({
    get: () => flashSegments.value[0].endAddr,
    set: (val) => {
      flashSegments.value[0].endAddr = val;
    },
  });
  const firmwarePath = computed({
    get: () => flashSegments.value[0].firmwarePath,
    set: (val) => {
      flashSegments.value[0].firmwarePath = val;
    },
  });
  const firmwareFile = computed({
    get: () => flashSegments.value[0].firmwareFile,
    set: (val) => {
      flashSegments.value[0].firmwareFile = val;
    },
  });

  const eraseStartAddr = ref("0x00000000");
  const eraseEndAddr = ref("0x00000000");
  const readStartAddr = ref("0x00000000");
  const readEndAddr = ref(chipManifest(DEFAULT_CHIP_ID).flashSize);
  const readDir = ref("");
  const readFileName = ref(
    `tyutool_read_${selectedChipId.value.toLowerCase()}.bin`,
  );
  const readFileNameModified = ref(false);
  const authorizeUuid = ref("");
  const authorizeAuthKey = ref("");
  const autoConnected = ref(false);

  let progressTimer: ReturnType<typeof setInterval> | null = null;
  let unlistenFlash: (() => void) | undefined;
  let operationStartTime: number | null = null;

  const {
    logLines,
    logScrollRef,
    lockAutoScroll,
    appendLog,
    clearLogs,
    copyLogs,
  } = useFlashLog();

  const selectedChipLabel = computed(() => {
    const id = selectedChipId.value;
    return t(`flash.chips.${id}`);
  });

  function logOperationDuration(): void {
    if (operationStartTime !== null) {
      const elapsed = Date.now() - operationStartTime;
      appendLog(
        t("flash.log.operationDuration", { duration: formatDuration(elapsed) }),
      );
      operationStartTime = null;
    }
  }

  const {
    flashProgress,
    flashPhase,
    flashMessage,
    runningOp,
    currentBackendPhase,
    phaseProgress,
    phaseIndeterminate,
    authOpIsRead,
    cancelIndeterminateCheck,
    handleFlashProgressPayload,
  } = useFlashProgress({
    appendLog,
    logOperationDuration,
    onOperationSettled: () => {
      // The flash operation closes the serial port on the Rust side when it
      // finishes (success, error, or cancel). Sync GUI state accordingly so
      // the port is visibly released and available for other features.
      connected.value = false;
      autoConnected.value = false;
      appendLog(t("flash.log.disconnected"));
      usePortManagerStore().release(selectedSerialPort.value, "flash");
    },
  });

  const busy = computed(() => flashPhase.value === "running");

  watch(activeTab, (tab) => {
    if (workspaceRestoreMuted.value) {
      return;
    }
    if (tab !== "authorize" && selectedChipId.value === AUTH_ONLY_CHIP_ID) {
      selectedChipId.value = lastFlashChipId.value;
    }
  });

  // Auto-update readFileName, readEndAddr and baudRate when chip changes
  watch(selectedChipId, (newChipId, oldChipId) => {
    if (workspaceRestoreMuted.value) {
      return;
    }
    if (newChipId === AUTH_ONLY_CHIP_ID) {
      if (oldChipId && oldChipId !== AUTH_ONLY_CHIP_ID) {
        lastFlashChipId.value = oldChipId;
      }
      selectedAuthBaudRate.value =
        chipManifest(AUTH_ONLY_CHIP_ID).defaultAuthBaudRate;
      const chipLabel = t(`flash.chips.${AUTH_ONLY_CHIP_ID}`);
      appendLog(t("flash.log.chipChanged", { chip: chipLabel }));
      rLog.info(
        `[Flash] Chip changed to ${AUTH_ONLY_CHIP_ID} (${chipLabel}), auth-only`,
      );
      return;
    }
    lastFlashChipId.value = newChipId;
    if (!readFileNameModified.value) {
      readFileName.value = `tyutool_read_${newChipId.toLowerCase()}.bin`;
    }
    const manifest = chipManifest(newChipId);
    readEndAddr.value = manifest.flashSize;
    selectedBaudRate.value = manifest.defaultBaudRate;
    selectedAuthBaudRate.value = manifest.defaultAuthBaudRate;
    const chipLabel = t(`flash.chips.${newChipId}`);
    appendLog(t("flash.log.chipChanged", { chip: chipLabel }));
    rLog.info(
      `[Flash] Chip changed to ${newChipId} (${chipLabel}), baud=${manifest.defaultBaudRate}`,
    );
  });

  function onReadFileNameInput(value: string): void {
    readFileName.value = value;
    readFileNameModified.value = true;
  }

  const readFilePath = computed(() => {
    const dir = readDir.value.trim();
    const name = readFileName.value.trim();
    if (!dir || !name) return "";
    // Normalize: ensure exactly one separator between dir and name
    const sep = dir.endsWith("/") || dir.endsWith("\\") ? "" : "/";
    return `${dir}${sep}${name}`;
  });

  function updateFlashEndAddr(index: number, fileSize: number): void {
    const seg = flashSegments.value[index];
    const startVal = parseHexAddr(seg.startAddr);
    const start = startVal !== null ? Number(startVal) : 0;
    const end = start + fileSize;
    seg.endAddr = `0x${end.toString(16).toUpperCase().padStart(8, "0")}`;
  }

  /** Default start/end for a new row: chain from previous segment's end (same value until a file sets the end). */
  function initialAddrsForNewSegment(): { startAddr: string; endAddr: string } {
    const prev = flashSegments.value[flashSegments.value.length - 1];
    const parsed = parseHexAddr(prev.endAddr);
    if (parsed === null) {
      return { startAddr: "0x00000000", endAddr: "0x00000000" };
    }
    const normalized = `0x${parsed.toString(16).toUpperCase().padStart(8, "0")}`;
    return { startAddr: normalized, endAddr: normalized };
  }

  async function onPickFile(index = 0): Promise<void> {
    activeSegmentIndex.value = index;
    if (isTauriRuntime()) {
      try {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const { dirname, homeDir } = await import("@tauri-apps/api/path");
        const existingPath = flashSegments.value[index].firmwarePath.trim();
        const defaultPath = existingPath
          ? await dirname(existingPath)
          : await homeDir();
        const selected = await open({
          multiple: false,
          defaultPath,
          filters: [
            {
              name: "Firmware",
              extensions: ["bin", "hex", "elf", "img"],
            },
          ],
        });
        if (selected !== null) {
          flashSegments.value[index].firmwarePath = selected;
          try {
            const { invoke } = await import("@tauri-apps/api/core");
            const size = await invoke<number>("get_file_size", {
              path: selected,
            });
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
    if (getRuntime() === "vscode") {
      const result = await platform.pickFile(
        crypto.randomUUID(),
        ".bin,.hex,.elf,.img",
      );
      if (result) {
        flashSegments.value[index].firmwarePath = result.path;
        flashSegments.value[index].firmwareFile = result.file;
        if (result.file) updateFlashEndAddr(index, result.file.size);
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
    seg.firmwarePath = file ? file.name : "";
    seg.firmwareFile = file ?? null;
    if (file) {
      updateFlashEndAddr(index, file.size);
    }
    input.value = "";
  }

  function addSegment(): void {
    if (flashSegments.value.length >= 10) return;
    const addrs = initialAddrsForNewSegment();
    flashSegments.value.push({
      id: Math.random().toString(36).substring(2, 9),
      firmwarePath: "",
      firmwareFile: null,
      startAddr: addrs.startAddr,
      endAddr: addrs.endAddr,
    });
    appendLog(t("flash.log.segmentAdded", { n: flashSegments.value.length }));
  }

  function removeSegment(index: number): void {
    if (index === 0 || flashSegments.value.length <= 1) return;
    flashSegments.value.splice(index, 1);
    appendLog(t("flash.log.segmentRemoved", { n: index + 1 }));
  }

  async function onPickReadDir(): Promise<void> {
    if (isTauriRuntime()) {
      try {
        const { open } = await import("@tauri-apps/plugin-dialog");
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
    appendLog(t("flash.log.browserReadNoDir"));
  }

  async function cancelBackendFlash(): Promise<void> {
    if (isTauriRuntime()) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("flash_cancel");
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

  /** Trigger authorize in read-only mode regardless of form credentials.
   *  Temporarily clears uuid/authkey → startOperation sees both empty → auth-read path.
   *  This reuses the fully-tested startOperation state machine (connects, logs, Done handler). */
  async function startAuthRead(): Promise<void> {
    const savedUuid = authorizeUuid.value;
    const savedKey = authorizeAuthKey.value;
    authorizeUuid.value = "";
    authorizeAuthKey.value = "";
    authOpIsRead.value = true;
    try {
      await startOperation("authorize");
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
    const { listen } = await import("@tauri-apps/api/event");
    unlistenFlash = await listen<FlashProgressPayload>(
      "flash-progress",
      (ev) => {
        handleFlashProgressPayload(ev.payload);
      },
    );
  }

  const { serialPortOptions, refreshDevice, connect, disconnect, deviceReset } =
    useFlashConnection({
      selectedSerialPort,
      selectedChipId,
      connected,
      autoConnected,
      busy,
      appendLog,
      onCancelRunningOperation: () => {
        stopFlash();
        runningOp.value = null;
        flashPhase.value = "idle";
        flashProgress.value = 0;
        phaseProgress.value = 0;
        currentBackendPhase.value = null;
        phaseIndeterminate.value = false;
        cancelIndeterminateCheck();
        flashMessage.value = "";
        appendLog(t("flash.log.operationCancelled"));
        rLog.info("[Flash] Operation cancelled by user");
      },
    });

  function opTitle(kind: OpKind): string {
    switch (kind) {
      case "flash":
        return t("flash.tabs.flash");
      case "erase":
        return t("flash.tabs.erase");
      case "read":
        return t("flash.tabs.read");
      case "authorize":
        return t("flash.tabs.authorize");
      default:
        return "";
    }
  }

  function buildFlashJob(kind: OpKind): FlashJobPayload {
    const chipId = rustPluginIdForChip(selectedChipId.value);
    return {
      mode: kind,
      chipId,
      port: selectedSerialPort.value,
      baudRate:
        kind === "authorize"
          ? selectedAuthBaudRate.value
          : selectedBaudRate.value,
      flashStartHex:
        kind === "flash" ? formatAddrHex(flashStartAddr.value) : null,
      flashEndHex: kind === "flash" ? formatAddrHex(flashEndAddr.value) : null,
      eraseStartHex:
        kind === "erase" ? formatAddrHex(eraseStartAddr.value) : null,
      eraseEndHex: kind === "erase" ? formatAddrHex(eraseEndAddr.value) : null,
      readStartHex: kind === "read" ? formatAddrHex(readStartAddr.value) : null,
      readEndHex: kind === "read" ? formatAddrHex(readEndAddr.value) : null,
      readFilePath:
        kind === "read" && readFilePath.value.trim()
          ? readFilePath.value.trim()
          : null,
      firmwarePath:
        kind === "flash" && firmwarePath.value.trim()
          ? firmwarePath.value.trim()
          : null,
      segments:
        kind === "flash"
          ? flashSegments.value.map((s) => ({
              firmwarePath: s.firmwarePath,
              startAddr: formatAddrHex(s.startAddr),
              endAddr: formatAddrHex(s.endAddr),
            }))
          : null,
      authorizeUuid:
        kind === "authorize" ? authorizeUuid.value.trim() || null : null,
      authorizeKey:
        kind === "authorize" ? authorizeAuthKey.value.trim() || null : null,
    };
  }

  async function startOperationTauri(kind: OpKind): Promise<void> {
    await ensureFlashListener();
    const { invoke } = await import("@tauri-apps/api/core");
    const job = buildFlashJob(kind);
    await invoke("flash_run", { job });
  }

  async function startOperationWs(kind: OpKind): Promise<void> {
    const job = buildFlashJob(kind);

    // For flash mode, send all File objects; server decodes and uses temp paths
    const filesToUpload =
      kind === "flash"
        ? flashSegments.value.map((s) => s.firmwareFile)
        : [firmwareFile.value];

    // For read mode in web, the server saves to a temp path and returns file_content
    if (kind === "read") {
      job.readFilePath = null; // server uses temp path
    }

    await wsTransport.runJob(job, filesToUpload, (ev: WsProgressEvent) => {
      handleFlashProgressPayload(ev.payload);

      // If server sent a file_content message (read mode), trigger browser download
      if (ev.fileContent) {
        triggerBrowserDownload(
          ev.fileContent.content,
          readFileName.value || ev.fileContent.name,
        );
      }
    });
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
      kind === "authInfo"
        ? "flash.eraseAuthInfo"
        : kind === "fullChipNoRf"
          ? "flash.eraseFullChipNoRf"
          : "flash.eraseFullChip";
    appendLog(
      t("flash.log.erasePresetApplied", {
        chip: selectedChipLabel.value,
        label: t(labelKey),
        start: preset.start,
        end: preset.end,
      }),
    );
  }

  /**
   * Release the serial port when an operation auto-connected (acquired the
   * port) but then bailed out before the backend emitted a terminal event —
   * e.g. the auth-overwrite confirm was cancelled, or `flash_run` threw before
   * `Done`. On those paths `onOperationSettled` never runs. Idempotent (a
   * mismatched/absent owner is a PortManager no-op); a no-op for a non-auto
   * connection. There is no manual disconnect control, so this is what frees
   * the port and re-enables the form selects on these exits.
   */
  function releaseIfAutoConnected(): void {
    if (!autoConnected.value) {
      return;
    }
    connected.value = false;
    autoConnected.value = false;
    usePortManagerStore().release(selectedSerialPort.value, "flash");
  }

  async function startOperation(kind: OpKind): Promise<void> {
    if (flashPhase.value === "running") {
      return;
    }

    // ── 1. Input validation ────────────────────────────────────────
    const vErr = validateOperation(kind, {
      flashSegments: flashSegments.value,
      readDir: readDir.value,
      readFileName: readFileName.value,
      authorizeUuid: authorizeUuid.value,
      authorizeAuthKey: authorizeAuthKey.value,
      selectedSerialPort: selectedSerialPort.value,
      eraseStartAddr: eraseStartAddr.value,
      eraseEndAddr: eraseEndAddr.value,
      readStartAddr: readStartAddr.value,
      readEndAddr: readEndAddr.value,
      isTauri: isTauriRuntime(),
    });
    if (vErr) {
      flashMessage.value = vErr.message;
      flashPhase.value = "error";
      appendLog(vErr.logLine);
      return;
    }

    // ── 2. Erase confirmation dialog ───────────────────────────────
    if (kind === "erase") {
      const start = formatAddrHex(eraseStartAddr.value);
      const end = formatAddrHex(eraseEndAddr.value);
      const chip = selectedChipLabel.value;

      let confirmMsg = t("flash.confirm.eraseBody", { chip, start, end });
      let okLabel = t("flash.confirm.eraseOk");

      if (chipManifest(selectedChipId.value).eraseRequires4KAlignment) {
        const sa = parseHexAddr(eraseStartAddr.value);
        const ea = parseHexAddr(eraseEndAddr.value);
        if (
          sa !== null &&
          ea !== null &&
          exclusiveEraseRangeNeeds4KAlignment(sa, ea)
        ) {
          const { alignedStart, alignedEndExclusive } =
            alignedExclusiveEraseRange4K(sa, ea);
          confirmMsg = t("flash.confirm.eraseBodyMisaligned4k", {
            chip,
            start,
            end,
            sectorHex: "0x1000",
            alignedStart: formatBigIntAddrHex(alignedStart),
            alignedEnd: formatBigIntAddrHex(alignedEndExclusive),
          });
          okLabel = t("flash.confirm.eraseOkAlign");
        }
      }

      const confirmed = await showConfirmDialog({
        title: t("flash.confirm.eraseTitle"),
        message: confirmMsg,
        kind: "warning",
        okLabel,
        cancelLabel: t("flash.confirm.eraseCancel"),
      });

      if (!confirmed) {
        appendLog(t("flash.log.eraseCancelled"));
        return;
      }

      if (chipManifest(selectedChipId.value).eraseRequires4KAlignment) {
        const sa = parseHexAddr(eraseStartAddr.value);
        const ea = parseHexAddr(eraseEndAddr.value);
        if (
          sa !== null &&
          ea !== null &&
          exclusiveEraseRangeNeeds4KAlignment(sa, ea)
        ) {
          const { alignedStart, alignedEndExclusive } =
            alignedExclusiveEraseRange4K(sa, ea);
          eraseStartAddr.value = formatBigIntAddrHex(alignedStart);
          eraseEndAddr.value = formatBigIntAddrHex(alignedEndExclusive);
          appendLog(
            t("flash.log.eraseRangeAligned", {
              fromStart: start,
              fromEnd: end,
              toStart: eraseStartAddr.value,
              toEnd: eraseEndAddr.value,
            }),
          );
        }
      }
    }

    // ── 3. Read file existence check ───────────────────────────────
    if (kind === "read" && isTauriRuntime()) {
      try {
        const fullPath = readFilePath.value;
        const { invoke } = await import("@tauri-apps/api/core");
        const exists = await invoke<boolean>("check_file_exists", {
          path: fullPath,
        });
        if (exists) {
          const overwrite = await showConfirmDialog({
            title: t("flash.confirm.readFileExistsTitle"),
            message: t("flash.confirm.readFileExistsBody", { path: fullPath }),
            kind: "warning",
            okLabel: t("flash.confirm.readOverwrite"),
            cancelLabel: t("flash.confirm.readTimestamp"),
          });
          if (overwrite) {
            appendLog(t("flash.log.readOverwriting", { path: fullPath }));
          } else {
            readFileName.value = addTimestampSuffix(readFileName.value.trim());
            readFileNameModified.value = true;
            appendLog(
              t("flash.log.readTimestamped", { path: readFilePath.value }),
            );
          }
        }
      } catch {
        /* proceed with original path */
      }
    }

    // ── 4. Commit to the operation — gives immediate visual feedback ──
    flashPhase.value = "running";
    operationStartTime = Date.now();
    runningOp.value = kind;
    flashProgress.value = 0;
    phaseProgress.value = 0;
    currentBackendPhase.value = null;
    phaseIndeterminate.value = false;
    cancelIndeterminateCheck();
    flashMessage.value = "";

    // ── 4b. Auto-connect if not manually connected ─────────────────
    if (!connected.value) {
      appendLog(
        t("flash.log.autoConnecting", { port: selectedSerialPort.value }),
      );
      await connect();
      if (!connected.value) {
        // connect() failed (port busy etc.) — error already logged
        flashMessage.value = t("flash.err.portUnavailable");
        flashPhase.value = "error";
        runningOp.value = null;
        return;
      }
      autoConnected.value = true;
    }

    // ── 4b. Authorize write: probe device auth in UI, confirm overwrite if different ──
    if (
      kind === "authorize" &&
      authorizeUuid.value.trim() &&
      authorizeAuthKey.value.trim() &&
      !authOpIsRead.value
    ) {
      const nu = authorizeUuid.value.trim();
      const nk = authorizeAuthKey.value.trim();
      try {
        let existing: { uuid: string; authkey: string } | null = null;
        if (isTauriRuntime()) {
          const { invoke } = await import("@tauri-apps/api/core");
          existing = await invoke<{ uuid: string; authkey: string } | null>(
            "authorize_probe_cmd",
            {
              port: selectedSerialPort.value,
            },
          );
        } else {
          existing = await wsTransport.authorizeProbe(
            selectedSerialPort.value,
            rustPluginIdForChip(selectedChipId.value),
            selectedAuthBaudRate.value,
          );
        }
        if (existing) {
          const eu = existing.uuid.trim();
          const ek = existing.authkey.trim();
          if (
            eu &&
            ek &&
            eu !== AUTHORIZE_PLACEHOLDER_UUID &&
            (eu !== nu || ek !== nk)
          ) {
            const confirmed = await showConfirmDialog({
              title: t("flash.confirm.authOverwriteTitle"),
              message: t("flash.confirm.authOverwriteBody", {
                existingUuid: eu,
                existingAuthkey: ek,
                newUuid: nu,
                newAuthkey: nk,
              }),
              kind: "warning",
              okLabel: t("flash.confirm.authOverwriteOk"),
              cancelLabel: t("flash.confirm.authOverwriteCancel"),
            });
            if (!confirmed) {
              appendLog(t("flash.log.authOverwriteCancelled"));
              releaseIfAutoConnected();
              flashPhase.value = "idle";
              runningOp.value = null;
              return;
            }
          }
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        appendLog(t("flash.log.authProbeSkipped", { msg }));
      }
    }

    // ── 5. Start operation ─────────────────────────────────────────
    rLog.info(
      `[Flash] Starting '${kind}' — chip=${selectedChipId.value}, port=${selectedSerialPort.value}, baud=${selectedBaudRate.value}`,
    );

    const chip = selectedChipLabel.value;
    appendLog(t("flash.log.targetChip", { chip }));
    appendLog(t("flash.log.operation", { op: opTitle(kind) }));

    if (kind === "flash") {
      flashSegments.value.forEach((seg, i) => {
        appendLog(t("flash.log.segmentLog", { n: i + 1 }));
        appendLog(t("flash.log.firmware", { path: seg.firmwarePath }));
        appendLog(
          t("flash.log.flashRangeLog", {
            start: formatAddrHex(seg.startAddr),
            end: formatAddrHex(seg.endAddr),
          }),
        );
      });
      appendLog(t("flash.log.baud", { n: String(selectedBaudRate.value) }));
    } else if (kind === "erase") {
      appendLog(
        t("flash.log.eraseRangeLog", {
          start: formatAddrHex(eraseStartAddr.value),
          end: formatAddrHex(eraseEndAddr.value),
        }),
      );
      appendLog(t("flash.log.erasePrep"));
    } else if (kind === "read") {
      appendLog(
        t("flash.log.readRangeLog", {
          start: formatAddrHex(readStartAddr.value),
          end: formatAddrHex(readEndAddr.value),
        }),
      );
      appendLog(t("flash.log.readSave", { path: readFilePath.value }));
      appendLog(t("flash.log.readPrep"));
    } else if (kind === "authorize") {
      const hasCredentials = !!authorizeUuid.value.trim();
      appendLog(
        t(hasCredentials ? "flash.log.authPrep" : "flash.log.authReadPrep"),
      );
    }

    if (isTauriRuntime()) {
      try {
        await startOperationTauri(kind);
      } catch (e) {
        cancelIndeterminateCheck();
        currentBackendPhase.value = null;
        phaseIndeterminate.value = false;
        runningOp.value = null;
        flashPhase.value = "error";
        const msg = e instanceof Error ? e.message : String(e);
        flashMessage.value = msg;
        appendLog(t("flash.err.withMsg", { msg }));
        logOperationDuration();
        releaseIfAutoConnected();
      }
    } else {
      try {
        await startOperationWs(kind);
      } catch (e) {
        cancelIndeterminateCheck();
        currentBackendPhase.value = null;
        phaseIndeterminate.value = false;
        runningOp.value = null;
        flashPhase.value = "error";
        const msg = e instanceof Error ? e.message : String(e);
        flashMessage.value = msg;
        appendLog(t("flash.err.withMsg", { msg }));
        logOperationDuration();
        releaseIfAutoConnected();
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
    flashPhase.value = "idle";
    flashProgress.value = 0;
    phaseProgress.value = 0;
    currentBackendPhase.value = null;
    phaseIndeterminate.value = false;
    cancelIndeterminateCheck();
    flashMessage.value = "";
  }

  /** Call from component's onUnmounted to release timers and listeners. */
  function cleanup(): void {
    stopFlash();
    cancelIndeterminateCheck();
    phaseIndeterminate.value = false;
    currentBackendPhase.value = null;
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
      flashSegments: flashSegments.value.map((s) => ({
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
      if (data.selectedChipId !== AUTH_ONLY_CHIP_ID) {
        lastFlashChipId.value = data.selectedChipId;
      }
      selectedBaudRate.value = data.selectedBaudRate;
      activeTab.value = data.activeTab;
      flashSegments.value = data.flashSegments.map((s) => ({
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
          const { invoke } = await import("@tauri-apps/api/core");
          const size = await invoke<number>("get_file_size", { path: p });
          updateFlashEndAddr(i, size);
        } catch {
          /* ignore */
        }
      }
    }
    rLog.info("[Flash] Workspace restored from last session");
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
        flashSegments: flashSegments.value.map((s) => ({
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
      { deep: true },
    );
  }

  const canFlash = computed(() => {
    if (busy.value || !selectedSerialPort.value) return false;
    if (selectedChipId.value === AUTH_ONLY_CHIP_ID) return false;
    return flashSegments.value.every((s) => !!s.firmwarePath.trim());
  });

  const canErase = computed(
    () =>
      !busy.value &&
      !!selectedSerialPort.value &&
      selectedChipId.value !== AUTH_ONLY_CHIP_ID,
  );

  const canRead = computed(
    () =>
      !busy.value &&
      (isTauriRuntime() ? !!readDir.value.trim() : true) &&
      !!readFileName.value.trim() &&
      !!selectedSerialPort.value &&
      selectedChipId.value !== AUTH_ONLY_CHIP_ID,
  );

  const canAuthorize = computed(
    () =>
      !busy.value &&
      !!selectedSerialPort.value &&
      !!authorizeUuid.value.trim() &&
      !!authorizeAuthKey.value.trim(),
  );

  /** Read-only auth — only needs a connected port. */
  const canReadAuth = computed(() => !busy.value && !!selectedSerialPort.value);

  const progressCaption = computed(() => {
    if (flashPhase.value !== "running" || !runningOp.value) {
      return t("flash.progress");
    }
    return t("flash.progressWith", { op: opTitle(runningOp.value) });
  });

  const statusText = computed(() =>
    connected.value
      ? t("flash.statusConnected")
      : t("flash.statusDisconnected"),
  );

  const tabList = computed(() => [
    { id: "flash" as const, label: t("flash.tabs.flash") },
    { id: "erase" as const, label: t("flash.tabs.erase") },
    { id: "read" as const, label: t("flash.tabs.read") },
    // UART-only TuyaOpen authorization — same for all chip platforms in the UI
    { id: "authorize" as const, label: t("flash.tabs.authorize") },
  ]);

  return {
    CHIP_IDS,
    AUTH_CHIP_IDS,
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
    currentBackendPhase,
    phaseProgress,
    phaseIndeterminate,
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
