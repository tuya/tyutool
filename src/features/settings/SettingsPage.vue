<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useI18n } from "vue-i18n";
import { APP_VERSION } from "@/config/app";
import { useSettingsStore, resolveLocale } from "@/stores/settings";
import { useSerialDebugStore } from "@/stores/serial-debug";
import {
  ARCHIVE_LIMIT_PRESETS,
  VISIBLE_LOG_WINDOW_PRESETS,
} from "@/features/serial-debug/constants";
import type {
  AutoUpdateIntervalId,
  LogLevelId,
  LocalePreference,
  // ThemeStyle,
} from "@/stores/settings";
import { isTauriRuntime } from "@/runtime";
import { DISCLAIMER_KEY } from "@/features/batch-flash-auth/disclaimer";
import { openLogsFolder as openLogsFolderAction } from "./open-logs-folder";
import UpdateDialog from "./UpdateDialog.vue";
import LogViewerDialog from "./LogViewerDialog.vue";
import { buildUpdateEntryModel } from "./update-entry-model";
import TySelect, { type TySelectOption } from "@/components/TySelect.vue";
import TySwitch from "@/components/TySwitch.vue";

const { locale, t } = useI18n();
const settings = useSettingsStore();
const sd = useSerialDebugStore();

const appVersion = APP_VERSION;
const showUpdateDialog = ref(false);
const showLogViewer = ref(false);
const updateEntryModel = buildUpdateEntryModel(appVersion);

const logLevelOptions = computed(() => [
  { value: "error", label: "Error" },
  { value: "warn", label: "Warn" },
  { value: "info", label: "Info" },
  { value: "debug", label: "Debug" },
  { value: "trace", label: "Trace" },
]);

const localeOptions = computed<TySelectOption[]>(() => [
  { value: "auto", label: t("settings.langAuto") },
  { value: "zh-CN", label: t("settings.langZh") },
  { value: "en", label: t("settings.langEn") },
]);

const autoUpdateIntervalOptions = computed<TySelectOption[]>(() => [
  { value: "off", label: t("settings.autoUpdateOff") },
  { value: "1h", label: t("settings.autoUpdate1h") },
  { value: "6h", label: t("settings.autoUpdate6h") },
  { value: "12h", label: t("settings.autoUpdate12h") },
  { value: "24h", label: t("settings.autoUpdate24h") },
]);

const logWindowOptions = computed<TySelectOption[]>(() =>
  VISIBLE_LOG_WINDOW_PRESETS.map((lines) => ({
    value: String(lines),
    label: t("serialDebug.logWindow.lines", { count: lines }),
  })),
);

const logWindowValue = computed({
  get: () => String(sd.logWindowLines),
  set: (val: string) => {
    sd.logWindowLines = Number(val);
  },
});

const archiveLimitOptions = computed<TySelectOption[]>(() =>
  ARCHIVE_LIMIT_PRESETS.map((mib) => ({
    value: String(mib),
    label: t("serialDebug.archiveLimit.size", { mib }),
  })),
);

const archiveLimitValue = computed({
  get: () => String(sd.archiveLimitMib),
  set: (val: string) => {
    sd.archiveLimitMib = Number(val);
  },
});

const localeValue = computed({
  get: () => settings.locale,
  set: (val: string) => {
    settings.setLocale(val as LocalePreference);
    locale.value = resolveLocale(val as LocalePreference);
  },
});

const autoUpdateIntervalValue = computed({
  get: () => settings.autoUpdateInterval,
  set: (val: string) =>
    settings.setAutoUpdateInterval(val as AutoUpdateIntervalId),
});

// const themeStyleOptions = computed<TySelectOption[]>(() => [
//   { value: "default", label: t("settings.themeStyleDefault") },
// ]);
//
// const themeStyleValue = computed({
//   get: () => settings.themeStyle,
//   set: (val: string) => settings.setThemeStyle(val as ThemeStyle),
// });

// Sync vue-i18n locale when settings locale changes (e.g. from Tauri store load)
watch(
  () => settings.locale,
  (pref) => {
    locale.value = resolveLocale(pref);
  },
);

function openLogsFolder(): Promise<void> {
  return openLogsFolderAction(t);
}

