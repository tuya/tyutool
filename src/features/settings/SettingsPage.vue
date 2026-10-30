<script setup lang="ts">
import TyPageHeader from "@/components/TyPageHeader.vue";
import { ref, computed, watch, onMounted, onUnmounted } from "vue";
import { useRoute, useRouter } from "vue-router";
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

const sectionIds = [
  "appearance-heading",
  "serial-debug-heading",
  "diagnostics-heading",
  "update-center-heading",
  "about-heading",
];
const activeSection = ref(sectionIds[0]);
let scrollRoot: HTMLElement | null = null;
function updateActiveSection(): void {
  if (!scrollRoot) return;
  if (
    scrollRoot.scrollTop + scrollRoot.clientHeight >=
    scrollRoot.scrollHeight - 2
  ) {
    activeSection.value = sectionIds[sectionIds.length - 1];
    return;
  }
  const top = scrollRoot.getBoundingClientRect().top + 48;
  let current = sectionIds[0];
  for (const id of sectionIds) {
    const section = document.getElementById(id)?.closest("section");
    if (section && section.getBoundingClientRect().top <= top) current = id;
  }
  activeSection.value = current;
}
onMounted(() => {
  scrollRoot = document.getElementById("app-main");
  scrollRoot?.addEventListener("scroll", updateActiveSection, {
    passive: true,
  });
  updateActiveSection();
});
onUnmounted(() =>
  scrollRoot?.removeEventListener("scroll", updateActiveSection),
);

