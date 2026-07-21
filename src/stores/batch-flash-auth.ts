import { ref, computed, watch } from "vue";
import { defineStore } from "pinia";
import type { FlashProgressPayload } from "@/features/firmware-flash/flash-ipc-types";
import { chipManifest } from "@/features/firmware-flash/chip-manifests";
import { isTauriRuntime } from "@/runtime";
import { i18n } from "@/i18n";
import { rLog } from "@/utils/log";
import {
  BATCH_FLASH_CAPABLE_CHIPS,
  OTP_CAPABLE_CHIPS,
  EXCEL_ERROR_CODES,
  type BatchSlotState,
  type BatchSlotStatus,
  type CumulativeStats,
  type PortFilterConfig,
  type BatchAuthConfigData,
  type BatchFlashProgressEvent,
  type BatchOpMode,
  type CompletionBanner,
  type BatchAuthProgressEvent,
  type BatchAuthReadProgressEvent,
  type BatchAuthStartConfig,
  type AuthFirmwareEntry,
  type BatchFirmwareSource,
  type ExcelStats,
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
  buildArchiveFolderName,
  buildBatchArchiveSummary,
  buildSlotsCsv,
} from "@/features/batch-flash-auth/archive";
import {
  loadBatchFlashAuthWorkspace,
  saveBatchFlashAuthCumulative,
  saveBatchFlashAuthFilterConfig,
  saveBatchFlashAuthFirmwareConfig,
  saveBatchFlashAuthConfig,
  saveBatchFlashAuthSharedConfig,
} from "@/stores/batch-flash-auth-workspace";

const ACTIVE_STATUSES: BatchSlotStatus[] = [
  "reading",
  "flashing",
  "reading_mac",
  "authorizing",
];