async function openOpensourceLicenses(): Promise<void> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl("https://github.com/tuya/tyutool/blob/HEAD/LICENSE.txt");
  } catch (_e) {
    window.open(
      "https://github.com/tuya/tyutool/blob/HEAD/LICENSE.txt",
      "_blank",
    );
  }
}

/** TuyaOpen tyutool documentation — locale-specific path when available. */
const tyutoolDocsUrl = computed(() =>
  locale.value.toLowerCase().startsWith("zh")
    ? "https://tuyaopen.ai/zh/docs/tyutool"
    : "https://tuyaopen.ai/docs/tyutool",
);

async function openDocumentation(): Promise<void> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(tyutoolDocsUrl.value);
  } catch (_e) {
    window.open(tyutoolDocsUrl.value, "_blank");
  }
}

// Show inline "done" feedback after resetting the batch-auth disclaimer;
// there is no general-purpose toast, so this transient flag is enough.
const disclaimerReset = ref(false);
let disclaimerResetTimer: ReturnType<typeof setTimeout> | undefined;
function resetBatchAuthDisclaimer(): void {
  localStorage.removeItem(DISCLAIMER_KEY);
  disclaimerReset.value = true;
  clearTimeout(disclaimerResetTimer);
  disclaimerResetTimer = setTimeout(() => {
    disclaimerReset.value = false;
  }, 2500);
}
</script>

