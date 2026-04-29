<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { APP_VERSION } from '@/config/app';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { isPortableInstall } from '@/utils/install-type';
import { UPDATE_SOURCES, fetchLatestJson, isNewerVersion, type LatestJson } from './update-sources';

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: 'close'): void }>();

const { t } = useI18n();

// ── Source state ──────────────────────────────────────────────────────────────

type SourceStatus = 'idle' | 'checking' | 'available' | 'upToDate' | 'failed';

interface SourceState {
  id: 'github' | 'gitee';
  labelKey: string;
  status: SourceStatus;
  version: string;
  elapsed: number;
  manifest: LatestJson | null;
  error: string;
}

function makeSourceState(s: { id: 'github' | 'gitee'; labelKey: string }, status: SourceStatus = 'idle'): SourceState {
  return { id: s.id, labelKey: s.labelKey, status, version: '', elapsed: 0, manifest: null, error: '' };
}

const sourceStates = ref<SourceState[]>(UPDATE_SOURCES.map(s => makeSourceState(s)));

// ── Download state ────────────────────────────────────────────────────────────

const downloading = ref(false);
const downloadReady = ref(false);
const downloadPercent = ref(0);
const downloadedBytes = ref(0);
const totalBytes = ref(0);
const downloadingSource = ref('');
const downloadingVersion = ref('');
const installing = ref(false);

// Hold the Update object so we can call install() later after user confirms
let pendingUpdate: Awaited<ReturnType<typeof import('@tauri-apps/plugin-updater').check>> = null;

// ── Portable detection ───────────────────────────────────────────────────────

const isPortable = ref(false);

// ── Helpers ───────────────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function resetState(): void {
  sourceStates.value = UPDATE_SOURCES.map(s => makeSourceState(s, 'checking'));
  downloading.value = false;
  downloadReady.value = false;
  downloadPercent.value = 0;
  downloadedBytes.value = 0;
  totalBytes.value = 0;
  downloadingSource.value = '';
  downloadingVersion.value = '';
  installing.value = false;
  pendingUpdate = null;
  // isPortable is cached globally, no need to reset
}

// ── Check updates (parallel) ──────────────────────────────────────────────────

async function checkSource(source: (typeof UPDATE_SOURCES)[number], idx: number): Promise<void> {
  const start = Date.now();
  try {
    const manifest = await fetchLatestJson(source.url);
    const elapsed = parseFloat(((Date.now() - start) / 1000).toFixed(1));
    const newer = isNewerVersion(manifest.version, APP_VERSION);
    sourceStates.value[idx] = {
      id: source.id,
      labelKey: source.labelKey,
      status: newer ? 'available' : 'upToDate',
      version: manifest.version,
      elapsed,
      manifest,
      error: '',
    };
  } catch (e: unknown) {
    const elapsed = parseFloat(((Date.now() - start) / 1000).toFixed(1));
    const errMsg = e instanceof Error ? e.message : String(e);
    sourceStates.value[idx] = {
      id: source.id,
      labelKey: source.labelKey,
      status: 'failed',
      version: '',
      elapsed,
      manifest: null,
      error: errMsg,
    };
  }
}

async function runChecks(): Promise<void> {
  await Promise.all(UPDATE_SOURCES.map((src, i) => checkSource(src, i)));
}

// ── Watch open ────────────────────────────────────────────────────────────────

watch(
  () => props.open,
  (val, prev) => {
    if (val && !prev) {
      resetState();
      void isPortableInstall().then(v => {
        isPortable.value = v;
      });
      void runChecks();
    }
  }
);

// ── Download & install ────────────────────────────────────────────────────────

