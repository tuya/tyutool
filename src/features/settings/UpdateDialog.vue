<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { APP_VERSION } from "@/config/app";
import { isTauriRuntime } from "@/runtime";
import { canUseInAppUpdater, getManualUpdateFlags } from "@/utils/install-type";
import {
  UPDATE_SOURCES,
  fetchLatestJson,
  isNewerVersion,
} from "./update-sources";
import { updateCheck, updateDownload, updateInstall } from "./in-app-updater";
import {
  deriveUpdateSourceAction,
  deriveUpdateSummaryState,
  type UpdateDialogSourceState,
  type UpdateSourceActionKind,
  type UpdateDialogSourceStatus,
} from "./update-dialog-state";
import { renderMarkdown } from "./render-markdown";
import { splitNotes, type SplitLocale } from "./split-notes";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "close"): void }>();

const { t, locale } = useI18n();

type SourceState = UpdateDialogSourceState;

function makeSourceState(
  source: { id: "github" | "tuya"; labelKey: string },
  status: UpdateDialogSourceStatus = "idle",
): SourceState {
  return {
    id: source.id,
    labelKey: source.labelKey,
    status,
    version: "",
    elapsed: 0,
    manifest: null,
    error: "",
  };
}

const sourceStates = ref<SourceState[]>(
  UPDATE_SOURCES.map((s) => makeSourceState(s)),
);

const downloading = ref(false);
const downloadReady = ref(false);
const downloadPercent = ref(0);
const downloadedBytes = ref(0);
const totalBytes = ref(0);
const downloadingSource = ref("");
const downloadingVersion = ref("");
const installing = ref(false);

const manualUpdateOnly = ref(false);
const debRpmInstall = ref(false);
const installTypeReady = ref(false);

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function resetState(): void {
  sourceStates.value = UPDATE_SOURCES.map((source) =>
    makeSourceState(source, "checking"),
  );
  downloading.value = false;
  downloadReady.value = false;
  downloadPercent.value = 0;
  downloadedBytes.value = 0;
  totalBytes.value = 0;
  downloadingSource.value = "";
  downloadingVersion.value = "";
  installing.value = false;
  installTypeReady.value = false;
  manualUpdateOnly.value = false;
  debRpmInstall.value = false;
}