<template>
  <div class="flex min-h-0 min-w-0 flex-col gap-3 sm:gap-4">
    <header
      class="page-header relative flex min-w-0 flex-col gap-3 overflow-hidden p-3 sm:flex-row sm:items-center sm:p-3.5"
    >
      <div
        class="page-header-bg pointer-events-none absolute inset-0"
        aria-hidden="true"
      />
      <div class="relative flex min-w-0 flex-1 items-center gap-3">
        <div
          class="page-header-icon flex size-10 shrink-0 items-center justify-center rounded-xl"
          aria-hidden="true"
        >
          <FontAwesomeIcon :icon="['fas', 'gear']" class="size-5" />
        </div>
        <div class="min-w-0">
          <p class="page-header-section">{{ t("settings.section") }}</p>
          <h1 class="page-header-title mt-0.5">{{ t("settings.title") }}</h1>
        </div>
        <div
          class="page-header-divider ml-1 hidden h-8 w-px shrink-0 sm:block"
          aria-hidden="true"
        />
        <p class="page-header-desc relative hidden max-w-[26rem] sm:block">
          {{ t("settings.subtitle") }}
        </p>
      </div>
    </header>

    <section
      class="settings-update-center relative overflow-hidden rounded-2xl p-4 sm:p-5"
      aria-labelledby="update-center-heading"
    >
      <div class="settings-update-center__bg" aria-hidden="true" />
      <div class="settings-update-center__layout">
        <div class="settings-update-center__intro">
          <p class="settings-update-center__eyebrow">
            {{ t(updateEntryModel.metaLabelKey) }}
          </p>
          <div class="settings-update-center__headline-row">
            <div class="min-w-0">
              <h2
                id="update-center-heading"
                class="settings-update-center__title"
              >
                {{ t(updateEntryModel.panelTitleKey) }}
              </h2>
              <p class="settings-update-center__body">
                {{ t(updateEntryModel.panelBodyKey) }}
              </p>
            </div>
            <div class="settings-update-center__version">
              <span class="settings-update-center__version-label">
                {{ t(updateEntryModel.versionLabelKey) }}
              </span>
              <strong class="settings-update-center__version-value">
                {{ updateEntryModel.badge }}
              </strong>
            </div>
          </div>
        </div>

        <button
          v-if="isTauriRuntime()"
          type="button"
          class="update-entry-card update-entry-card--hero"
          @click="showUpdateDialog = true"
        >
          <div class="update-entry-card__glow" aria-hidden="true" />
          <div class="update-entry-card__icon" aria-hidden="true">
            <FontAwesomeIcon
              :icon="['fas', 'arrows-rotate']"
              class="size-4.5"
            />
          </div>
          <div class="update-entry-card__body">
            <div class="update-entry-card__meta">
              <span class="update-entry-card__meta-label">
                {{ t(updateEntryModel.metaLabelKey) }}
              </span>
              <span class="update-entry-card__badge">
                {{ updateEntryModel.badge }}
              </span>
            </div>
            <strong class="update-entry-card__title">
              {{ t(updateEntryModel.titleKey) }}
            </strong>
            <p class="update-entry-card__subtitle">
              {{ t(updateEntryModel.subtitleKey) }}
            </p>
          </div>
          <div class="update-entry-card__arrow" aria-hidden="true">
            <FontAwesomeIcon :icon="['fas', 'angle-right']" class="size-4" />
          </div>
        </button>

        <div class="settings-update-center__control">
          <div class="settings-update-center__control-copy">
            <label
              for="settings-auto-update-interval"
              class="block text-sm font-medium text-[var(--ty-text)]"
            >
              {{ t("settings.autoUpdate") }}
            </label>
            <p class="settings-update-center__control-hint">
              {{ t("settings.autoUpdateHint") }}
            </p>
          </div>
          <TySelect
            id="settings-auto-update-interval"
            v-model="autoUpdateIntervalValue"
            :options="autoUpdateIntervalOptions"
            class="settings-update-center__control-select"
            style="height: 2.75rem"
          />
        </div>
      </div>
    </section>

    <div
      class="grid min-w-0 grid-cols-1 gap-3 md:grid-cols-2 md:items-start md:gap-4"
    >
      <section
        class="ty-card min-w-0 rounded-xl p-4 sm:p-5"
        aria-labelledby="appearance-heading"
      >
        <div class="settings-card-head">
          <h2 id="appearance-heading" class="ty-section-title">
            {{ t("settings.appearance") }}
          </h2>
          <p class="settings-card-subtitle">
            {{ t("settings.appearanceHint") }}
          </p>
        </div>
        <!-- Theme style: temporarily hidden -->
        <!-- <fieldset class="mt-4 space-y-2"> ... </fieldset> -->
        <!-- Theme mode: segmented icons -->
        <fieldset class="mt-4 space-y-2">
          <legend class="text-sm font-medium text-[var(--ty-text)]">
            {{ t("settings.theme") }}
          </legend>
          <div
            class="inline-flex w-full max-w-md rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] p-1"
            role="radiogroup"
            :aria-label="t('settings.themeLegend')"
          >
            <button
              v-for="opt in [
                { v: 'light', icon: 'sun', label: t('settings.themeLight') },
                { v: 'dark', icon: 'moon', label: t('settings.themeDark') },
                {
                  v: 'system',
                  icon: 'circle-half-stroke',
                  label: t('settings.themeSystem'),
                },
              ]"
              :key="opt.v"
              type="button"
              role="radio"
              :aria-checked="settings.theme === opt.v"
              class="flex flex-1 items-center justify-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium transition-colors"
              :class="
                settings.theme === opt.v
                  ? 'bg-[var(--ty-surface)] text-[var(--ty-primary)] shadow-sm ring-1 ring-[color-mix(in_srgb,var(--ty-primary)_32%,transparent)]'
                  : 'text-[var(--ty-text-muted)] hover:text-[var(--ty-text)]'
              "
              @click="settings.setTheme(opt.v as 'light' | 'dark' | 'system')"
            >
              <FontAwesomeIcon
                :icon="['fas', opt.icon]"
                class="size-3.5"
                aria-hidden="true"
              />
              <span>{{ opt.label }}</span>
            </button>
          </div>
        </fieldset>
        <div class="settings-row-group mt-6">
          <div class="settings-row">
            <div class="settings-row__copy">
              <label for="settings-locale" class="settings-row__title">{{
                t("settings.language")
              }}</label>
              <p class="settings-row__hint">
                {{ t("settings.languageHint") }}
              </p>
            </div>
            <TySelect
              id="settings-locale"
              v-model="localeValue"
              :options="localeOptions"
              class="settings-row__control settings-row__control--select"
              style="height: 2.5rem"
            />
          </div>

          <div class="settings-row">
            <div class="settings-row__copy">
              <span class="settings-row__title">{{
                t("settings.serialPortIndicators")
              }}</span>
              <p class="settings-row__hint">
                {{ t("settings.serialPortIndicatorsHint") }}
              </p>
            </div>
            <TySwitch
              :model-value="settings.serialPortIndicatorsEnabled"
              :aria-label="t('settings.serialPortIndicators')"
              @update:model-value="settings.setSerialPortIndicatorsEnabled"
            />
          </div>
        </div>
      </section>

      <section
        class="ty-card min-w-0 rounded-xl p-4 sm:p-5"
        aria-labelledby="diagnostics-heading"
      >
        <div class="settings-card-head">
          <h2 id="diagnostics-heading" class="ty-section-title">
            {{ t("settings.diagnosticsTitle") }}
          </h2>
          <p class="settings-card-subtitle">
            {{ t("settings.diagnosticsHint") }}
          </p>
        </div>
        <div class="settings-row-group mt-4">
          <div class="settings-row">
            <div class="settings-row__copy">
              <span class="settings-row__title">{{
                t("settings.logEnabled")
              }}</span>
              <p class="settings-row__hint">
                {{ t("settings.diagnosticsHint") }}
              </p>
            </div>
            <TySwitch
              :model-value="settings.logEnabled"
              :aria-label="t('settings.logEnabled')"
              @update:model-value="settings.setLogEnabled"
            />
          </div>

          <div class="settings-row">
            <div class="settings-row__copy">
              <span class="settings-row__title">{{
                t("settings.logLevel")
              }}</span>
              <p class="settings-row__hint">
                {{ t("settings.logLevelHint") }}
              </p>
            </div>
            <TySelect
              :model-value="settings.logLevel"
              :options="logLevelOptions"
              :disabled="!settings.logEnabled"
              class="settings-row__control settings-row__control--select"
              @update:model-value="settings.setLogLevel($event as LogLevelId)"
            />
          </div>

          <div class="diagnostics-actions">
            <button
              type="button"
              class="diagnostics-action-card"
              @click="openLogsFolder"
            >
              <div class="diagnostics-action-card__icon" aria-hidden="true">
                <FontAwesomeIcon
                  :icon="['fas', 'folder-open']"
                  class="size-4"
                />
              </div>
              <div class="min-w-0">
                <strong class="diagnostics-action-card__title">
                  {{ t("settings.logsFolder") }}
                </strong>
                <p class="diagnostics-action-card__copy">
                  {{ t("settings.logsFolderHint") }}
                </p>
              </div>
            </button>

            <button
              type="button"
              class="diagnostics-action-card"
              @click="showLogViewer = true"
            >
              <div class="diagnostics-action-card__icon" aria-hidden="true">
                <FontAwesomeIcon :icon="['fas', 'file-lines']" class="size-4" />
              </div>
              <div class="min-w-0">
                <strong class="diagnostics-action-card__title">
                  {{ t("settings.viewLogs") }}
                </strong>
                <p class="diagnostics-action-card__copy">
                  {{ t("settings.viewLogsHint") }}
                </p>
              </div>
            </button>
          </div>
        </div>
      </section>
    </div>

    <section
      class="ty-card min-w-0 rounded-xl p-4 sm:p-5"
      aria-labelledby="serial-debug-heading"
    >
      <div class="settings-card-head">
        <h2 id="serial-debug-heading" class="ty-section-title">
          {{ t("settings.serialLogsSection") }}
        </h2>
        <p class="settings-card-subtitle">
          {{ t("settings.serialLogsHint") }}
        </p>
      </div>
      <div class="settings-row-group mt-4">
        <div class="settings-row">
          <div class="settings-row__copy">
            <span class="settings-row__title">{{
              t("serialDebug.autoSave.label")
            }}</span>
            <p class="settings-row__hint">
              {{ t("serialDebug.autoSave.description") }}
            </p>
          </div>
          <TySwitch
            :model-value="sd.autoSave"
            :aria-label="t('serialDebug.autoSave.label')"
            @update:model-value="sd.setAutoSaveEnabled"
          />
        </div>
        <div class="settings-row settings-row--path">
          <div class="settings-row__copy">
            <span class="settings-row__title">{{
              t("serialDebug.autoSave.dirLabel")
            }}</span>
            <div class="path-scroll min-w-0 max-w-full overflow-x-auto">
              <span class="settings-row__value">{{
                sd.autoSaveDir || "—"
              }}</span>
            </div>
          </div>
          <button
            type="button"
            class="ty-btn-secondary settings-inline-action"
            @click="sd.pickAutoSaveDir()"
          >
            {{ t("serialDebug.autoSave.pickDir") }}
          </button>
        </div>
        <div class="settings-row">
          <div class="settings-row__copy">
            <span class="settings-row__title">{{
              t("serialDebug.autoSave.timestamp")
            }}</span>
            <p class="settings-row__hint">
              {{
                sd.autoSaveTimestamp
                  ? t("serialDebug.autoSave.timestampFmtOn")
                  : t("serialDebug.autoSave.timestampFmtOff")
              }}
            </p>
          </div>
          <TySwitch
            v-model="sd.autoSaveTimestamp"
            :aria-label="t('serialDebug.autoSave.timestamp')"
          />
        </div>
        <div class="settings-row">
          <div class="settings-row__copy">
            <span class="settings-row__title">{{
              t("serialDebug.logWindow.label")
            }}</span>
            <p class="settings-row__hint">
              {{ t("serialDebug.logWindow.hint") }}
            </p>
          </div>
          <TySelect
            id="settings-serial-log-window"
            v-model="logWindowValue"
            :options="logWindowOptions"
            :placeholder="
              t('serialDebug.logWindow.lines', { count: sd.logWindowLines })
            "
            class="settings-row__control settings-row__control--select"
            style="height: 2.5rem"
          />
        </div>
        <div class="settings-row">
          <div class="settings-row__copy">
            <span class="settings-row__title">{{
              t("serialDebug.archiveLimit.label")
            }}</span>
            <p class="settings-row__hint">
              {{ t("serialDebug.archiveLimit.hint") }}
            </p>
          </div>
          <TySelect
            id="settings-serial-archive-limit"
            v-model="archiveLimitValue"
            :options="archiveLimitOptions"
            :placeholder="
              t('serialDebug.archiveLimit.size', { mib: sd.archiveLimitMib })
            "
            class="settings-row__control settings-row__control--select"
            style="height: 2.5rem"
          />
        </div>
      </div>
    </section>

    <section
      class="ty-card min-w-0 rounded-xl p-4 sm:p-5"
      aria-labelledby="about-heading"
    >
      <div class="settings-card-head">
        <h2 id="about-heading" class="ty-section-title">
          {{ t("settings.about") }}
        </h2>
        <p class="settings-card-subtitle">
          {{ t("settings.aboutHint") }}
        </p>
      </div>
      <div class="about-footer">
        <button
          type="button"
          class="ty-btn-secondary settings-inline-action w-full justify-center sm:w-auto"
          @click="openOpensourceLicenses"
        >
          {{ t("settings.opensource") }}
        </button>
        <button
          v-if="isTauriRuntime()"
          type="button"
          class="ty-btn-secondary settings-inline-action w-full justify-center sm:w-auto"
          @click="resetBatchAuthDisclaimer"
        >
          {{ t("settings.resetDisclaimer") }}
        </button>
        <span
          v-if="disclaimerReset"
          class="disclaimer-reset-feedback"
          role="status"
        >
          {{ t("settings.resetDisclaimerDone") }}
        </span>
        <button
          type="button"
          class="docs-cta w-full sm:w-auto"
          @click="openDocumentation"
        >
          <span class="docs-cta__shine" aria-hidden="true"></span>
          <span class="docs-cta__icon" aria-hidden="true">
            <FontAwesomeIcon :icon="['fas', 'book-open']" class="size-3.5" />
          </span>
          <span class="docs-cta__label">{{ t("settings.docs") }}</span>
          <FontAwesomeIcon
            :icon="['fas', 'arrow-up-right-from-square']"
            class="docs-cta__arrow size-3"
            aria-hidden="true"
          />
        </button>
      </div>
    </section>

    <UpdateDialog :open="showUpdateDialog" @close="showUpdateDialog = false" />
    <LogViewerDialog :open="showLogViewer" @close="showLogViewer = false" />
  </div>