async function startDownload(srcState: SourceState): Promise<void> {
  if (!isTauriRuntime()) return;
  downloading.value = true;
  downloadReady.value = false;
  downloadPercent.value = 0;
  downloadedBytes.value = 0;
  totalBytes.value = 0;
  downloadingSource.value = t(srcState.labelKey);
  downloadingVersion.value = srcState.version;

  const { info, error: logError } = await import('@tauri-apps/plugin-log');

  try {
    await info(`[Update] startDownload: source=${srcState.id}, version=${srcState.version}`);
    const { check } = await import('@tauri-apps/plugin-updater');
    await info('[Update] calling check()...');
    const update = await check();
    await info(
      `[Update] check() returned: available=${update?.available}, version=${update?.version}, currentVersion=${update?.currentVersion}`
    );
    if (update) {
      await info(`[Update] update details: date=${update.date}, body=${update.body?.substring(0, 200)}`);
    }
    if (!update?.available) {
      await info('[Update] no update available from plugin-updater, aborting download');
      downloading.value = false;
      return;
    }
    // Download only — do NOT install yet.
    // On Windows, downloadAndInstall() launches NSIS immediately after download,
    // giving the user no chance to choose "restart later". By splitting into
    // download() + install(), we can show the restart prompt first.
    await info('[Update] starting download...');
    await update.download(evt => {
      if (evt.event === 'Started') {
        totalBytes.value = (evt.data as { contentLength?: number }).contentLength ?? 0;
        void info(`[Update] download Started: contentLength=${totalBytes.value}`);
      } else if (evt.event === 'Progress') {
        downloadedBytes.value += (evt.data as { chunkLength: number }).chunkLength;
        if (totalBytes.value > 0) {
          downloadPercent.value = Math.round((downloadedBytes.value / totalBytes.value) * 100);
        }
      } else if (evt.event === 'Finished') {
        downloadPercent.value = 100;
        void info('[Update] download Finished');
      }
    });
    // Download finished — hold the update object for later install
    await info('[Update] download complete, ready to install');
    pendingUpdate = update;
    downloading.value = false;
    downloadReady.value = true;
    downloadPercent.value = 100;
  } catch (e: unknown) {
    const errMsg = e instanceof Error ? `${e.message}\n${e.stack}` : String(e);
    await logError(`[Update] startDownload failed: ${errMsg}`);
    console.error('[Update] startDownload failed:', e);
    downloading.value = false;
  }
}

async function restartNow(): Promise<void> {
  if (!isTauriRuntime()) return;
  installing.value = true;
  const { info, error: logError } = await import('@tauri-apps/plugin-log');
  try {
    await info('[Update] restartNow: starting install...');
    // Install the previously downloaded update, then relaunch
    if (pendingUpdate) {
      await info('[Update] calling pendingUpdate.install()...');
      await pendingUpdate.install();
      await info('[Update] install() completed, relaunching...');
      pendingUpdate = null;
    } else {
      await info('[Update] restartNow: pendingUpdate is null, skipping install');
    }
    const { relaunch } = await import('@tauri-apps/plugin-process');
    await relaunch();
  } catch (e: unknown) {
    const errMsg = e instanceof Error ? `${e.message}\n${e.stack}` : String(e);
    await logError(`[Update] restartNow failed: ${errMsg}`);
    console.error('[Update] restartNow failed:', e);
    installing.value = false;
  }
}

function handleClose(): void {
  if (!downloading.value && !installing.value) emit('close');
}

// ── Portable download ────────────────────────────────────────────────────────

/** Detect current platform key for portable manifest lookup */
function getPortablePlatformKey(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('win')) return 'windows-x86_64';
  if (ua.includes('mac')) {
    // Apple Silicon detection is best-effort; arm64 via navigator.platform or userAgentData
    const isArm =
      navigator.userAgent.includes('ARM') ||
      (navigator as { userAgentData?: { platform?: string } }).userAgentData?.platform === 'macOS';
    return isArm ? 'darwin-aarch64' : 'darwin-x86_64';
  }
  return 'linux-x86_64';
}