async function checkSource(
  source: (typeof UPDATE_SOURCES)[number],
  idx: number,
): Promise<void> {
  const start = Date.now();
  try {
    const manifest = await fetchLatestJson(source.url);
    const elapsed = parseFloat(((Date.now() - start) / 1000).toFixed(1));
    const newer = isNewerVersion(manifest.version, APP_VERSION);
    sourceStates.value[idx] = {
      id: source.id,
      labelKey: source.labelKey,
      status: newer ? "available" : "upToDate",
      version: manifest.version,
      elapsed,
      manifest,
      error: "",
    };
  } catch (error: unknown) {
    const elapsed = parseFloat(((Date.now() - start) / 1000).toFixed(1));
    sourceStates.value[idx] = {
      id: source.id,
      labelKey: source.labelKey,
      status: "failed",
      version: "",
      elapsed,
      manifest: null,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

async function runChecks(): Promise<void> {
  await Promise.all(
    UPDATE_SOURCES.map((source, idx) => checkSource(source, idx)),
  );
}

const summaryState = computed(() =>
  deriveUpdateSummaryState({
    sourceStates: sourceStates.value,
    downloading: downloading.value,
    downloadReady: downloadReady.value,
    installing: installing.value,
  }),
);

const primaryAvailableSource = computed(
  () => summaryState.value.availableSource,
);

const stableSource = computed(
  () =>
    primaryAvailableSource.value ??
    sourceStates.value.find((source) => source.status === "upToDate") ??
    sourceStates.value.find((source) => source.status === "failed") ??
    null,
);

const availableUpdate = computed(() => {
  const source = primaryAvailableSource.value;
  if (!source) return null;
  return {
    source,
    version: source.version,
    notes: source.manifest?.notes?.trim() ?? "",
  };
});

const renderedNotes = computed(() => {
  const raw = availableUpdate.value?.notes;
  if (!raw) return "";
  // Collapse the two-block bilingual notes to the active locale before rendering.
  // `locale` is already resolved; anything that isn't zh-CN uses the English block.
  const scoped = splitNotes(
    raw,
    (locale.value === "zh-CN" ? "zh-CN" : "en") as SplitLocale,
  );
  return renderMarkdown(scoped);
});

const inAppUpdateSupported = computed(() =>
  canUseInAppUpdater(installTypeReady.value, {
    manualOnly: manualUpdateOnly.value,
    debRpm: debRpmInstall.value,
  }),
);

const overviewModel = computed(() => {
  switch (summaryState.value.kind) {
    case "available":
      return {
        title: t("settings.update.summaryAvailableTitle"),
        description: t("settings.update.summaryAvailableBody", {
          version: availableUpdate.value?.version ?? "",
        }),
        highlight: `v${availableUpdate.value?.version ?? APP_VERSION}`,
        highlightLabel: t("settings.update.latestVersionLabel"),
      };
    case "upToDate":
      return {
        title: t("settings.update.summaryUpToDateTitle"),
        description: t("settings.update.summaryUpToDateBody", {
          version: APP_VERSION,
        }),
        highlight: `v${APP_VERSION}`,
        highlightLabel: t("settings.update.currentVersionLabel"),
      };
    case "failed":
      return {
        title: t("settings.update.summaryFailedTitle"),
        description: t("settings.update.summaryFailedBody"),
        highlight: `v${APP_VERSION}`,
        highlightLabel: t("settings.update.currentVersionLabel"),
      };
    case "downloading":
      return {
        title: t("settings.update.summaryDownloadingTitle"),
        description: t("settings.update.downloading", {
          source: downloadingSource.value,
          version: downloadingVersion.value,
        }),
        highlight: `${downloadPercent.value}%`,
        highlightLabel: t("settings.update.progressLabel"),
      };
    case "ready":
      return {
        title: t("settings.update.ready", {
          version: downloadingVersion.value,
        }),
        description: t("settings.update.readyHint"),
        highlight: `v${downloadingVersion.value}`,
        highlightLabel: t("settings.update.latestVersionLabel"),
      };
    case "installing":
      return {
        title: t("settings.update.installing"),
        description: t("settings.update.summaryInstallingBody"),
        highlight: `v${downloadingVersion.value}`,
        highlightLabel: t("settings.update.latestVersionLabel"),
      };
    case "checking":
    default:
      return {
        title: t("settings.update.dialogTitle"),
        description: t("settings.update.summaryCheckingBody"),
        highlight: `v${APP_VERSION}`,
        highlightLabel: t("settings.update.currentVersionLabel"),
      };
  }
});

const showInAppUpdateAction = computed(
  () =>
    summaryState.value.kind === "available" &&
    !!primaryAvailableSource.value &&
    isTauriRuntime() &&
    inAppUpdateSupported.value,
);

const showManualUpdateAction = computed(
  () =>
    summaryState.value.kind === "available" &&
    !!primaryAvailableSource.value &&
    isTauriRuntime() &&
    installTypeReady.value &&
    manualUpdateOnly.value,
);

const showRestartActions = computed(
  () =>
    summaryState.value.kind === "ready" ||
    summaryState.value.kind === "installing",
);

function sourceActionKind(source: SourceState): UpdateSourceActionKind {
  return deriveUpdateSourceAction({
    source,
    summaryKind: summaryState.value.kind,
    isTauri: isTauriRuntime(),
    installTypeReady: installTypeReady.value,
    manualUpdateOnly: manualUpdateOnly.value,
    inAppUpdateSupported: inAppUpdateSupported.value,
  });
}

async function triggerSourceAction(source: SourceState): Promise<void> {
  const action = sourceActionKind(source);
  if (action === "download") {
    await startDownload(source);
    return;
  }

  if (action === "manual") {
    await openManualReleaseDownload(source);
  }
}

function sourceStatusLabel(source: SourceState): string {
  switch (source.status) {
    case "available":
      return t("settings.update.sourceStatusAvailable");
    case "upToDate":
      return t("settings.update.sourceStatusUpToDate");
    case "failed":
      return t("settings.update.sourceStatusFailed");
    case "checking":
    case "idle":
    default:
      return t("settings.update.sourceStatusChecking");
  }
}

function sourceStatusCopy(source: SourceState): string {
  switch (source.status) {
    case "available":
      return t("settings.update.available", {
        version: source.version,
        time: source.elapsed,
      });
    case "upToDate":
      return t("settings.update.upToDate", { time: source.elapsed });
    case "failed":
      return t("settings.update.failed");
    case "checking":
    case "idle":
    default:
      return t("settings.update.checking");
  }
}

watch(
  () => props.open,
  (open, previousOpen) => {
    if (open && !previousOpen) {
      resetState();
      void getManualUpdateFlags().then(({ manualOnly, debRpm }) => {
        manualUpdateOnly.value = manualOnly;
        debRpmInstall.value = debRpm;
        installTypeReady.value = true;
      });
      void runChecks();
    }
  },
);

async function startDownload(sourceState: SourceState): Promise<void> {
  if (!isTauriRuntime()) return;
  if (!inAppUpdateSupported.value) return;

  downloading.value = true;
  downloadReady.value = false;
  downloadPercent.value = 0;
  downloadedBytes.value = 0;
  totalBytes.value = 0;
  downloadingSource.value = t(sourceState.labelKey);
  downloadingVersion.value = sourceState.version;

  const { info, error: logError } = await import("@tauri-apps/plugin-log");

  try {
    await info(
      `[Update] startDownload: source=${sourceState.id}, version=${sourceState.version}`,
    );
    const update = await updateCheck(sourceState.id);
    await info(
      `[Update] update_check returned: available=${update.available}, version=${update.version}, currentVersion=${update.currentVersion}`,
    );
    if (update.available) {
      await info(
        `[Update] update details: date=${update.date}, body=${update.body?.substring(0, 200)}`,
      );
    } else {
      await info("[Update] no update available from update_check");
      downloading.value = false;
      return;
    }

    await updateDownload((event) => {
      if (event.event === "Started") {
        totalBytes.value = event.data.contentLength ?? 0;
      } else if (event.event === "Progress") {
        downloadedBytes.value += event.data.chunkLength;
        if (totalBytes.value > 0) {
          downloadPercent.value = Math.round(
            (downloadedBytes.value / totalBytes.value) * 100,
          );
        }
      } else if (event.event === "Finished") {
        downloadPercent.value = 100;
      }
    });

    downloading.value = false;
    downloadReady.value = true;
    downloadPercent.value = 100;
  } catch (error: unknown) {
    const errMsg =
      error instanceof Error
        ? `${error.message}\n${error.stack}`
        : String(error);
    await logError(`[Update] startDownload failed: ${errMsg}`);
    console.error("[Update] startDownload failed:", error);
    downloading.value = false;
  }
}

async function restartNow(): Promise<void> {
  if (!isTauriRuntime()) return;
  installing.value = true;
  const { info, error: logError } = await import("@tauri-apps/plugin-log");

  try {
    await info("[Update] restartNow: starting install...");
    if (downloadReady.value) {
      await updateInstall();
      downloadReady.value = false;
    }
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  } catch (error: unknown) {
    const errMsg =
      error instanceof Error
        ? `${error.message}\n${error.stack}`
        : String(error);
    await logError(`[Update] restartNow failed: ${errMsg}`);
    console.error("[Update] restartNow failed:", error);
    installing.value = false;
  }
}

function handleClose(): void {
  if (!downloading.value && !installing.value) {
    emit("close");
  }
}

function getPortablePlatformKey(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("win")) return "windows-x86_64";
  if (ua.includes("mac")) {
    const isArm =
      navigator.userAgent.includes("ARM") ||
      (navigator as { userAgentData?: { platform?: string } }).userAgentData
        ?.platform === "macOS";
    return isArm ? "darwin-aarch64" : "darwin-x86_64";
  }
  return "linux-x86_64";
}

async function openManualReleaseDownload(
  sourceState: SourceState,
): Promise<void> {
  if (!isTauriRuntime()) return;

  let url: string;
  if (debRpmInstall.value) {
    const releasePageUrl = UPDATE_SOURCES.find(
      (source) => source.id === sourceState.id,
    )?.releasePageUrl;
    url = releasePageUrl || "https://github.com/tuya/tyutool/releases/latest";
  } else {
    const platformKey = getPortablePlatformKey();
    const portableUrl = sourceState.manifest?.portable?.[platformKey]?.url;
    const releasePageUrl = UPDATE_SOURCES.find(
      (source) => source.id === sourceState.id,
    )?.releasePageUrl;
    url =
      portableUrl ||
      releasePageUrl ||
      "https://github.com/tuya/tyutool/releases/latest";
  }

  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } catch {
    window.open(url, "_blank");
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="ty-dialog">
      <div
        v-if="props.open"
        class="ty-dialog-backdrop"
        role="presentation"
        @click.self="handleClose"
        @keydown.esc.window="handleClose"
      >
        <div
          class="ty-dialog-container"
          role="dialog"
          aria-modal="true"
          aria-labelledby="ty-update-dialog-title"
        >
          <div
            class="ty-dialog-accent-bar ty-dialog-accent-info"
            aria-hidden="true"
          />

          <div class="ty-dialog-header">
            <div class="ty-dialog-header-main">
              <h2 id="ty-update-dialog-title" class="ty-dialog-title">
                {{ t("settings.update.dialogTitle") }}
              </h2>
            </div>
            <button
              type="button"
              class="ty-dialog-close"
              :disabled="downloading || installing"
              :aria-label="t('common.closeDialog')"
              @click="handleClose"
            >
              <FontAwesomeIcon
                :icon="['fas', 'xmark']"
                class="size-4"
                aria-hidden="true"
              />
            </button>
          </div>

          <div class="ud-layout">
            <section
              class="ud-overview-card"
              :class="`ud-overview-card--${summaryState.kind}`"
              aria-labelledby="ud-overview-title"
            >
              <div class="ud-overview-top">
                <div
                  class="ud-overview-icon"
                  :class="`ud-overview-icon--${summaryState.kind}`"
                >
                  <span
                    v-if="
                      summaryState.kind === 'checking' ||
                      summaryState.kind === 'downloading' ||
                      summaryState.kind === 'installing'
                    "
                    class="ud-spinner"
                    :class="{
                      'ud-spinner--light': summaryState.kind === 'installing',
                    }"
                    aria-hidden="true"
                  />
                  <FontAwesomeIcon
                    v-else-if="summaryState.kind === 'available'"
                    :icon="['fas', 'circle-arrow-up']"
                    class="size-5"
                    aria-hidden="true"
                  />
                  <FontAwesomeIcon
                    v-else-if="summaryState.kind === 'upToDate'"
                    :icon="['fas', 'circle-check']"
                    class="size-5"
                    aria-hidden="true"
                  />
                  <FontAwesomeIcon
                    v-else-if="summaryState.kind === 'ready'"
                    :icon="['fas', 'download']"
                    class="size-5"
                    aria-hidden="true"
                  />
                  <FontAwesomeIcon
                    v-else
                    :icon="['fas', 'circle-xmark']"
                    class="size-5"
                    aria-hidden="true"
                  />
                </div>

                <div class="ud-overview-copy">
                  <p class="ud-overview-kicker">
                    {{ t("settings.update.dialogTitle") }}
                  </p>
                  <h3 id="ud-overview-title" class="ud-overview-title">
                    {{ overviewModel.title }}
                  </h3>
                  <p class="ud-overview-description">
                    {{ overviewModel.description }}
                  </p>
                </div>

                <div class="ud-overview-highlight">
                  <span class="ud-overview-highlight-label">
                    {{ overviewModel.highlightLabel }}
                  </span>
                  <strong class="ud-overview-highlight-value">
                    {{ overviewModel.highlight }}
                  </strong>
                </div>
              </div>

              <div class="ud-overview-meta">
                <span class="ud-meta-pill">
                  {{
                    t("settings.update.currentVersion", {
                      version: APP_VERSION,
                    })
                  }}
                </span>
                <span v-if="stableSource" class="ud-meta-pill">
                  {{ t(stableSource.labelKey) }}
                </span>
                <span
                  v-if="summaryState.failedCount > 0"
                  class="ud-meta-pill ud-meta-pill--warning"
                >
                  {{
                    t("settings.update.sourceFailures", {
                      count: summaryState.failedCount,
                    })
                  }}
                </span>
              </div>

              <div
                v-if="downloading"
                class="ud-progress-wrap"
                aria-live="polite"
              >
                <div class="ud-progress-track">
                  <div
                    class="ud-progress-bar"
                    :style="{ width: `${downloadPercent}%` }"
                  />
                </div>
                <p class="ud-progress-copy">
                  {{
                    t("settings.update.downloadProgress", {
                      percent: downloadPercent,
                      downloaded: formatBytes(downloadedBytes),
                      total: totalBytes > 0 ? formatBytes(totalBytes) : "…",
                    })
                  }}
                </p>
              </div>

              <div
                v-if="manualUpdateOnly && primaryAvailableSource"
                class="ud-portable-hint"
              >
                <div class="ud-portable-icon" aria-hidden="true">
                  <FontAwesomeIcon
                    :icon="['fas', 'circle-info']"
                    class="size-4"
                  />
                </div>
                <p class="ud-portable-text">
                  {{ t("settings.update.portableHint") }}
                </p>
              </div>

              <div
                v-if="
                  showInAppUpdateAction ||
                  showManualUpdateAction ||
                  showRestartActions
                "
                class="ud-overview-actions"
              >
                <button
                  v-if="showInAppUpdateAction && primaryAvailableSource"
                  type="button"
                  class="ud-btn-primary"
                  @click="startDownload(primaryAvailableSource)"
                >
                  {{ t("settings.update.btnUpdate") }}
                </button>

                <button
                  v-if="showManualUpdateAction && primaryAvailableSource"
                  type="button"
                  class="ud-btn-primary"
                  @click="openManualReleaseDownload(primaryAvailableSource)"
                >
                  {{ t("settings.update.portableDownload") }}
                </button>

                <template v-if="showRestartActions">
                  <button
                    type="button"
                    class="ud-btn-secondary"
                    :disabled="installing"
                    @click="emit('close')"
                  >
                    {{ t("settings.update.restartLater") }}
                  </button>
                  <button
                    type="button"
                    class="ud-btn-primary"
                    :disabled="installing"
                    @click="restartNow"
                  >
                    <span
                      v-if="installing"
                      class="ud-spinner ud-spinner--sm ud-spinner--light"
                    />
                    {{
                      installing
                        ? t("settings.update.installing")
                        : t("settings.update.restartNow")
                    }}
                  </button>
                </template>
              </div>
            </section>

            <section
              v-if="availableUpdate"
              class="ud-notes-card"
              aria-labelledby="ud-notes-title"
            >
              <div class="ud-card-header">
                <div>
                  <h3 id="ud-notes-title" class="ud-card-title">
                    {{
                      t("settings.update.releaseNotes", {
                        version: availableUpdate.version,
                      })
                    }}
                  </h3>
                  <p class="ud-card-subtitle">
                    {{ t(availableUpdate.source.labelKey) }}
                  </p>
                </div>
              </div>

              <!-- eslint-disable-next-line vue/no-v-html -- renderMarkdown escapes all input and emits a fixed tag whitelist (see render-markdown.ts) -->
              <div
                v-if="availableUpdate.notes"
                class="ud-notes-body md-content"
                v-html="renderedNotes"
              />
              <div v-else class="ud-notes-empty">
                {{ t("settings.update.releaseNotesEmpty") }}
              </div>
            </section>

            <section
              class="ud-details-section"
              aria-labelledby="ud-details-title"
            >
              <div class="ud-card-header">
                <div>
                  <h3 id="ud-details-title" class="ud-card-title">
                    {{ t("settings.update.checkDetails") }}
                  </h3>
                  <p class="ud-card-subtitle">
                    {{ t("settings.update.checkDetailsHint") }}
                  </p>
                </div>
              </div>

              <div class="ud-source-grid">
                <article
                  v-for="source in sourceStates"
                  :key="source.id"
                  class="ud-source-card"
                  :class="`ud-source-card--${source.status}`"
                >
                  <div class="ud-source-card-head">
                    <div class="ud-source-heading">
                      <span class="ud-source-icon" aria-hidden="true">
                        <span
                          v-if="source.status === 'checking'"
                          class="ud-spinner ud-spinner--xs"
                        />
                        <FontAwesomeIcon
                          v-else-if="source.status === 'available'"
                          :icon="['fas', 'circle-arrow-up']"
                          class="size-3.5"
                        />
                        <FontAwesomeIcon
                          v-else-if="source.status === 'upToDate'"
                          :icon="['fas', 'circle-check']"
                          class="size-3.5"
                        />
                        <FontAwesomeIcon
                          v-else
                          :icon="['fas', 'circle-xmark']"
                          class="size-3.5"
                        />
                      </span>
                      <span class="ud-source-label">{{
                        t(source.labelKey)
                      }}</span>
                    </div>

                    <span
                      class="ud-source-pill"
                      :class="`ud-source-pill--${source.status}`"
                    >
                      {{ sourceStatusLabel(source) }}
                    </span>
                  </div>

                  <p class="ud-source-copy">
                    {{ sourceStatusCopy(source) }}
                  </p>

                  <p
                    v-if="
                      source.status !== 'checking' &&
                      source.status !== 'idle' &&
                      source.elapsed > 0
                    "
                    class="ud-source-metric"
                  >
                    {{
                      t("settings.update.sourceElapsed", {
                        time: source.elapsed,
                      })
                    }}
                  </p>

                  <p
                    v-if="source.status === 'failed' && source.error"
                    class="ud-source-error"
                  >
                    {{ source.error }}
                  </p>

                  <button
                    v-if="sourceActionKind(source) !== 'none'"
                    type="button"
                    class="ud-source-action"
                    :class="
                      sourceActionKind(source) === 'download'
                        ? 'ud-btn-primary'
                        : 'ud-btn-secondary'
                    "
                    :disabled="downloading || installing"
                    @click="triggerSourceAction(source)"
                  >
                    {{ t("settings.update.updateFromSource") }}
                  </button>
                </article>
              </div>
            </section>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.ty-dialog-backdrop {
  position: fixed;
  inset: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
  padding: 1.25rem;
}

.ty-dialog-container {
  position: relative;
  display: flex;
  max-height: min(90vh, 54rem);
  width: min(100%, 43rem);
  flex-direction: column;
  border-radius: 1.125rem;
  border: 1px solid var(--ty-border);
  background: linear-gradient(
    180deg,
    color-mix(in srgb, var(--ty-surface) 94%, white 6%) 0%,
    var(--ty-surface) 100%
  );
  box-shadow:
    0 24px 60px rgba(15, 23, 42, 0.2),
    0 8px 24px rgba(15, 23, 42, 0.12),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
  overflow: hidden;
}

.dark .ty-dialog-container {
  box-shadow:
    0 24px 60px rgba(0, 0, 0, 0.58),
    0 8px 24px rgba(0, 0, 0, 0.36),
    inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

.ty-dialog-accent-bar {
  height: 3px;
  width: 100%;
}

.ty-dialog-accent-info {
  background: linear-gradient(
    90deg,
    var(--ty-primary),
    color-mix(in srgb, var(--ty-primary) 60%, transparent)
  );
}

.ty-dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 1.1rem 1rem 0 1.35rem;
}

.ty-dialog-header-main {
  min-width: 0;
  flex: 1;
}

.ty-dialog-title {
  margin: 0;
  color: var(--ty-text);
  font-size: 1rem;
  font-weight: 700;
  letter-spacing: -0.015em;
  line-height: 1.2;
}

.ty-dialog-close {
  display: flex;
  height: 2rem;
  width: 2rem;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border-radius: 0.625rem;
  border: 1px solid var(--ty-border);
  background: transparent;
  color: var(--ty-text-muted);
  cursor: pointer;
  transition:
    background-color 0.18s ease,
    color 0.18s ease,
    border-color 0.18s ease,
    box-shadow 0.18s ease;
}

.ty-dialog-close:hover:not(:disabled) {
  background-color: color-mix(
    in srgb,
    var(--ty-danger) 12%,
    var(--ty-surface-muted)
  );
  border-color: color-mix(in srgb, var(--ty-danger) 36%, var(--ty-border));
  color: var(--ty-danger);
}

.ty-dialog-close:disabled {
  cursor: not-allowed;
  opacity: 0.4;
}

.ud-layout {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 1rem;
  overflow-y: auto;
  padding: 1rem 1.35rem 1.35rem;
}

.ud-overview-card,
.ud-notes-card,
.ud-details-section {
  border-radius: 1rem;
  border: 1px solid var(--ty-border);
  background-color: color-mix(in srgb, var(--ty-surface-muted) 82%, white 18%);
}

.dark .ud-overview-card,
.dark .ud-notes-card,
.dark .ud-details-section {
  background-color: color-mix(in srgb, var(--ty-surface-muted) 92%, black 8%);
}

.ud-overview-card {
  display: flex;
  flex-direction: column;
  gap: 0.9rem;
  padding: 1.1rem 1.1rem 1rem;
}

.ud-overview-card--available {
  border-color: color-mix(in srgb, var(--ty-primary) 34%, var(--ty-border));
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--ty-primary) 9%, var(--ty-surface-muted)) 0%,
    color-mix(in srgb, var(--ty-surface) 92%, white 8%) 100%
  );
}