</template>

<style scoped>
/* Hide scrollbar while keeping scroll functionality */
.path-scroll::-webkit-scrollbar {
  display: none;
}
.path-scroll {
  scrollbar-width: none;
}

.settings-update-center {
  border: 1px solid color-mix(in srgb, var(--ty-primary) 24%, var(--ty-border));
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--ty-primary) 8%, var(--ty-surface)) 0%,
    color-mix(in srgb, var(--ty-surface) 94%, white 6%) 100%
  );
  box-shadow:
    0 18px 36px rgba(15, 23, 42, 0.08),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

.settings-update-center__bg {
  pointer-events: none;
  position: absolute;
  inset: 0;
  background:
    radial-gradient(
      circle at top right,
      color-mix(in srgb, var(--ty-primary) 16%, transparent) 0,
      transparent 34%
    ),
    radial-gradient(
      circle at left bottom,
      color-mix(in srgb, var(--ty-primary) 10%, transparent) 0,
      transparent 28%
    );
}

.settings-update-center__layout {
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1.3fr) minmax(18rem, 0.9fr);
  gap: 1rem;
  align-items: start;
}

.settings-update-center__intro {
  min-width: 0;
}

.settings-update-center__eyebrow {
  margin: 0 0 0.45rem;
  color: var(--ty-text-muted);
  font-size: 0.72rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.settings-update-center__headline-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
}

