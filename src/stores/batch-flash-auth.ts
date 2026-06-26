import { ref, computed, watch } from "vue";
import { defineStore } from "pinia";
import type { FlashProgressPayload } from "@/features/firmware-flash/flash-ipc-types";
import { chipManifest } from "@/features/firmware-flash/chip-manifests";
import { isTauriRuntime } from "@/runtime";
import {
  BATCH_FLASH_CAPABLE_CHIPS,
  type BatchSlotState,
  type BatchSlotStatus,
  type CumulativeStats,
  type PortFilterConfig,
  type BatchAuthConfigData,
  type BatchFlashProgressEvent,
  type BatchOpMode,
  type CompletionBanner,
  type BatchAuthProgressEvent,
  type BatchAuthStartConfig,
  type AuthFirmwareEntry,
  type BatchFirmwareSource,
} from "@/features/batch-flash-auth/types";
import {
  fetchAuthFirmwareManifest,
  filterByChip,
  downloadAuthFirmware,
} from "@/features/batch-flash-auth/auth-firmware";
import {
  applyPortFilter,
  normalizePortName,
} from "@/features/batch-flash-auth/port-filter";
import {
  loadBatchFlashAuthWorkspace,
  saveBatchFlashAuthCumulative,
  saveBatchFlashAuthFilterConfig,
  saveBatchFlashAuthFirmwareConfig,
  saveBatchFlashAuthConfig,
} from "@/stores/batch-flash-auth-workspace";

const ACTIVE_STATUSES: BatchSlotStatus[] = [
  "flashing",
  "reading_mac",
  "authorizing",
];