export const useBatchFlashAuthStore = defineStore("batch-flash-auth", () => {
  const t = i18n.global.t;

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
  const localFirmwarePath = ref<string>("");
  const flashFirmware = ref<boolean>(true);
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
    authStorage: "kv",
  });
  const batchStartTime = ref<number | null>(null);
  const batchEndTime = ref<number | null>(null);
  const completionBanner = ref<CompletionBanner | null>(null);
  const currentBatchPorts = ref<string[]>([]);
  const firmwareDownloadProgress = ref<number | null>(null);
  const excelStats = ref<ExcelStats | null>(null);
  const excelError = ref<string | null>(null);
  const archiveStatus = ref<"idle" | "archiving" | "done" | "error">("idle");
  const archiveError = ref<string>("");
  const lastArchivePath = ref<string>("");

  let unlisten: (() => void) | undefined;
  let unlistenAuth: (() => void) | undefined;
  let unlistenFwProgress: (() => void) | undefined;
  let unlistenRead: (() => void) | undefined;
  let _saveStatsTimer: ReturnType<typeof setTimeout> | undefined;

  // ── Computed ──────────────────────────────────────────────────────────────
  const canFlash = computed(() =>
    (BATCH_FLASH_CAPABLE_CHIPS as readonly string[]).includes(chipId.value),
  );

  /** Whether the current chip's firmware supports OTP (write-once) storage. */
  const isOtpCapable = computed(() =>
    (OTP_CAPABLE_CHIPS as readonly string[]).includes(chipId.value),
  );

  const opMode = computed<BatchOpMode>(() =>
    canFlash.value && flashFirmware.value && !!firmwarePath.value
      ? "flash-then-auth"
      : "auth-only",
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
    () =>
      !isBusy.value &&
      slots.value.length > 0 &&
      inputsValid.value &&
      !excelError.value &&
      excelStats.value !== null &&
      // Only NEW devices need an Available row ("remaining"). Devices already
      // recorded in the sheet (in-progress or used rows) are matched by MAC
      // and reuse their own row — recovery after KV loss or a retry works
      // with remaining = 0, and unrecorded devices surface per-slot as
      // no_code. Block start only when the sheet offers neither codes to
      // allocate nor history to recover.
      excelStats.value.remaining +
        excelStats.value.inProgress +
        excelStats.value.used >
        0,
  );
  const canCancel = computed(() => isBusy.value);
  const canRetry = computed(() =>
    slots.value.some(
      (s) =>
        (s.status === "failed" && !s.cancelledAfterWrite) ||
        s.status === "no_code",
    ),
  );
  const canReadAll = computed(() => !isBusy.value && slots.value.length > 0);
  /** Archiving needs a finished batch (results to record) and the Excel path
   *  (the sheet copy is the core of the archive). */
  const canArchive = computed(
    () =>
      !isBusy.value &&
      batchEndTime.value !== null &&
      slots.value.length > 0 &&
      !!authConfig.value.excelPath,
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

  function blockPort(port: string) {
    addBlockedPort(port);
    const slot = findSlot(port);
    if (slot && !ACTIVE_STATUSES.includes(slot.status)) {
      slots.value = slots.value.filter((s) => s.port !== port);
    }
  }

  async function autoAssign() {
    if (!isTauriRuntime()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    const all: Array<{ path: string }> = await invoke("list_serial_ports_cmd");
    // Windows reassigns COM numbers when an adapter is re-plugged; drop
    // non-active slots whose port no longer exists so stale numbers don't
    // accumulate alongside the new ones.
    const present = new Set(all.map((p) => p.path));
    slots.value = slots.value.filter(
      (s) => ACTIVE_STATUSES.includes(s.status) || present.has(s.port),
    );
    const filtered = applyPortFilter(
      all.map((p) => p.path),
      filterConfig.value.blockedPorts,
    );
    addPorts(filtered);
  }

  async function validateExcel(path: string) {
    if (!path || !isTauriRuntime()) {
      excelStats.value = null;
      excelError.value = null;
      return;
    }
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      excelStats.value = await invoke<ExcelStats>("validate_excel_cmd", {
        path,
      });
      excelError.value = null;
      if (excelStats.value) {
        rLog.info(
          `[batch-auth] excel selected: path=${path} total=${excelStats.value.total} used=${excelStats.value.used} remaining=${excelStats.value.remaining}`,
        );
      }
    } catch (e) {
      excelStats.value = null;
      const raw = String(e);
      excelError.value = EXCEL_ERROR_CODES[raw] ?? raw;
    }
  }

  watch(() => authConfig.value.excelPath, validateExcel, { immediate: true });

  // ── Default-firmware download ─────────────────────────────────────────────
  async function saveFirmwareConfig() {
    await saveBatchFlashAuthFirmwareConfig({
      source: firmwareSource.value,
      version: selectedDefaultVersion.value,
      localPath: localFirmwarePath.value,
    });
  }

  function setLocalFirmwarePath(path: string) {
    firmwarePath.value = path;
    localFirmwarePath.value = path;
    rLog.info(`[batch-auth] firmware selected: source=local path=${path}`);
    void saveFirmwareConfig();
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
    const chipAtStart = chipId.value;
    defaultFirmwareStatus.value = "downloading";
    defaultFirmwareError.value = "";
    firmwareDownloadProgress.value = 0;
    try {
      const path = await downloadAuthFirmware(entry);
      // Chip switched mid-download: the binary belongs to the old chip.
      if (chipId.value !== chipAtStart) return;
      firmwarePath.value = path;
      rLog.info(
        `[batch-auth] firmware ready: source=default version=${version} path=${firmwarePath.value}`,
      );
      defaultFirmwareStatus.value = "ready";
    } catch (e) {
      if (chipId.value !== chipAtStart) return;
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

  /** batch_auth_start rejected before any slot ran (e.g. Excel file locked):
   *  fail the just-started ports so nothing hangs in reading_mac. */
  function failPortsOnStartError(ports: string[], e: unknown) {
    const raw = String(e);
    const msg = EXCEL_ERROR_CODES[raw] ? t(EXCEL_ERROR_CODES[raw]) : raw;
    rLog.error(`[batch-auth] batch_auth_start rejected: ${raw}`);
    for (const port of ports) {
      updateSlot(port, { status: "failed", error: msg, currentPhase: "" });
    }
    checkBatchCompletion();
  }

  async function startAuth() {
    if (!canStart.value) return;
    if (!isTauriRuntime()) return;
    batchStartTime.value = Date.now();
    batchEndTime.value = null;
    completionBanner.value = null;
    archiveStatus.value = "idle";
    archiveError.value = "";
    lastArchivePath.value = "";

    for (const slot of slots.value.filter(
      (s) => !ACTIVE_STATUSES.includes(s.status),
    )) {
      updateSlot(slot.port, {
        status: "idle",
        progress: 0,
        currentPhase: "",
        error: undefined,
        excelError: undefined,
      });
    }

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
    const fw =
      canFlash.value && flashFirmware.value
        ? firmwarePath.value || undefined
        : undefined;
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
      authStorage: authConfig.value.authStorage,
    };
    try {
      await invoke("batch_auth_start", { config, ports: idlePorts });
    } catch (e) {
      failPortsOnStartError(idlePorts, e);
    }
  }

  async function startBatch() {
    await startAuth();
  }

  async function retryFailed() {
    if (!canRetry.value) return;
    completionBanner.value = null;
    for (const slot of slots.value.filter(
      (s) =>
        (s.status === "failed" && !s.cancelledAfterWrite) ||
        s.status === "no_code",
    )) {
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

  async function retryPort(port: string) {
    if (!isTauriRuntime()) return;
    const slot = findSlot(port);
    if (!slot) return;
    if (slot.status !== "failed" && slot.status !== "no_code") return;
    if (slot.cancelledAfterWrite) return;
    updateSlot(port, {
      status: "idle",
      progress: 0,
      currentPhase: "",
      error: undefined,
      excelError: undefined,
    });
    batchEndTime.value = null;
    const { invoke } = await import("@tauri-apps/api/core");
    const fw =
      canFlash.value && flashFirmware.value
        ? firmwarePath.value || undefined
        : undefined;
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
      authStorage: authConfig.value.authStorage,
    };
    try {
      await invoke("batch_auth_start", { config, ports: [port] });
    } catch (e) {
      failPortsOnStartError([port], e);
    }
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

  async function readPort(port: string) {
    if (!isTauriRuntime()) return;
    const slot = findSlot(port);
    if (!slot || ACTIVE_STATUSES.includes(slot.status)) return;
    updateSlot(port, { status: "reading", readError: undefined });
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("batch_auth_read_ports", {
      config: {
        chipId: chipId.value,
        baudRate: authBaudRate.value,
        authStorage: authConfig.value.authStorage,
      },
      ports: [port],
    });
  }

  async function readAll() {
    if (!isTauriRuntime()) return;
    const readable = slots.value.filter(
      (s) => !ACTIVE_STATUSES.includes(s.status),
    );
    if (!readable.length) return;
    for (const s of readable) {
      updateSlot(s.port, { status: "reading", readError: undefined });
    }
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("batch_auth_read_ports", {
      config: {
        chipId: chipId.value,
        baudRate: authBaudRate.value,
        authStorage: authConfig.value.authStorage,
      },
      ports: readable.map((s) => s.port),
    });
  }

  // ── Archive ───────────────────────────────────────────────────────────────

  /** Export the finished batch into an archive folder: a copy of the auth
   *  Excel sheet, the firmware binary (when one was flashed), a logs.zip,
   *  batch-summary.json and batch-slots.csv. The user picks the parent
   *  directory; one timestamped subfolder is created per run. */
  async function archiveBatch() {
    if (!canArchive.value || !isTauriRuntime()) return;
    if (archiveStatus.value === "archiving") return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string" || !dir) return;
    archiveStatus.value = "archiving";
    archiveError.value = "";
    try {
      const now = new Date();
      const folderName = buildArchiveFolderName(chipId.value, now);
      const summary = buildBatchArchiveSummary(
        {
          chipId: chipId.value,
          opMode: opMode.value,
          baudRate: baudRate.value,
          authBaudRate: authBaudRate.value,
          firmwareSource: firmwareSource.value,
          firmwareVersion:
            firmwareSource.value === "default"
              ? selectedDefaultVersion.value
              : "",
          firmwarePath: firmwarePath.value,
          excelPath: authConfig.value.excelPath,
          conflictPolicy: authConfig.value.conflictPolicy,
          authStorage: authConfig.value.authStorage,
          excelStats: excelStats.value,
          completionBanner: completionBanner.value,
          batchStartTime: batchStartTime.value,
          batchEndTime: batchEndTime.value,
          currentBatchPorts: currentBatchPorts.value,
          slots: slots.value,
          cumulativeStats: cumulativeStats.value,
          blockedPorts: filterConfig.value.blockedPorts,
        },
        now,
      );
      const slotsCsv = buildSlotsCsv(slots.value, currentBatchPorts.value);
      const fw =
        opMode.value === "flash-then-auth" ? firmwarePath.value : undefined;
      const { invoke } = await import("@tauri-apps/api/core");
      const path = await invoke<string>("archive_batch_cmd", {
        destDir: dir,
        folderName,
        excelPath: authConfig.value.excelPath,
        firmwarePath: fw,
        summaryJson: JSON.stringify(summary),
        slotsCsv,
      });
      lastArchivePath.value = path;
      archiveStatus.value = "done";
      rLog.info(`[batch-auth] batch archived to ${path}`);
    } catch (e) {
      archiveError.value = e instanceof Error ? e.message : String(e);
      archiveStatus.value = "error";
      rLog.error(`[batch-auth] archive failed: ${archiveError.value}`);
    }
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
        updateSlot(port, {
          status: "done",
          progress: 100,
          currentPhase: "",
          // A fresh successful flash supersedes any stale "read failed" flag
          // left by a pre-batch read probe (e.g. device was in download mode).
          readError: undefined,
        });
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
        authUuid: ev.uuid,
        // The device is authorized by definition now; overwrite any stale
        // "not authorized" flag left by a pre-batch read probe (the done
        // event carries no uuid, so the probe flag would otherwise stick).
        isAuthorized: true,
        readError: undefined,
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
        excelError: ev.excelError,
        mac: ev.mac,
      });
      cumulativeStats.value.auth.total++;
      cumulativeStats.value.auth.fail++;
      scheduleSaveStats();
      checkBatchCompletion();
    } else if (step === "no_code") {
      updateSlot(port, {
        status: "no_code",
        currentPhase: "",
        mac: ev.mac,
        error: undefined,
        excelError: undefined,
        readError: undefined,
      });
      checkBatchCompletion();
    } else if (step === "skipped") {
      updateSlot(port, {
        status: "skipped",
        currentPhase: "",
        mac: ev.mac,
        // Skipped means the device already carries auth; the event reports
        // it as existingUuid (there is no ev.uuid for this step).
        authUuid: ev.uuid ?? ev.existingUuid,
        isAuthorized: true,
        readError: undefined,
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
    } else if (step === "cancelled_after_write") {
      // auth_write was sent but cancel arrived before verify. The credential
      // may already be on the device (KV overwritable, OTP permanent).
      // Surface as failed with quarantine flag so operator can isolate device.
      updateSlot(port, {
        status: "failed",
        error: "Cancelled after auth write — device may carry credential",
        excelError: ev.excelError,
        mac: ev.mac,
        cancelledAfterWrite: true,
        authUuid: ev.uuid,
      });
      cumulativeStats.value.auth.total++;
      cumulativeStats.value.auth.fail++;
      scheduleSaveStats();
      checkBatchCompletion();
    } else if (step === "default_mac") {
      updateSlot(port, {
        status: "failed",
        currentPhase: "",
        mac: ev.mac,
        error: t("batchFlashAuth.defaultMacError"),
      });
      cumulativeStats.value.auth.total++;
      cumulativeStats.value.auth.fail++;
      scheduleSaveStats();
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

  function handleReadProgress(ev: BatchAuthReadProgressEvent) {
    const { port, step } = ev;
    if (!findSlot(port)) return;
    if (step === "done") {
      updateSlot(port, {
        status: "idle",
        mac: ev.mac,
        authUuid: ev.uuid,
        isAuthorized: typeof ev.uuid === "string",
        readError: undefined,
      });
    } else if (step === "failed") {
      updateSlot(port, {
        status: "idle",
        mac: undefined,
        authUuid: undefined,
        isAuthorized: undefined,
        readError: ev.error,
      });
    } else if (step === "cancelled") {
      updateSlot(port, { status: "idle" });
    }
  }

  function checkBatchCompletion() {
    const anyActive = slots.value.some((s) =>
      ACTIVE_STATUSES.includes(s.status),
    );
    if (
      anyActive ||
      batchStartTime.value === null ||
      batchEndTime.value !== null
    )
      return;

    // Count only slots from the current run to avoid prior-run done slots skewing
    // the banner. currentBatchPorts is set by startAuth; when empty
    // (e.g. direct test calls), fall back to all slots.
    const batchPortSet = new Set(currentBatchPorts.value);
    const batchSlots =
      batchPortSet.size > 0
        ? slots.value.filter((s) => batchPortSet.has(s.port))
        : slots.value;
    const done = batchSlots.filter((s) => s.status === "done").length;
    const failed = batchSlots.filter(
      (s) => s.status === "failed" || s.status === "no_code",
    ).length;
    const skipped = batchSlots.filter((s) => s.status === "skipped").length;

    batchEndTime.value = Date.now();
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
    void validateExcel(authConfig.value.excelPath);
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
      sharedConfig,
    } = await loadBatchFlashAuthWorkspace();
    if (cumulative) cumulativeStats.value = cumulative;
    if (filter) filterConfig.value = filter;
    if (savedAuthConfig) {
      // Pick known fields explicitly: legacy sessions may have persisted a
      // now-removed `lockOtpAfterAuth` key that must not leak back in.
      authConfig.value = {
        excelPath: savedAuthConfig.excelPath ?? "",
        conflictPolicy: savedAuthConfig.conflictPolicy ?? "skip",
        authStorage: (savedAuthConfig.authStorage as "kv" | "otp") ?? "kv",
      };
    }
    if (sharedConfig) {
      chipId.value = sharedConfig.chipId;
      baudRate.value = sharedConfig.baudRate;
      authBaudRate.value = sharedConfig.authBaudRate;
      flashFirmware.value = sharedConfig.flashFirmware ?? true;
    } else {
      // First run: apply manifest defaults for the initial chip.
      const m = chipManifest(chipId.value);
      baudRate.value = m.defaultBaudRate;
      authBaudRate.value = m.defaultAuthBaudRate;
    }
    if (firmware) {
      firmwareSource.value = firmware.source;
      selectedDefaultVersion.value = firmware.version;
      if (firmware.localPath) {
        localFirmwarePath.value = firmware.localPath;
      }
      if (firmware.source === "local" && firmware.localPath) {
        firmwarePath.value = firmware.localPath;
      } else if (firmware.source === "default") {
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

  async function saveSharedConfig() {
    await saveBatchFlashAuthSharedConfig({
      chipId: chipId.value,
      baudRate: baudRate.value,
      authBaudRate: authBaudRate.value,
      flashFirmware: flashFirmware.value,
    });
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
    if (!unlistenRead) {
      unlistenRead = await listen<BatchAuthReadProgressEvent>(
        "batch-auth-read-progress",
        ({ payload }) => {
          handleReadProgress(payload);
        },
      );
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
    unlistenRead?.();
    unlistenRead = undefined;
  }

  watch(authConfig, () => void saveAuthConfig(), { deep: true });
  watch(
    [chipId, baudRate, authBaudRate, flashFirmware],
    () => void saveSharedConfig(),
  );

  // OTP is write-once — a device that already holds different credentials can
  // never be overwritten, so "Overwrite" is meaningless for OTP. Force Skip.
  watch(
    () => authConfig.value.authStorage,
    (storage) => {
      if (storage === "otp" && authConfig.value.conflictPolicy !== "skip") {
        authConfig.value.conflictPolicy = "skip";
      }
    },
    { immediate: true },
  );

  // Only OTP-capable chips may keep OTP storage; switching to any other chip
  // resets storage to KV so a stale "otp" cannot leak to a non-OTP chip.
  watch(chipId, () => {
    if (!isOtpCapable.value && authConfig.value.authStorage === "otp") {
      authConfig.value.authStorage = "kv";
    }
  });

  // The default-firmware list is per-chip: on chip switch, drop the previous
  // chip's list and firmware path (the manifest may have nothing at all for
  // the new chip) and reload. An in-flight manifest load already filters by
  // the new chipId on completion, so it is not restarted.
  watch(chipId, () => {
    if (firmwareSource.value !== "default") return;
    firmwarePath.value = "";
    defaultFirmwareEntries.value = [];
    defaultFirmwareError.value = "";
    if (defaultFirmwareStatus.value !== "loading") {
      void loadDefaultFirmwareList();
    }
  });

  return {
    // State
    slots,
    chipId,
    baudRate,
    authBaudRate,
    firmwarePath,
    flashFirmware,
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
    excelStats,
    excelError,
    archiveStatus,
    archiveError,
    lastArchivePath,
    // Computed
    canFlash,
    isOtpCapable,
    opMode,
    currentStats,
    inputsValid,
    isBusy,
    canStart,
    canCancel,
    canRetry,
    canReadAll,
    canArchive,
    filterActive,
    batchStartTime,
    batchEndTime,
    // Actions
    addPorts,
    removeSlot,
    autoAssign,
    setFirmwareSource,
    setLocalFirmwarePath,
    loadDefaultFirmwareList,
    downloadDefaultFirmware,
    startAuth,
    startBatch,
    retryFailed,
    retryPort,
    cancelPort,
    cancelAll,
    readPort,
    readAll,
    archiveBatch,
    blockPort,
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
    handleReadProgress,
    checkBatchCompletion,
    currentBatchPorts,
  };
});