.settings-update-center__title {
  margin: 0;
  color: var(--ty-text);
  font-size: 1.28rem;
  font-weight: 800;
  letter-spacing: -0.03em;
  line-height: 1.1;
}

.settings-update-center__body {
  margin: 0.55rem 0 0;
  max-width: 34rem;
  color: var(--ty-text-muted);
  font-size: 0.88rem;
  line-height: 1.65;
}

.settings-update-center__version {
  display: flex;
  min-width: 8.5rem;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.18rem;
  padding-top: 0.1rem;
}

.settings-update-center__version-label {
  color: var(--ty-text-muted);
  font-size: 0.72rem;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.settings-update-center__version-value {
  color: var(--ty-text);
  font-size: 1.4rem;
  font-weight: 800;
  letter-spacing: -0.04em;
  line-height: 1;
}

.settings-update-center__control {
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  border-radius: 1rem;
  border: 1px solid var(--ty-border);
  background-color: color-mix(in srgb, var(--ty-surface) 86%, white 14%);
  padding: 0.9rem 1rem;
}

.settings-update-center__control-copy {
  min-width: 0;
}

.settings-update-center__control-hint {
  margin: 0.28rem 0 0;
  color: var(--ty-text-muted);
  font-size: 0.78rem;
  line-height: 1.55;
}

.settings-update-center__control-select {
  width: 100%;
  max-width: 12rem;
  flex-shrink: 0;
}

.settings-card-head {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.settings-card-subtitle {
  margin: 0;
  color: var(--ty-text-muted);
  font-size: 0.8rem;
  line-height: 1.55;
}

.settings-row-group {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.settings-row {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  border-radius: 1rem;
  border: 1px solid var(--ty-border);
  background-color: color-mix(in srgb, var(--ty-surface) 88%, white 12%);
  padding: 0.9rem 1rem;
}

.settings-row--path {
  align-items: flex-start;
}

.settings-row__copy {
  min-width: 0;
  flex: 1;
}

.settings-row__title {
  display: block;
  color: var(--ty-text);
  font-size: 0.84rem;
  font-weight: 700;
  line-height: 1.2;
}

.settings-row__hint {
  margin: 0.3rem 0 0;
  color: var(--ty-text-muted);
  font-size: 0.75rem;
  line-height: 1.5;
}

.settings-row__value {
  display: block;
  margin-top: 0.28rem;
  color: var(--ty-text-muted);
  font-size: 0.75rem;
  line-height: 1.45;
  white-space: nowrap;
}

.settings-row__control {
  flex-shrink: 0;
}

.settings-row__control--select {
  width: 100%;
  max-width: 12rem;
}

.settings-inline-action {
  display: inline-flex;
  min-height: 2.75rem;
  flex-shrink: 0;
  align-items: center;
  border-radius: 0.75rem;
  padding-inline: 1rem;
}

.update-entry-card {
  position: relative;
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  gap: 0.9rem;
  align-items: center;
  overflow: hidden;
  border-radius: 1rem;
  border: 1px solid color-mix(in srgb, var(--ty-primary) 28%, var(--ty-border));
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--ty-primary) 10%, var(--ty-surface-muted)) 0%,
    color-mix(in srgb, var(--ty-surface) 90%, white 10%) 100%
  );
  padding: 0.95rem 1rem;
  text-align: left;
  cursor: pointer;
  box-shadow:
    0 10px 24px rgba(15, 23, 42, 0.08),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
  transition:
    transform 0.18s ease,
    border-color 0.18s ease,
    box-shadow 0.18s ease,
    background-color 0.18s ease;
}