async function openPortableDownload(srcState: SourceState): Promise<void> {
  if (!isTauriRuntime()) return;

  // Try to get direct portable URL from manifest
  const platformKey = getPortablePlatformKey();
  const portableUrl = srcState.manifest?.portable?.[platformKey]?.url;

  // Fallback to GitHub releases page
  const url = portableUrl || 'https://github.com/tuya/tyutool/releases/latest';

  try {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  } catch {
    // Last resort: window.open
    window.open(url, '_blank');
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
        <div class="ty-dialog-container" role="dialog" aria-modal="true" aria-labelledby="ty-update-dialog-title">
          <div class="ty-dialog-accent-bar ty-dialog-accent-info" aria-hidden="true" />

          <div class="ty-dialog-header">
            <div class="ty-dialog-header-main">
              <h2 id="ty-update-dialog-title" class="ty-dialog-title">
                {{ t('settings.update.dialogTitle') }}
              </h2>
            </div>
            <button
              type="button"
              class="ty-dialog-close"
              :disabled="downloading || installing"
              :aria-label="t('common.closeDialog')"
              @click="handleClose"
            >
              <FontAwesomeIcon :icon="['fas', 'xmark']" class="size-4" aria-hidden="true" />
            </button>
          </div>

          <div class="ty-dialog-body--update">
            <!-- Current version -->
            <p class="ud-current-version">
              {{ t('settings.update.currentVersion', { version: APP_VERSION }) }}
            </p>

            <!-- Source cards -->
            <div class="ud-sources">
              <div
                v-for="src in sourceStates"
                :key="src.id"
                class="ud-source-card"
                :class="`ud-source-card--${src.status}`"
              >
                <div class="ud-source-header">
                  <!-- Status icon -->
                  <span class="ud-source-icon" aria-hidden="true">
                    <span v-if="src.status === 'checking'" class="ud-spinner" />
                    <FontAwesomeIcon v-else-if="src.status === 'available'" :icon="['fas', 'circle-arrow-up']" />
                    <FontAwesomeIcon v-else-if="src.status === 'upToDate'" :icon="['fas', 'circle-check']" />
                    <FontAwesomeIcon v-else-if="src.status === 'failed'" :icon="['fas', 'circle-xmark']" />
                    <FontAwesomeIcon v-else :icon="['fas', 'circle']" />
                  </span>
                  <!-- Source label -->
                  <span class="ud-source-label">{{ t(src.labelKey) }}</span>
                  <!-- Update button (available state, Tauri only, NOT portable) -->
                  <button
                    v-if="
                      src.status === 'available' && isTauriRuntime() && !isPortable && !downloading && !downloadReady
                    "
                    type="button"
                    class="ud-update-btn"
                    @click="startDownload(src)"
                  >
                    {{ t('settings.update.btnUpdate') }}
                  </button>
                  <!-- Portable: download button instead of in-app update -->
                  <button
                    v-if="
                      src.status === 'available' && isTauriRuntime() && isPortable && !downloading && !downloadReady
                    "
                    type="button"
                    class="ud-update-btn"
                    @click="openPortableDownload(src)"
                  >
                    {{ t('settings.update.portableDownload') }}
                  </button>
                </div>
                <!-- Status text -->
                <p class="ud-source-status-text">
                  <template v-if="src.status === 'checking'">
                    {{ t('settings.update.checking') }}
                  </template>
                  <template v-else-if="src.status === 'available'">
                    {{ t('settings.update.available', { version: src.version, time: src.elapsed }) }}
                  </template>
                  <template v-else-if="src.status === 'upToDate'">
                    {{ t('settings.update.upToDate', { time: src.elapsed }) }}
                  </template>
                  <template v-else-if="src.status === 'failed'">
                    {{ t('settings.update.failed') }}
                  </template>
                </p>
                <!-- Error details (shown when failed) -->
                <p v-if="src.status === 'failed' && src.error" class="ud-source-error">
                  {{ src.error }}
                </p>
              </div>
            </div>

            <!-- Portable mode hint -->
            <div v-if="isPortable && sourceStates.some(s => s.status === 'available')" class="ud-portable-hint">
              <div class="ud-portable-icon" aria-hidden="true">
                <FontAwesomeIcon :icon="['fas', 'circle-info']" class="size-4" />
              </div>
              <p class="ud-portable-text">{{ t('settings.update.portableHint') }}</p>
            </div>

            <!-- Download progress -->
            <div v-if="downloading" class="ud-download-section">
              <p class="ud-download-label">
                {{ t('settings.update.downloading', { source: downloadingSource, version: downloadingVersion }) }}
              </p>
              <div class="ud-progress-bar-wrap">
                <div class="ud-progress-bar" :style="{ width: `${downloadPercent}%` }" />
              </div>
              <p class="ud-download-sub">
                {{
                  t('settings.update.downloadProgress', {
                    percent: downloadPercent,
                    downloaded: formatBytes(downloadedBytes),
                    total: totalBytes > 0 ? formatBytes(totalBytes) : '…',
                  })
                }}
              </p>
            </div>

            <!-- Ready to restart -->
            <div v-if="downloadReady" class="ud-ready-section">
              <div class="ud-ready-icon" aria-hidden="true">
                <FontAwesomeIcon :icon="['fas', 'circle-check']" class="size-5" />
              </div>
              <p class="ud-ready-title">
                {{ t('settings.update.ready', { version: downloadingVersion }) }}
              </p>
              <p class="ud-ready-hint">{{ t('settings.update.readyHint') }}</p>
              <div class="ud-ready-actions">
                <button type="button" class="ud-btn-secondary" :disabled="installing" @click="emit('close')">
                  {{ t('settings.update.restartLater') }}
                </button>
                <button type="button" class="ud-btn-primary" :disabled="installing" @click="restartNow">
                  <span v-if="installing" class="ud-spinner ud-spinner--sm" />
                  {{ installing ? t('settings.update.installing') : t('settings.update.restartNow') }}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* ── Modal shell (aligned with TyConfirmDialog — see AGENTS.md) ── */
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
  padding: 1.5rem;
}

.ty-dialog-container {
  position: relative;
  width: 100%;
  max-width: 25rem;
  border-radius: 1rem;
  border: 1px solid var(--ty-border);
  background-color: var(--ty-surface);
  box-shadow:
    0 12px 40px rgba(0, 0, 0, 0.2),
    0 4px 12px rgba(0, 0, 0, 0.1),
    inset 0 1px 0 rgba(255, 255, 255, 0.06);
  overflow: hidden;
}

.dark .ty-dialog-container {
  box-shadow:
    0 12px 40px rgba(0, 0, 0, 0.6),
    0 4px 12px rgba(0, 0, 0, 0.35),
    inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

.ty-dialog-accent-bar {
  height: 3px;
  width: 100%;
}

.ty-dialog-accent-info {
  background: linear-gradient(90deg, var(--ty-primary), color-mix(in srgb, var(--ty-primary) 60%, transparent));
}

.ty-dialog-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 1rem 0.75rem 0 1.25rem;
}

.ty-dialog-header-main {
  display: flex;
  align-items: flex-start;
  gap: 0.625rem;
  min-width: 0;
  flex: 1;
}

.ty-dialog-title {
  text-align: left;
  font-size: 0.9375rem;
  font-weight: 600;
  color: var(--ty-text);
  line-height: 1.35;
  padding: 0;
  margin: 0;
  letter-spacing: -0.01em;
  flex: 1;
  min-width: 0;
}

.ty-dialog-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.875rem;
  height: 1.875rem;
  margin: 0;
  flex-shrink: 0;
  border-radius: 0.5rem;
  border: 1px solid var(--ty-border);
  background: transparent;
  color: var(--ty-text-muted);
  cursor: pointer;
  transition:
    background-color 0.15s ease,
    color 0.15s ease,
    border-color 0.15s ease,
    box-shadow 0.15s ease;
}