export const useBatchFlashAuthStore = defineStore("batch-flash-auth", () => {
  // ── Persisted config ──────────────────────────────────────────────────────
  const filterConfig = ref<PortFilterConfig>({ blockedPorts: [] });
  const cumulativeStats = ref<CumulativeStats>({
    flash: { total: 0, success: 0, fail: 0 },
    auth: { total: 0, success: 0, fail: 0 },
  });

  // ── Session state ─────────────────────────────────────────────────────────
  const slots = ref<BatchSlotState[]>([]);
  const chipId = ref<string>("esp32");
  const baudRate = ref<number>(115200);
  const authBaudRate = ref<number>(115200);
  const firmwarePath = ref<string>("");
  const firmwareSource = ref<BatchFirmwareSource>("local");
  const selectedDefaultVersion = ref<string>("");
  const defaultFirmwareEntries = ref<AuthFirmwareEntry[]>([]);
  const defaultFirmwareStatus = ref<
    "idle" | "loading" | "downloading" | "ready" | "error"
  >("idle");
  const defaultFirmwareError = ref<string>("");
  const authConfig = ref<BatchAuthConfigData>({
    excelPath: "",
    conflictPolicy: "skip",
  });
  const batchStartTime = ref<number | null>(null);
  const completionBanner = ref<CompletionBanner | null>(null);
  const currentBatchPorts = ref<string[]>([]);
  const firmwareDownloadProgress = ref<number | null>(null);

  let unlisten: (() => void) | undefined;
  let unlistenAuth: (() => void) | undefined;
  let unlistenFwProgress: (() => void) | undefined;
  let _saveStatsTimer: ReturnType<typeof setTimeout> | undefined;

  // ── Computed ──────────────────────────────────────────────────────────────
  const canFlash = computed(() =>
    (BATCH_FLASH_CAPABLE_CHIPS as readonly string[]).includes(chipId.value),
  );

  const opMode = computed<BatchOpMode>(() =>
    canFlash.value && !!firmwarePath.value ? "flash-then-auth" : "auth-only",
  );

  const currentStats = computed(() => ({
    active: slots.value.filter((s) => ACTIVE_STATUSES.includes(s.status))
      .length,
    done: slots.value.filter((s) => s.status === "done").length,
    failed: slots.value.filter((s) => s.status === "failed").length,
    skipped: slots.value.filter((s) => s.status === "skipped").length,
  }));

  const inputsValid = computed(() => !!authConfig.value.excelPath);

  const isBusy = computed(() => currentStats.value.active > 0);
  const canStart = computed(
    () => slots.value.some((s) => s.status === "idle") && inputsValid.value,
  );
  const canCancel = computed(() => isBusy.value);
  const canRetry = computed(() =>
    slots.value.some((s) => s.status === "failed"),
  );
  const filterActive = computed(
    () => filterConfig.value.blockedPorts.length > 0,
  );

  // ── Slot helpers ──────────────────────────────────────────────────────────
  function findSlot(port: string): BatchSlotState | undefined {
    return slots.value.find((s) => s.port === port);
  }

  function updateSlot(port: string, patch: Partial<BatchSlotState>) {
    const slot = findSlot(port);
    if (slot) Object.assign(slot, patch);
  }

  // ── Port management ───────────────────────────────────────────────────────
  function addPorts(ports: string[]) {
    const existing = new Set(slots.value.map((s) => s.port));
    for (const port of ports) {
      if (!existing.has(port)) {
        const entry: BatchSlotState = {
          port,
          status: "idle",
          progress: 0,
          currentPhase: "",
        };
        slots.value.push(entry);
        existing.add(port);
      }
    }
  }

  function removeSlot(port: string) {
    const slot = findSlot(port);
    if (!slot) return;
    if (
      slot.status === "idle" ||
      slot.status === "done" ||
      slot.status === "skipped"
    ) {
      slots.value = slots.value.filter((s) => s.port !== port);
    }
  }

  async function autoAssign() {
    if (!isTauriRuntime()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    const all: Array<{ path: string }> = await invoke("list_serial_ports_cmd");
    const filtered = applyPortFilter(
      all.map((p) => p.path),
      filterConfig.value.blockedPorts,
    );
    addPorts(filtered);
  }

  // ── Default-firmware download ─────────────────────────────────────────────
  async function saveFirmwareConfig() {
    await saveBatchFlashAuthFirmwareConfig({
      source: firmwareSource.value,
      version: selectedDefaultVersion.value,
    });
  }

  function setFirmwareSource(source: BatchFirmwareSource) {
    if (firmwareSource.value === source) return;
    firmwareSource.value = source;
    // Switching source invalidates the previously chosen firmware path.
    firmwarePath.value = "";
    if (source === "local") {
      defaultFirmwareStatus.value = "idle";
      defaultFirmwareError.value = "";
    }
    void saveFirmwareConfig();
  }

  async function downloadDefaultFirmware(version: string) {
    if (!isTauriRuntime()) return;
    const entry = defaultFirmwareEntries.value.find(
      (e) => e.version === version,
    );
    if (!entry) return;
    selectedDefaultVersion.value = version;
    void saveFirmwareConfig();
    defaultFirmwareStatus.value = "downloading";
    defaultFirmwareError.value = "";
    firmwareDownloadProgress.value = 0;
    try {
      firmwarePath.value = await downloadAuthFirmware(entry);
      defaultFirmwareStatus.value = "ready";
    } catch (e) {
      firmwarePath.value = "";
      defaultFirmwareStatus.value = "error";
      defaultFirmwareError.value = e instanceof Error ? e.message : String(e);
    } finally {
      firmwareDownloadProgress.value = null;
    }
  }

  async function loadDefaultFirmwareList() {
    if (!isTauriRuntime()) return;
    defaultFirmwareStatus.value = "loading";
    defaultFirmwareError.value = "";
    try {
      const { manifest } = await fetchAuthFirmwareManifest();
      defaultFirmwareEntries.value = filterByChip(
        manifest.firmwares,
        chipId.value,
      );
      defaultFirmwareStatus.value = "idle";
      // Restore the previously selected version's path if still available.
      if (
        selectedDefaultVersion.value &&
        defaultFirmwareEntries.value.some(
          (e) => e.version === selectedDefaultVersion.value,
        )
      ) {
        await downloadDefaultFirmware(selectedDefaultVersion.value);
      }
    } catch (e) {
      defaultFirmwareStatus.value = "error";
      defaultFirmwareError.value = e instanceof Error ? e.message : String(e);
      defaultFirmwareEntries.value = [];
    }
  }

  // ── Flash actions ─────────────────────────────────────────────────────────
  async function startAuth() {
    if (!canStart.value) return;
    if (!isTauriRuntime()) return;
    batchStartTime.value = Date.now();
    completionBanner.value = null;

    const idlePorts = slots.value
      .filter((s) => s.status === "idle")
      .map((s) => s.port);
    currentBatchPorts.value = idlePorts;
    for (const port of idlePorts) {
      updateSlot(port, {
        status: "reading_mac",
        progress: 0,
        currentPhase: "reading_mac",
        error: undefined,
        excelError: undefined,
      });
    }

    const { invoke } = await import("@tauri-apps/api/core");
    const fw = firmwarePath.value || undefined;
    let flashStartHex: string | undefined;
    let flashEndHex: string | undefined;
    if (fw) {
      const m = chipManifest(chipId.value);
      const preset = m.erasePresets.fullChipNoRf ?? m.erasePresets.fullChip;
      flashStartHex = preset?.start ?? "0x00000000";
      flashEndHex = preset?.end ?? m.flashSize;
    }
    const config: BatchAuthStartConfig = {
      chipId: chipId.value,
      baudRate: baudRate.value,
      authBaudRate: authBaudRate.value,
      firmwarePath: fw,
      flashStartHex,
      flashEndHex,
      excelPath: authConfig.value.excelPath,
      conflictPolicy: authConfig.value.conflictPolicy,
    };
    await invoke("batch_auth_start", { config, ports: idlePorts });
  }

  async function startBatch() {
    await startAuth();
  }

  async function retryFailed() {
    if (!canRetry.value) return;
    completionBanner.value = null;
    for (const slot of slots.value.filter((s) => s.status === "failed")) {
      updateSlot(slot.port, {
        status: "idle",
        progress: 0,
        currentPhase: "",
        error: undefined,
        excelError: undefined,
      });
    }
    await startBatch();
  }

  async function cancelPort(port: string) {
    if (!isTauriRuntime()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("batch_auth_cancel_port", { port });
  }

  async function cancelAll() {
    if (!isTauriRuntime()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("batch_auth_cancel_all");
  }

  // ── Progress event handler ────────────────────────────────────────────────

  function normalizePhase(phase: unknown): string {
    if (typeof phase === "string") return phase;
    if (phase !== null && typeof phase === "object") {
      if ("write_segment" in (phase as object)) return "write_segment";
      if ("other" in (phase as object))
        return (phase as { other: string }).other;
      const key = Object.keys(phase as object)[0];
      if (key) return key;
    }
    return "unknown";
  }

  function handleFlashProgress(ev: BatchFlashProgressEvent) {
    const { port, event: e } = ev;
    if (!findSlot(port)) return;

    if (e.kind === "percent") {
      updateSlot(port, { progress: e.value });
    } else if (e.kind === "phase") {
      updateSlot(port, { currentPhase: normalizePhase(e.phase) });
    } else if (e.kind === "done") {
      const r = e.result;
      if ("ok" in r) {
        updateSlot(port, { status: "done", progress: 100, currentPhase: "" });
        cumulativeStats.value.flash.total++;
        cumulativeStats.value.flash.success++;
      } else if ("err" in r) {
        updateSlot(port, { status: "failed", error: r.err.message });
        cumulativeStats.value.flash.total++;
        cumulativeStats.value.flash.fail++;
      } else {
        // cancelled — not counted in cumulative
        updateSlot(port, { status: "idle", progress: 0, currentPhase: "" });
      }
      scheduleSaveStats();
      checkBatchCompletion();
    }
  }

  function handleAuthProgress(ev: BatchAuthProgressEvent) {
    const { port, step } = ev;
    if (!findSlot(port)) return;

    if (step === "reading_mac") {
      updateSlot(port, { status: "reading_mac", currentPhase: "reading_mac" });
    } else if (
      step === "reading_auth" ||
      step === "writing_auth" ||
      step === "verifying"
    ) {
      updateSlot(port, { status: "authorizing", currentPhase: step });
    } else if (step === "done") {
      updateSlot(port, {
        status: "done",
        progress: 100,
        currentPhase: "",
        mac: ev.mac,
        excelError: ev.excelError,
      });
      cumulativeStats.value.auth.total++;
      cumulativeStats.value.auth.success++;
      scheduleSaveStats();
      checkBatchCompletion();
    } else if (step === "failed") {
      updateSlot(port, {
        status: "failed",
        error: ev.error ?? "Unknown auth error",
      });
      cumulativeStats.value.auth.total++;
      cumulativeStats.value.auth.fail++;
      scheduleSaveStats();
      checkBatchCompletion();
    } else if (step === "skipped") {
      updateSlot(port, {
        status: "skipped",
        currentPhase: "",
        mac: ev.mac,
        excelError: ev.excelError,
      });
      checkBatchCompletion();
    } else if (step === "cancelled") {
      // Rust cancelled the slot (e.g. user hit cancel mid-flight).
      // Reset to idle so the slot can be retried without remove+re-add.
      updateSlot(port, {
        status: "idle",
        progress: 0,
        currentPhase: "",
        error: undefined,
        excelError: undefined,
      });
      checkBatchCompletion();
    } else if (step === "flashing" && ev.event) {
      const e = ev.event as FlashProgressPayload;
      if (e.kind === "percent") {
        updateSlot(port, { progress: e.value });
      } else if (e.kind === "phase") {
        updateSlot(port, {
          status: "flashing",
          currentPhase: normalizePhase(e.phase),
        });
      } else if (e.kind === "done" && "ok" in e.result) {
        // Flash sub-step completed; transition to auth phase while waiting for auth events.
        updateSlot(port, {
          status: "reading_mac",
          progress: 0,
          currentPhase: "reading_mac",
        });
      }
      // err/cancelled: the subsequent auth 'failed'/'skipped' step handles final state.
    }
  }

  function checkBatchCompletion() {
    const anyActive = slots.value.some((s) =>
      ACTIVE_STATUSES.includes(s.status),
    );
    if (anyActive || batchStartTime.value === null) return;

    // Count only slots from the current run to avoid prior-run done slots skewing
    // the banner. currentBatchPorts is set by startAuth; when empty
    // (e.g. direct test calls), fall back to all slots.
    const batchPortSet = new Set(currentBatchPorts.value);
    const batchSlots =
      batchPortSet.size > 0
        ? slots.value.filter((s) => batchPortSet.has(s.port))
        : slots.value;
    const done = batchSlots.filter((s) => s.status === "done").length;
    const failed = batchSlots.filter((s) => s.status === "failed").length;
    const skipped = batchSlots.filter((s) => s.status === "skipped").length;

    if (done === 0 && failed === 0 && skipped > 0) {
      completionBanner.value = { kind: "all-skipped", count: skipped };
    } else if (failed === 0) {
      completionBanner.value = { kind: "all-success", count: done };
    } else if (done === 0) {
      completionBanner.value = { kind: "all-failed" };
    } else {
      completionBanner.value = { kind: "partial", done, failed };
    }
    flushSaveStats();
  }

  function dismissBanner() {
    completionBanner.value = null;
  }

  // ── Port filter ───────────────────────────────────────────────────────────
  function addBlockedPort(port: string) {
    const normalized = normalizePortName(port);
    if (!filterConfig.value.blockedPorts.includes(normalized)) {
      filterConfig.value.blockedPorts.push(normalized);
    }
    slots.value = slots.value.filter(
      (s) =>
        s.status !== "idle" ||
        !filterConfig.value.blockedPorts.includes(normalizePortName(s.port)),
    );
    void saveFilterConfig();
  }

  function removeBlockedPort(port: string) {
    const normalized = normalizePortName(port);
    filterConfig.value.blockedPorts = filterConfig.value.blockedPorts.filter(
      (p) => p !== normalized,
    );
    void saveFilterConfig();
  }

  // ── Cumulative stats reset ────────────────────────────────────────────────
  function resetFlashStats() {
    cumulativeStats.value.flash = { total: 0, success: 0, fail: 0 };
    flushSaveStats();
  }

  function resetAuthStats() {
    cumulativeStats.value.auth = { total: 0, success: 0, fail: 0 };
    flushSaveStats();
  }

  // ── Debounced cumulative stats save ──────────────────────────────────────
  function scheduleSaveStats() {
    clearTimeout(_saveStatsTimer);
    _saveStatsTimer = setTimeout(() => {
      _saveStatsTimer = undefined;
      void saveCumulativeStats();
    }, 1000);
  }

  function flushSaveStats() {
    clearTimeout(_saveStatsTimer);
    _saveStatsTimer = undefined;
    void saveCumulativeStats();
  }

  // ── Persistence ───────────────────────────────────────────────────────────
  async function loadPersistedData() {
    const {
      cumulative,
      filter,
      firmware,
      authConfig: savedAuthConfig,
    } = await loadBatchFlashAuthWorkspace();
    if (cumulative) cumulativeStats.value = cumulative;
    if (filter) filterConfig.value = filter;
    if (savedAuthConfig) authConfig.value = savedAuthConfig;
    if (firmware) {
      firmwareSource.value = firmware.source;
      selectedDefaultVersion.value = firmware.version;
      if (firmware.source === "default") {
        await loadDefaultFirmwareList();
      }
    }
  }

  async function saveCumulativeStats() {
    await saveBatchFlashAuthCumulative(cumulativeStats.value);
  }

  async function saveFilterConfig() {
    await saveBatchFlashAuthFilterConfig(filterConfig.value);
  }

  async function saveAuthConfig() {
    await saveBatchFlashAuthConfig(authConfig.value);
  }

  // ── Event listener lifecycle ──────────────────────────────────────────────
  async function ensureListener() {
    if (!isTauriRuntime()) return;
    const { listen } = await import("@tauri-apps/api/event");
    if (!unlisten) {
      unlisten = await listen<BatchFlashProgressEvent>(
        "batch-flash-progress",
        ({ payload }) => {
          handleFlashProgress(payload);
        },
      );
    }
    if (!unlistenAuth) {
      unlistenAuth = await listen<BatchAuthProgressEvent>(
        "batch-auth-progress",
        ({ payload }) => {
          handleAuthProgress(payload);
        },
      );
    }
    if (!unlistenFwProgress) {
      unlistenFwProgress = await listen<{
        bytesDone: number;
        bytesTotal: number;
      }>("auth-firmware-download-progress", ({ payload }) => {
        if (payload.bytesTotal > 0) {
          firmwareDownloadProgress.value = Math.round(
            (payload.bytesDone / payload.bytesTotal) * 100,
          );
        }
      });
    }
  }

  function cleanup() {
    clearTimeout(_saveStatsTimer);
    _saveStatsTimer = undefined;
    unlisten?.();
    unlisten = undefined;
    unlistenAuth?.();
    unlistenAuth = undefined;
    unlistenFwProgress?.();
    unlistenFwProgress = undefined;
  }

  watch(authConfig, () => void saveAuthConfig(), { deep: true });

  return {
    // State
    slots,
    chipId,
    baudRate,
    authBaudRate,
    firmwarePath,
    authConfig,
    filterConfig,
    cumulativeStats,
    completionBanner,
    firmwareSource,
    selectedDefaultVersion,
    defaultFirmwareEntries,
    defaultFirmwareStatus,
    defaultFirmwareError,
    firmwareDownloadProgress,
    // Computed
    canFlash,
    opMode,
    currentStats,
    inputsValid,
    isBusy,
    canStart,
    canCancel,
    canRetry,
    filterActive,
    batchStartTime,
    // Actions
    addPorts,
    removeSlot,
    autoAssign,
    setFirmwareSource,
    loadDefaultFirmwareList,
    downloadDefaultFirmware,
    startAuth,
    startBatch,
    retryFailed,
    cancelPort,
    cancelAll,
    addBlockedPort,
    removeBlockedPort,
    resetFlashStats,
    resetAuthStats,
    dismissBanner,
    loadPersistedData,
    ensureListener,
    cleanup,
    // Internal (exposed for testing)
    handleFlashProgress,
    handleAuthProgress,
    checkBatchCompletion,
    currentBatchPorts,
  };
});