.update-entry-card--hero {
  min-height: 100%;
  align-self: stretch;
}

.update-entry-card:hover {
  transform: translateY(-1px);
  border-color: color-mix(in srgb, var(--ty-primary) 42%, var(--ty-border));
  box-shadow:
    0 14px 30px rgba(15, 23, 42, 0.1),
    0 4px 14px color-mix(in srgb, var(--ty-primary) 14%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

.update-entry-card:focus-visible {
  outline: none;
  box-shadow:
    0 0 0 3px color-mix(in srgb, var(--ty-primary) 18%, transparent),
    0 14px 30px rgba(15, 23, 42, 0.1),
    0 4px 14px color-mix(in srgb, var(--ty-primary) 14%, transparent);
}

.update-entry-card__glow {
  pointer-events: none;
  position: absolute;
  inset: auto auto -2.25rem -1.5rem;
  height: 5.5rem;
  width: 5.5rem;
  border-radius: 9999px;
  background: color-mix(in srgb, var(--ty-primary) 16%, transparent);
  filter: blur(10px);
  opacity: 0.8;
}

.update-entry-card__icon {
  position: relative;
  z-index: 1;
  display: flex;
  height: 2.75rem;
  width: 2.75rem;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border-radius: 0.9rem;
  border: 1px solid color-mix(in srgb, var(--ty-primary) 22%, transparent);
  background-color: color-mix(in srgb, var(--ty-primary) 12%, transparent);
  color: var(--ty-primary);
}

.update-entry-card__body {
  position: relative;
  z-index: 1;
  min-width: 0;
}

.update-entry-card__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 0.45rem;
  align-items: center;
  margin-bottom: 0.35rem;
}

.update-entry-card__meta-label {
  color: var(--ty-text-muted);
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.update-entry-card__badge {
  display: inline-flex;
  align-items: center;
  border-radius: 9999px;
  border: 1px solid color-mix(in srgb, var(--ty-primary) 24%, transparent);
  background-color: color-mix(in srgb, var(--ty-surface) 82%, white 18%);
  padding: 0.25rem 0.55rem;
  color: var(--ty-primary);
  font-size: 0.72rem;
  font-weight: 700;
  line-height: 1;
}

.update-entry-card__title {
  display: block;
  color: var(--ty-text);
  font-size: 0.95rem;
  font-weight: 700;
  letter-spacing: -0.015em;
  line-height: 1.2;
}

.update-entry-card__subtitle {
  margin: 0.28rem 0 0;
  color: var(--ty-text-muted);
  font-size: 0.79rem;
  line-height: 1.55;
}

.update-entry-card__arrow {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: color-mix(in srgb, var(--ty-primary) 76%, var(--ty-text-muted));
}

.diagnostics-actions {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.75rem;
}

.diagnostics-action-card {
  display: flex;
  min-width: 0;
  align-items: flex-start;
  gap: 0.75rem;
  border-radius: 0.95rem;
  border: 1px solid var(--ty-border);
  background-color: color-mix(in srgb, var(--ty-surface) 88%, white 12%);
  padding: 0.85rem 0.95rem;
  text-align: left;
  cursor: pointer;
  transition:
    border-color 0.18s ease,
    background-color 0.18s ease,
    transform 0.18s ease,
    box-shadow 0.18s ease;
}

.diagnostics-action-card:hover {
  transform: translateY(-1px);
  border-color: color-mix(in srgb, var(--ty-primary) 24%, var(--ty-border));
  box-shadow: 0 10px 24px rgba(15, 23, 42, 0.06);
}

.diagnostics-action-card:focus-visible {
  outline: none;
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--ty-primary) 14%, transparent);
}