.ud-overview-card--upToDate,
.ud-overview-card--ready {
  border-color: color-mix(
    in srgb,
    var(--ty-success, #22c55e) 34%,
    var(--ty-border)
  );
}

.ud-overview-card--failed {
  border-color: color-mix(in srgb, var(--ty-danger) 28%, var(--ty-border));
}

.ud-overview-card--installing {
  border-color: color-mix(in srgb, var(--ty-primary) 34%, var(--ty-border));
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--ty-primary) 12%, var(--ty-surface-muted)) 0%,
    color-mix(in srgb, var(--ty-primary-hover) 16%, var(--ty-surface)) 100%
  );
}

.ud-overview-top {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  gap: 0.9rem;
  align-items: start;
}

.ud-overview-icon {
  display: flex;
  height: 3rem;
  width: 3rem;
  align-items: center;
  justify-content: center;
  border-radius: 0.9rem;
  border: 1px solid transparent;
}

.ud-overview-icon--checking,
.ud-overview-icon--downloading {
  color: var(--ty-primary);
  background-color: color-mix(in srgb, var(--ty-primary) 10%, transparent);
  border-color: color-mix(in srgb, var(--ty-primary) 20%, transparent);
}

.ud-overview-icon--available {
  color: var(--ty-primary);
  background-color: color-mix(in srgb, var(--ty-primary) 12%, transparent);
  border-color: color-mix(in srgb, var(--ty-primary) 24%, transparent);
}