.ty-dialog-close:hover:not(:disabled) {
  background-color: color-mix(in srgb, var(--ty-danger) 12%, var(--ty-surface-muted));
  border-color: color-mix(in srgb, var(--ty-danger) 40%, var(--ty-border));
  color: var(--ty-danger);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.12);
}

.ty-dialog-close:active:not(:disabled) {
  transform: scale(0.95);
}

.ty-dialog-close:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

/* Update-specific body (inner content keeps ud-* below) */
.ty-dialog-body--update {
  padding: 1rem 1.25rem 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.875rem;
}

.ud-current-version {
  font-size: 0.75rem;
  color: var(--ty-text-muted);
}

/* ── Source cards ── */
.ud-sources {
  display: flex;
  flex-direction: column;
  gap: 0.625rem;
}

.ud-source-card {
  border-radius: 0.625rem;
  border: 1px solid var(--ty-border);
  background-color: var(--ty-surface-muted);
  padding: 0.625rem 0.875rem;
  transition: border-color 0.18s ease;
}

.ud-source-card--available {
  border-color: color-mix(in srgb, var(--ty-primary) 50%, var(--ty-border));
  background-color: color-mix(in srgb, var(--ty-primary) 5%, var(--ty-surface-muted));
}

.ud-source-card--upToDate {
  border-color: color-mix(in srgb, var(--ty-success, #22c55e) 40%, var(--ty-border));
}

.ud-source-card--failed {
  border-color: color-mix(in srgb, var(--ty-danger) 40%, var(--ty-border));
}

.ud-source-header {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.ud-source-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1rem;
  flex-shrink: 0;
  font-size: 0.875rem;
}

.ud-source-card--checking .ud-source-icon {
  color: var(--ty-text-muted);
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
  flex: 1;
  font-size: 0.8125rem;
  font-weight: 500;
  color: var(--ty-text);
}

.ud-source-status-text {
  margin-top: 0.25rem;
  font-size: 0.75rem;
  color: var(--ty-text-muted);
  padding-left: 1.5rem;
}

.ud-source-error {
  margin-top: 0.25rem;
  font-size: 0.6875rem;
  color: var(--ty-danger);
  padding-left: 1.5rem;
  word-break: break-all;
  font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace;
  opacity: 0.85;
}

/* ── Portable hint ── */
.ud-portable-hint {
  display: flex;
  align-items: flex-start;
  gap: 0.5rem;
  padding: 0.625rem 0.875rem;
  border-radius: 0.625rem;
  border: 1px solid color-mix(in srgb, var(--ty-warning, #f59e0b) 40%, var(--ty-border));
  background-color: color-mix(in srgb, var(--ty-warning, #f59e0b) 6%, var(--ty-surface-muted));
}

.ud-portable-icon {
  flex-shrink: 0;
  color: var(--ty-warning, #f59e0b);
  margin-top: 0.0625rem;
}

.ud-portable-text {
  font-size: 0.75rem;
  color: var(--ty-text-muted);
  line-height: 1.5;
}

/* ── Update button ── */
.ud-update-btn {
  flex-shrink: 0;
  padding: 0.25rem 0.75rem;
  border-radius: 0.5rem;
  border: none;
  background: linear-gradient(135deg, var(--ty-primary) 0%, var(--ty-primary-hover) 100%);
  color: #fff;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  transition: filter 0.15s ease;
  box-shadow:
    0 1px 3px rgba(0, 0, 0, 0.12),
    0 2px 8px color-mix(in srgb, var(--ty-primary) 25%, transparent);
}

.ud-update-btn:hover {
  filter: brightness(1.06);
}

/* ── Spinner ── */
.ud-spinner {
  display: inline-block;
  width: 0.875rem;
  height: 0.875rem;
  border: 2px solid var(--ty-border);
  border-top-color: var(--ty-text-muted);
  border-radius: 50%;
  animation: ud-spin 0.7s linear infinite;
}

.ud-spinner--sm {
  width: 0.75rem;
  height: 0.75rem;
  border-width: 1.5px;
  border-top-color: #fff;
  vertical-align: -0.1em;
  margin-right: 0.25rem;
}

@keyframes ud-spin {
  to {
    transform: rotate(360deg);
  }
}

/* ── Download progress ── */
.ud-download-section {
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
  padding: 0.75rem 0.875rem;
  border-radius: 0.625rem;
  border: 1px solid var(--ty-border);
  background-color: var(--ty-surface-muted);
}

.ud-download-label {
  font-size: 0.8125rem;
  color: var(--ty-text);
  font-weight: 500;
}

.ud-progress-bar-wrap {
  height: 0.375rem;
  border-radius: 9999px;
  background-color: var(--ty-border);
  overflow: hidden;
}

.ud-progress-bar {
  height: 100%;
  border-radius: 9999px;
  background: linear-gradient(90deg, var(--ty-primary), var(--ty-primary-hover));
  transition: width 0.3s ease;
}

.ud-download-sub {
  font-size: 0.75rem;
  color: var(--ty-text-muted);
  font-variant-numeric: tabular-nums;
}

/* ── Ready section ── */
.ud-ready-section {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.375rem;
  padding: 1rem 0.875rem 0.75rem;
  border-radius: 0.625rem;
  border: 1px solid color-mix(in srgb, var(--ty-success, #22c55e) 40%, var(--ty-border));
  background-color: color-mix(in srgb, var(--ty-success, #22c55e) 5%, var(--ty-surface-muted));
  text-align: center;
}

.ud-ready-icon {
  color: var(--ty-success, #22c55e);
  margin-bottom: 0.125rem;
}

.ud-ready-title {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--ty-text);
}

.ud-ready-hint {
  font-size: 0.75rem;
  color: var(--ty-text-muted);
}

.ud-ready-actions {
  display: flex;
  gap: 0.625rem;
  margin-top: 0.625rem;
  width: 100%;
}

/* ── Shared button styles ── */
.ud-btn-secondary,
.ud-btn-primary {
  flex: 1;
  min-height: 2.125rem;
  border-radius: 0.5rem;
  padding: 0.375rem 0.75rem;
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  transition:
    background-color 0.15s ease,
    filter 0.15s ease,
    transform 0.1s ease;
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

.ud-btn-secondary:hover {
  background-color: var(--ty-surface-muted);
  border-color: var(--ty-border-strong);
}

.ud-btn-primary {
  border: none;
  background: linear-gradient(135deg, var(--ty-primary) 0%, var(--ty-primary-hover) 100%);
  color: #fff;
  box-shadow:
    0 1px 3px rgba(0, 0, 0, 0.12),
    0 2px 8px color-mix(in srgb, var(--ty-primary) 25%, transparent);
}

.ud-btn-primary:hover {
  filter: brightness(1.06);
}

/* ── Transition (same name/keyframes as TyConfirmDialog) ── */
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
    transform: scale(0.92) translateY(8px);
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
    transform: scale(0.95) translateY(4px);
  }
}

@media (prefers-reduced-motion: reduce) {
  .ty-dialog-enter-active,
  .ty-dialog-leave-active {
    transition-duration: 0.01ms !important;
  }

  .ty-dialog-enter-active .ty-dialog-container,
  .ty-dialog-leave-active .ty-dialog-container {
    animation-duration: 0.01ms !important;
  }
}
</style>