.diagnostics-action-card__icon {
  display: flex;
  height: 2.25rem;
  width: 2.25rem;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border-radius: 0.8rem;
  background-color: color-mix(in srgb, var(--ty-primary) 10%, transparent);
  color: var(--ty-primary);
}

.diagnostics-action-card__title {
  display: block;
  color: var(--ty-text);
  font-size: 0.84rem;
  font-weight: 700;
  line-height: 1.2;
}

.diagnostics-action-card__copy {
  margin: 0.28rem 0 0;
  color: var(--ty-text-muted);
  font-size: 0.75rem;
  line-height: 1.5;
}

.about-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 1rem;
  margin-top: 0.95rem;
  border-top: 1px solid color-mix(in srgb, var(--ty-border) 82%, transparent);
  padding-top: 1rem;
}

/* Documentation call-to-action: the one accented button in the about row. */
.docs-cta {
  position: relative;
  display: inline-flex;
  min-height: 2.75rem;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  gap: 0.5rem;
  overflow: hidden;
  border-radius: 0.75rem;
  border: 1px solid color-mix(in srgb, var(--ty-primary) 55%, transparent);
  background: linear-gradient(
    135deg,
    var(--ty-primary) 0%,
    var(--ty-primary-hover) 58%,
    color-mix(in srgb, var(--ty-primary-hover) 84%, black 16%) 100%
  );
  padding-inline: 1rem;
  color: #fff;
  font-size: 0.875rem;
  font-weight: 700;
  letter-spacing: -0.01em;
  cursor: pointer;
  box-shadow:
    0 1px 2px rgba(15, 23, 42, 0.12),
    0 8px 20px color-mix(in srgb, var(--ty-primary) 30%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.18);
  transition:
    transform 0.18s ease,
    border-color 0.18s ease,
    box-shadow 0.18s ease;
}