.ud-overview-icon--upToDate,
.ud-overview-icon--ready {
  color: var(--ty-success, #22c55e);
  background-color: color-mix(
    in srgb,
    var(--ty-success, #22c55e) 12%,
    transparent
  );
  border-color: color-mix(in srgb, var(--ty-success, #22c55e) 24%, transparent);
}

.ud-overview-icon--failed {
  color: var(--ty-danger);
  background-color: color-mix(in srgb, var(--ty-danger) 10%, transparent);
  border-color: color-mix(in srgb, var(--ty-danger) 24%, transparent);
}

.ud-overview-icon--installing {
  color: #fff;
  background-color: color-mix(in srgb, var(--ty-primary) 50%, transparent);
  border-color: color-mix(in srgb, var(--ty-primary) 34%, transparent);
}

.ud-overview-copy {
  min-width: 0;
}

.ud-overview-kicker {
  margin: 0 0 0.2rem;
  color: var(--ty-text-muted);
  font-size: 0.72rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.ud-overview-title {
  margin: 0;
  color: var(--ty-text);
  font-size: 1.12rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  line-height: 1.15;
}

.ud-overview-description {
  margin: 0.4rem 0 0;
  color: var(--ty-text-muted);
  font-size: 0.84rem;
  line-height: 1.6;
}

.ud-overview-highlight {
  display: flex;
  min-width: 8.5rem;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.15rem;
  padding-left: 0.5rem;
}

.ud-overview-highlight-label {
  color: var(--ty-text-muted);
  font-size: 0.72rem;
  font-weight: 600;
  letter-spacing: 0.03em;
  text-transform: uppercase;
}

.ud-overview-highlight-value {
  color: var(--ty-text);
  font-size: 1.6rem;
  font-weight: 800;
  letter-spacing: -0.04em;
  line-height: 1;
}

.ud-overview-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.ud-meta-pill {
  display: inline-flex;
  align-items: center;
  border-radius: 9999px;
  border: 1px solid var(--ty-border);
  background-color: color-mix(in srgb, var(--ty-surface) 88%, white 12%);
  padding: 0.35rem 0.65rem;
  color: var(--ty-text-muted);
  font-size: 0.74rem;
  font-weight: 600;
  line-height: 1;
}

.ud-meta-pill--warning {
  border-color: color-mix(
    in srgb,
    var(--ty-warning, #f59e0b) 40%,
    var(--ty-border)
  );
  color: color-mix(in srgb, var(--ty-warning, #f59e0b) 88%, black 12%);
}

.ud-progress-wrap {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.ud-progress-track {
  height: 0.5rem;
  overflow: hidden;
  border-radius: 9999px;
  background-color: color-mix(in srgb, var(--ty-border) 88%, white 12%);
}

.ud-progress-bar {
  height: 100%;
  border-radius: 9999px;
  background: linear-gradient(
    90deg,
    var(--ty-primary),
    var(--ty-primary-hover)
  );
  transition: width 0.3s ease;
}

.ud-progress-copy {
  margin: 0;
  color: var(--ty-text-muted);
  font-size: 0.76rem;
  font-variant-numeric: tabular-nums;
}

.ud-portable-hint {
  display: flex;
  gap: 0.65rem;
  align-items: flex-start;
  border-radius: 0.85rem;
  border: 1px solid
    color-mix(in srgb, var(--ty-warning, #f59e0b) 36%, var(--ty-border));
  background-color: color-mix(
    in srgb,
    var(--ty-warning, #f59e0b) 6%,
    var(--ty-surface-muted)
  );
  padding: 0.8rem 0.9rem;
}

.ud-portable-icon {
  flex-shrink: 0;
  color: var(--ty-warning, #f59e0b);
}

.ud-portable-text {
  margin: 0;
  color: var(--ty-text-muted);
  font-size: 0.78rem;
  line-height: 1.55;
}

.ud-overview-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.7rem;
}

.ud-btn-secondary,
.ud-btn-primary {
  display: inline-flex;
  min-height: 2.5rem;
  align-items: center;
  justify-content: center;
  gap: 0.45rem;
  flex: 1;
  border-radius: 0.75rem;
  padding: 0.55rem 0.95rem;
  font-size: 0.86rem;
  font-weight: 700;
  cursor: pointer;
  transition:
    background-color 0.18s ease,
    border-color 0.18s ease,
    filter 0.18s ease,
    transform 0.12s ease;
}

.ud-btn-secondary:active,
.ud-btn-primary:active {
  transform: scale(0.98);
}

.ud-btn-secondary {
  border: 1px solid var(--ty-border);
  background-color: var(--ty-surface);
  color: var(--ty-text);
}

.ud-btn-secondary:hover:not(:disabled) {
  background-color: var(--ty-surface-muted);
  border-color: var(--ty-border-strong);
}

.ud-btn-primary {
  border: none;
  color: #fff;
  background: linear-gradient(
    135deg,
    var(--ty-primary) 0%,
    var(--ty-primary-hover) 100%
  );
  box-shadow:
    0 1px 3px rgba(0, 0, 0, 0.12),
    0 6px 18px color-mix(in srgb, var(--ty-primary) 28%, transparent);
}

.ud-btn-primary:hover:not(:disabled) {
  filter: brightness(1.06);
}

.ud-btn-secondary:disabled,
.ud-btn-primary:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

.ud-notes-card,
.ud-details-section {
  padding: 1rem 1rem 1.05rem;
}

.ud-card-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.75rem;
  margin-bottom: 0.8rem;
}

.ud-card-title {
  margin: 0;
  color: var(--ty-text);
  font-size: 0.95rem;
  font-weight: 700;
  letter-spacing: -0.015em;
  line-height: 1.2;
}

.ud-card-subtitle {
  margin: 0.35rem 0 0;
  color: var(--ty-text-muted);
  font-size: 0.78rem;
  line-height: 1.45;
}

.ud-notes-body {
  max-height: 17rem;
  overflow-y: auto;
  overflow-wrap: anywhere;
  padding-right: 0.2rem;
  color: var(--ty-text);
  font-size: 0.84rem;
  line-height: 1.7;
}

.ud-notes-empty {
  border-radius: 0.85rem;
  border: 1px dashed var(--ty-border);
  background-color: color-mix(in srgb, var(--ty-surface) 84%, white 16%);
  padding: 1rem;
  color: var(--ty-text-muted);
  font-size: 0.8rem;
  line-height: 1.6;
}

.ud-source-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.8rem;
}

.ud-source-card {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  border-radius: 0.9rem;
  border: 1px solid var(--ty-border);
  background-color: color-mix(in srgb, var(--ty-surface) 88%, white 12%);
  padding: 0.9rem;
}

.ud-source-card--available {
  border-color: color-mix(in srgb, var(--ty-primary) 30%, var(--ty-border));
}

.ud-source-card--upToDate {
  border-color: color-mix(
    in srgb,
    var(--ty-success, #22c55e) 26%,
    var(--ty-border)
  );
}

.ud-source-card--failed {
  border-color: color-mix(in srgb, var(--ty-danger) 24%, var(--ty-border));
}

.ud-source-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.65rem;
}

.ud-source-heading {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 0.5rem;
}

.ud-source-icon {
  display: inline-flex;
  height: 1rem;
  width: 1rem;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.ud-source-card--available .ud-source-icon {
  color: var(--ty-primary);
}

.ud-source-card--upToDate .ud-source-icon {
  color: var(--ty-success, #22c55e);
}

.ud-source-card--failed .ud-source-icon {
  color: var(--ty-danger);
}

.ud-source-label {
  min-width: 0;
  color: var(--ty-text);
  font-size: 0.82rem;
  font-weight: 600;
}

.ud-source-pill {
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
  border-radius: 9999px;
  border: 1px solid var(--ty-border);
  padding: 0.26rem 0.55rem;
  font-size: 0.7rem;
  font-weight: 700;
  line-height: 1;
}

.ud-source-pill--available {
  border-color: color-mix(in srgb, var(--ty-primary) 30%, transparent);
  color: var(--ty-primary);
}

.ud-source-pill--upToDate {
  border-color: color-mix(in srgb, var(--ty-success, #22c55e) 30%, transparent);
  color: var(--ty-success, #22c55e);
}

.ud-source-pill--failed {
  border-color: color-mix(in srgb, var(--ty-danger) 30%, transparent);
  color: var(--ty-danger);
}

.ud-source-pill--checking,
.ud-source-pill--idle {
  color: var(--ty-text-muted);
}

.ud-source-copy,
.ud-source-metric,
.ud-source-error {
  margin: 0;
  font-size: 0.76rem;
  line-height: 1.55;
}

.ud-source-copy,
.ud-source-metric {
  color: var(--ty-text-muted);
}

.ud-source-error {
  color: var(--ty-danger);
  font-family:
    ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  overflow-wrap: anywhere;
}

.ud-source-action {
  margin-top: auto;
  width: 100%;
}

.md-content :deep(:first-child) {
  margin-top: 0;
}

.md-content :deep(:last-child) {
  margin-bottom: 0;
}

.md-content :deep(p) {
  margin: 0 0 0.85rem;
}

.md-content :deep(.md-h) {
  margin: 1rem 0 0.55rem;
  color: var(--ty-text);
  font-weight: 700;
  letter-spacing: -0.015em;
}

.md-content :deep(.md-h1) {
  font-size: 1rem;
}

.md-content :deep(.md-h2) {
  font-size: 0.93rem;
}

.md-content :deep(.md-h3) {
  font-size: 0.88rem;
}

.md-content :deep(ul),
.md-content :deep(ol) {
  margin: 0 0 0.95rem;
  padding-left: 1.2rem;
}

.md-content :deep(li) {
  margin: 0.3rem 0;
}

.md-content :deep(a) {
  color: var(--ty-primary);
  text-decoration: underline;
  text-underline-offset: 0.16em;
}

.md-content :deep(code) {
  border-radius: 0.35rem;
  background-color: color-mix(in srgb, var(--ty-primary) 8%, transparent);
  padding: 0.08rem 0.35rem;
  font-family:
    ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  font-size: 0.92em;
}

.md-content :deep(pre) {
  margin: 0 0 0.95rem;
  overflow-x: auto;
  border-radius: 0.8rem;
  border: 1px solid color-mix(in srgb, var(--ty-border) 82%, white 18%);
  background-color: color-mix(in srgb, var(--ty-surface) 86%, black 14%);
  padding: 0.8rem 0.9rem;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

.md-content :deep(pre code) {
  background: none;
  padding: 0;
  white-space: pre;
}

.md-content :deep(hr) {
  margin: 1rem 0;
  border: none;
  border-top: 1px solid var(--ty-border);
}

.ud-spinner {
  display: inline-block;
  height: 0.95rem;
  width: 0.95rem;
  border-radius: 50%;
  border: 2px solid var(--ty-border);
  border-top-color: var(--ty-primary);
  animation: ud-spin 0.7s linear infinite;
}

.ud-spinner--light {
  border-top-color: #fff;
}

.ud-spinner--sm {
  height: 0.8rem;
  width: 0.8rem;
  border-width: 1.5px;
}

.ud-spinner--xs {
  height: 0.75rem;
  width: 0.75rem;
  border-width: 1.5px;
}

@keyframes ud-spin {
  to {
    transform: rotate(360deg);
  }
}

.ty-dialog-enter-active {
  transition: opacity 0.22s ease-out;
}

.ty-dialog-leave-active {
  transition: opacity 0.15s ease-in;
}

.ty-dialog-enter-from,
.ty-dialog-leave-to {
  opacity: 0;
}

.ty-dialog-enter-active .ty-dialog-container {
  animation: ty-dialog-in 0.22s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.ty-dialog-leave-active .ty-dialog-container {
  animation: ty-dialog-out 0.15s ease-in forwards;
}

@keyframes ty-dialog-in {
  from {
    opacity: 0;
    transform: scale(0.94) translateY(8px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}

@keyframes ty-dialog-out {
  from {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
  to {
    opacity: 0;
    transform: scale(0.96) translateY(4px);
  }
}

@media (max-width: 640px) {
  .ty-dialog-backdrop {
    padding: 0.85rem;
  }

  .ty-dialog-header {
    padding: 1rem 0.9rem 0 1rem;
  }

  .ud-layout {
    padding: 0.9rem 1rem 1rem;
  }

  .ud-overview-top {
    grid-template-columns: auto minmax(0, 1fr);
  }

  .ud-overview-highlight {
    grid-column: 1 / -1;
    align-items: flex-start;
    padding-left: 0;
  }

  .ud-source-grid {
    grid-template-columns: 1fr;
  }

  .ud-overview-actions {
    flex-direction: column;
  }
}

@media (prefers-reduced-motion: reduce) {
  .ty-dialog-enter-active,
  .ty-dialog-leave-active {
    transition-duration: 0.01ms !important;
  }

  .ty-dialog-enter-active .ty-dialog-container,
  .ty-dialog-leave-active .ty-dialog-container,
  .ud-spinner {
    animation-duration: 0.01ms !important;
  }

  .ud-progress-bar {
    transition-duration: 0.01ms !important;
  }
}
</style>