function goToSection(id: string): void {
  const heading = document.getElementById(id);
  if (!heading) return;
  activeSection.value = id;
  heading.scrollIntoView({ block: "start" });
  heading.setAttribute("tabindex", "-1");
  heading.focus({ preventScroll: true });
}
const appVersion = APP_VERSION;
const showUpdateDialog = ref(false);
const route = useRoute();
const router = useRouter();
watch(
  () => route.query.update,
  (value) => {
    if (value === "1") {
      showUpdateDialog.value = true;
      const query = { ...route.query };
      delete query.update;
      void router.replace({ path: route.path, query });
    }
  },
  { immediate: true },
);
function onThemeKey(event: KeyboardEvent): void {
  if (
    ![
      "ArrowLeft",
      "ArrowRight",
      "ArrowUp",
      "ArrowDown",
      "Home",
      "End",
    ].includes(event.key)
  )
    return;
  event.preventDefault();
  const buttons = [
    ...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>(
      "[role=radio]",
    ),
  ];
  const index = buttons.findIndex(
    (button) => button.dataset.theme === settings.theme,
  );
  const next =
    event.key === "Home"
      ? 0
      : event.key === "End"
        ? buttons.length - 1
        : (index +
            (["ArrowRight", "ArrowDown"].includes(event.key) ? 1 : -1) +
            buttons.length) %
          buttons.length;
  const theme = buttons[next]?.dataset.theme;
  if (theme === "light" || theme === "dark" || theme === "system")
    settings.setTheme(theme);
  buttons[next]?.focus();
}
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
  <div class="flex min-h-0 min-w-0 flex-col gap-5">
    <TyPageHeader
      :title="t('settings.title')"
      :section="t('settings.section')"
      :description="t('settings.subtitle')"
    />

    <div class="settings-layout">
      <nav class="settings-nav" :aria-label="t('settings.title')">
        <a
          href="#appearance-heading"
          :aria-current="
            activeSection === 'appearance-heading' ? 'location' : undefined
          "
          @click.prevent="goToSection('appearance-heading')"
          >{{ t("settings.appearance") }}</a
        >
        <a
          href="#serial-debug-heading"
          :aria-current="
            activeSection === 'serial-debug-heading' ? 'location' : undefined
          "
          @click.prevent="goToSection('serial-debug-heading')"
          >{{ t("settings.serialLogsSection") }}</a
        >
        <a
          href="#diagnostics-heading"
          :aria-current="
            activeSection === 'diagnostics-heading' ? 'location' : undefined
          "
          @click.prevent="goToSection('diagnostics-heading')"
          >{{ t("settings.diagnosticsTitle") }}</a
        >
        <a
          href="#update-center-heading"
          :aria-current="
            activeSection === 'update-center-heading' ? 'location' : undefined
          "
          @click.prevent="goToSection('update-center-heading')"
          >{{ t("settings.updateNav") }}</a
        >
        <a
          href="#about-heading"
          :aria-current="
            activeSection === 'about-heading' ? 'location' : undefined
          "
          @click.prevent="goToSection('about-heading')"
          >{{ t("settings.about") }}</a
        >
      </nav>
      <div class="settings-sections">
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
          <!-- Theme mode: segmented icons -->
          <fieldset class="mt-4 space-y-2">
            <legend class="text-sm font-medium text-[var(--ty-text)]">
              {{ t("settings.theme") }}
            </legend>
            <div
              class="theme-options"
              role="radiogroup"
              @keydown="onThemeKey"
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
                :data-theme="opt.v"
                :tabindex="settings.theme === opt.v ? 0 : -1"
                :aria-checked="settings.theme === opt.v"
                class="theme-choice"
                @click="settings.setTheme(opt.v as 'light' | 'dark' | 'system')"
              >
                <span
                  class="theme-preview"
                  :class="`theme-preview--${opt.v}`"
                  aria-hidden="true"
                  ><span class="theme-preview-side"><i></i><i></i><i></i></span
                  ><span class="theme-preview-main"><i></i><b></b><i></i></span
                ></span>
                <span class="theme-choice-label"
                  ><FontAwesomeIcon
                    :icon="['fas', opt.icon]"
                    class="size-3.5"
                    aria-hidden="true"
                  />
                  <span>{{ opt.label }}</span></span
                >
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
                <label
                  for="settings-serial-log-window"
                  class="settings-row__title"
                  >{{ t("serialDebug.logWindow.label") }}</label
                >
                <p
                  id="settings-serial-log-window-hint"
                  class="settings-row__hint"
                >
                  {{ t("serialDebug.logWindow.hint") }}
                </p>
              </div>
              <TySelect
                id="settings-serial-log-window"
                :aria-label="t('serialDebug.logWindow.label')"
                aria-describedby="settings-serial-log-window-hint"
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
                <label
                  for="settings-serial-archive-limit"
                  class="settings-row__title"
                  >{{ t("serialDebug.archiveLimit.label") }}</label
                >
                <p
                  id="settings-serial-archive-limit-hint"
                  class="settings-row__hint"
                >
                  {{ t("serialDebug.archiveLimit.hint") }}
                </p>
              </div>
              <TySelect
                id="settings-serial-archive-limit"
                :aria-label="t('serialDebug.archiveLimit.label')"
                aria-describedby="settings-serial-archive-limit-hint"
                v-model="archiveLimitValue"
                :options="archiveLimitOptions"
                :placeholder="
                  t('serialDebug.archiveLimit.size', {
                    mib: sd.archiveLimitMib,
                  })
                "
                class="settings-row__control settings-row__control--select"
                style="height: 2.5rem"
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
                <label for="settings-log-level" class="settings-row__title">{{
                  t("settings.logLevel")
                }}</label>
                <p id="settings-log-level-hint" class="settings-row__hint">
                  {{ t("settings.logLevelHint") }}
                </p>
              </div>
              <TySelect
                :model-value="settings.logLevel"
                :aria-label="t('settings.logLevel')"
                aria-describedby="settings-log-level-hint"
                id="settings-log-level"
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
                  <FontAwesomeIcon
                    :icon="['fas', 'file-lines']"
                    class="size-4"
                  />
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
        <section
          class="settings-update-center relative overflow-hidden rounded-2xl p-4 sm:p-5"
          aria-labelledby="update-center-heading"
        >
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
                <FontAwesomeIcon
                  :icon="['fas', 'chevron-right']"
                  class="size-4"
                />
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
              <span class="docs-cta__icon" aria-hidden="true">
                <FontAwesomeIcon
                  :icon="['fas', 'book-open']"
                  class="size-3.5"
                />
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
      </div>
    </div>
    <UpdateDialog :open="showUpdateDialog" @close="showUpdateDialog = false" />
    <LogViewerDialog :open="showLogViewer" @close="showLogViewer = false" />
  </div>