.docs-cta:hover {
  transform: translateY(-1px);
  border-color: color-mix(in srgb, var(--ty-primary) 72%, transparent);
  box-shadow:
    0 2px 4px rgba(15, 23, 42, 0.14),
    0 14px 28px color-mix(in srgb, var(--ty-primary) 38%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.22);
}

.docs-cta:active {
  transform: translateY(0) scale(0.99);
}

.docs-cta:focus-visible {
  outline: none;
  box-shadow:
    0 0 0 3px color-mix(in srgb, var(--ty-primary) 26%, transparent),
    0 10px 24px color-mix(in srgb, var(--ty-primary) 32%, transparent);
}

/* Light sweep across the face on hover/focus. */
.docs-cta__shine {
  pointer-events: none;
  position: absolute;
  inset: 0;
  background: linear-gradient(
    105deg,
    transparent 38%,
    rgba(255, 255, 255, 0.32) 50%,
    transparent 62%
  );
  transform: translateX(-120%);
  transition: transform 0.55s ease;
}

.docs-cta:hover .docs-cta__shine,
.docs-cta:focus-visible .docs-cta__shine {
  transform: translateX(120%);
}

.docs-cta__icon {
  position: relative;
  display: flex;
  height: 1.55rem;
  width: 1.55rem;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border-radius: 0.55rem;
  border: 1px solid rgba(255, 255, 255, 0.28);
  background-color: rgba(255, 255, 255, 0.18);
}

.docs-cta__label {
  position: relative;
}

.docs-cta__arrow {
  position: relative;
  opacity: 0.85;
  transition:
    transform 0.18s ease,
    opacity 0.18s ease;
}

.docs-cta:hover .docs-cta__arrow {
  transform: translate(2px, -2px);
  opacity: 1;
}

.disclaimer-reset-feedback {
  color: color-mix(in srgb, var(--ty-primary) 85%, var(--ty-text-muted));
  font-size: 0.75rem;
  line-height: 1.5;
}

@media (max-width: 900px) {
  .settings-update-center__layout {
    grid-template-columns: 1fr;
  }

  .update-entry-card--hero {
    min-height: 0;
  }
}

@media (max-width: 640px) {
  .settings-update-center__headline-row {
    flex-direction: column;
  }

  .settings-update-center__version {
    min-width: 0;
    align-items: flex-start;
  }

  .settings-update-center__control {
    flex-direction: column;
    align-items: stretch;
  }

  .settings-update-center__control-select {
    max-width: none;
  }

  .diagnostics-actions {
    grid-template-columns: 1fr;
  }

  .settings-row {
    flex-direction: column;
    align-items: stretch;
  }

  .settings-row__control--select {
    max-width: none;
  }

  .settings-inline-action {
    width: 100%;
    justify-content: center;
  }

  .about-footer {
    flex-direction: column;
    align-items: stretch;
  }
}

@media (prefers-reduced-motion: reduce) {
  .update-entry-card,
  .diagnostics-action-card,
  .docs-cta,
  .docs-cta__arrow {
    transition-duration: 0.01ms !important;
  }

  .docs-cta__shine {
    display: none;
  }
}
</style>