</template>

<style scoped>
.settings-layout {
  display: grid;
  grid-template-columns: 138px minmax(0, 1fr);
  gap: 24px;
  align-items: start;
}
.settings-nav {
  position: sticky;
  top: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.settings-nav a {
  padding: 10px 12px;
  border-radius: var(--ty-control-radius);
  color: var(--ty-text-muted);
  font-size: 13px;
  text-decoration: none;
}
.settings-nav a[aria-current="location"] {
  color: var(--ty-primary);
  background: var(--ty-primary-soft);
}
.settings-nav a:hover {
  color: var(--ty-text);
  background: var(--ty-primary-soft);
}
.settings-sections {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 20px;
}
.settings-sections > section {
  scroll-margin-top: 16px;
}
.settings-card-head {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.settings-card-subtitle,
.settings-row__hint,
.diagnostics-action-card__copy,
.settings-update-center__body,
.settings-update-center__control-hint,
.update-entry-card__subtitle {
  color: var(--ty-text-muted);
  font-size: 12px;
  line-height: 1.6;
}
.settings-row-group {
  display: flex;
  flex-direction: column;
}
.settings-row {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  padding: 16px 0;
}
.settings-row + .settings-row {
  border-top: 1px solid var(--ty-border);
}
.settings-row__copy {
  flex: 1;
  min-width: 0;
}
.settings-row__title {
  display: block;
  font-size: 13px;
  font-weight: 500;
  color: var(--ty-text);
}
.settings-row__hint {
  margin-top: 4px;
}
.settings-row__value {
  display: block;
  margin-top: 4px;
  white-space: nowrap;
  font-family: ui-monospace, monospace;
  font-size: 12px;
  color: var(--ty-text-muted);
}
.settings-row__control--select {
  width: 176px;
  flex-shrink: 0;
}
.settings-inline-action {
  flex-shrink: 0;
}
.path-scroll {
  scrollbar-width: thin;
}
.theme-options {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}
.theme-choice {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 10px;
  text-align: left;
  border: 1px solid var(--ty-border-strong);
  border-radius: var(--ty-control-radius);
  padding: 8px;
  color: var(--ty-text-muted);
  cursor: pointer;
  background: var(--ty-surface);
}
.theme-choice[aria-checked="true"] {
  border-color: var(--ty-primary);
  color: var(--ty-primary);
  box-shadow: inset 0 0 0 1px var(--ty-primary);
}
.theme-choice:hover {
  background: var(--ty-field);
}
.theme-choice-label {
  display: flex;
  gap: 8px;
  align-items: center;
  font-size: 12px;
  padding: 0 4px 4px;
}
.theme-preview {
  height: 72px;
  width: 100%;
  display: grid;
  grid-template-columns: 24% 1fr;
  gap: 8px;
  padding: 8px;
  background: #f1f2ef;
  border-radius: 3px;
  --mini-rail: #dce4dd;
  --mini-panel: #ffffff;
  --mini-accent: #006b5e;
}
.theme-preview--dark {
  background: #111518;
  --mini-rail: #232b30;
  --mini-panel: #1a2024;
  --mini-accent: #74ddc9;
}
.theme-preview--system {
  background: linear-gradient(110deg, #f1f2ef 50%, #111518 50%);
}
.theme-preview-side {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 5px;
  background: var(--mini-rail);
}
.theme-preview-side i {
  height: 3px;
  background: var(--mini-accent);
}
.theme-preview-main {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.theme-preview-main i {
  height: 4px;
  background: var(--mini-rail);
}
.theme-preview-main b {
  flex: 1;
  border: 1px solid var(--mini-rail);
  background: var(--mini-panel);
}
.settings-update-center {
  border: 1px solid var(--ty-border);
  border-radius: var(--ty-radius);
  background: var(--ty-surface);
}
.settings-update-center__layout {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.settings-update-center__headline-row {
  display: flex;
  justify-content: space-between;
  gap: 16px;
}
.settings-update-center__eyebrow,
.settings-update-center__version-label,
.update-entry-card__meta-label {
  font-size: 11px;
  color: var(--ty-text-muted);
}
.settings-update-center__title {
  font-size: 16px;
  font-weight: 600;
  margin: 5px 0;
}
.settings-update-center__version {
  text-align: right;
  flex-shrink: 0;
}
.settings-update-center__version-value {
  display: block;
  font:
    500 20px ui-monospace,
    monospace;
  color: var(--ty-text);
}
.settings-update-center__control {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 20px;
  border-top: 1px solid var(--ty-border);
  padding-top: 16px;
}
.settings-update-center__control-select {
  width: 176px;
  flex-shrink: 0;
}
.update-entry-card {
  display: flex;
  align-items: center;
  gap: 12px;
  border: 1px solid var(--ty-border);
  background: var(--ty-field);
  border-radius: var(--ty-control-radius);
  padding: 14px;
  text-align: left;
  cursor: pointer;
}
.update-entry-card:hover {
  background: var(--ty-surface-muted);
}
.update-entry-card__body {
  min-width: 0;
  flex: 1;
}
.update-entry-card__meta {
  display: flex;
  align-items: center;
  gap: 10px;
}
.update-entry-card__badge {
  color: var(--ty-primary);
  font:
    12px ui-monospace,
    monospace;
}
.update-entry-card__title {
  display: block;
  margin: 5px 0;
  font-size: 13px;
  font-weight: 600;
}
.update-entry-card__icon,
.update-entry-card__arrow {
  color: var(--ty-text-muted);
  flex-shrink: 0;
}
.diagnostics-actions {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
  padding-top: 12px;
}
.diagnostics-action-card {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  border: 1px solid var(--ty-border);
  background: var(--ty-field);
  border-radius: var(--ty-control-radius);
  padding: 12px;
  text-align: left;
  cursor: pointer;
}
.diagnostics-action-card:hover {
  border-color: var(--ty-border-strong);
}
.diagnostics-action-card__icon {
  color: var(--ty-text-muted);
}
.diagnostics-action-card__title {
  font-size: 13px;
  font-weight: 500;
}
.about-footer {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  margin-top: 16px;
}
.docs-cta {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-height: 34px;
  padding: 6px 12px;
  border: 1px solid var(--ty-primary);
  border-radius: var(--ty-control-radius);
  color: var(--ty-primary);
  background: var(--ty-primary-soft);
  font-size: 13px;
  cursor: pointer;
}
.docs-cta:hover {
  background: var(--ty-field);
}
.disclaimer-reset-feedback {
  font-size: 12px;
  color: var(--ty-success);
}
@media (max-width: 1100px) {
  .settings-layout {
    grid-template-columns: 120px minmax(0, 1fr);
    gap: 16px;
  }
  .theme-options {
    gap: 8px;
  }
}
@media (max-width: 800px) {
  .settings-layout {
    grid-template-columns: 1fr;
  }
  .settings-nav {
    position: static;
    flex-direction: row;
    flex-wrap: wrap;
  }
}
@media (max-width: 540px) {
  .theme-options {
    grid-template-columns: 1fr;
  }
  .theme-choice {
    flex-direction: row;
    align-items: center;
  }
  .theme-preview {
    width: 95px;
  }
  .settings-row,
  .settings-update-center__control {
    flex-wrap: wrap;
  }
  .settings-row__copy {
    flex-basis: 100%;
  }
  .diagnostics-actions {
    grid-template-columns: 1fr;
  }
}
</style>
