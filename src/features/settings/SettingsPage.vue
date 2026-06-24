<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useI18n } from "vue-i18n";
import { APP_VERSION } from "@/config/app";
import { useSettingsStore, resolveLocale } from "@/stores/settings";
import { useSerialDebugStore } from "@/stores/serial-debug";
import type {
  LogLevelId,
  LocalePreference,
  // ThemeStyle,
} from "@/stores/settings";
import { isTauriRuntime } from "@/runtime";
import { openLogsFolder as openLogsFolderAction } from "./open-logs-folder";
import UpdateDialog from "./UpdateDialog.vue";
import LogViewerDialog from "./LogViewerDialog.vue";
import TySelect, { type TySelectOption } from "@/components/TySelect.vue";

const { locale, t } = useI18n();
const settings = useSettingsStore();
const sd = useSerialDebugStore();

const appVersion = APP_VERSION;
const showUpdateDialog = ref(false);
const showLogViewer = ref(false);

const logToggleOptions = computed(() => [
  { value: true, label: t("settings.logOn") },
  { value: false, label: t("settings.logOff") },
]);

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

const localeValue = computed({
  get: () => settings.locale,
  set: (val: string) => {
    settings.setLocale(val as LocalePreference);
    locale.value = resolveLocale(val as LocalePreference);
  },
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

async function toggleAutoSave(): Promise<void> {
  if (!sd.autoSave && !sd.autoSaveDir) {
    sd.autoSave = true;
    await sd.pickAutoSaveDir();
  } else {
    sd.autoSave = !sd.autoSave;
  }
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

    <div
      class="grid min-w-0 grid-cols-1 gap-3 md:grid-cols-2 md:items-stretch md:gap-4"
    >
      <section
        class="ty-card min-w-0 rounded-xl p-4 sm:p-5"
        aria-labelledby="appearance-heading"
      >
        <h2 id="appearance-heading" class="ty-section-title">
          {{ t("settings.appearance") }}
        </h2>
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
        <div class="mt-6 space-y-2">
          <label
            for="settings-locale"
            class="block text-sm font-medium text-[var(--ty-text)]"
            >{{ t("settings.language") }}</label
          >
          <TySelect
            id="settings-locale"
            v-model="localeValue"
            :options="localeOptions"
            class="w-full max-w-md"
            style="height: 2.5rem"
          />
          <p class="text-xs text-[var(--ty-text-muted)]">
            {{ t("settings.languageHint") }}
          </p>
        </div>
      </section>

      <section
        class="ty-card min-w-0 rounded-xl p-4 sm:p-5"
        aria-labelledby="app-heading"
      >
        <h2 id="app-heading" class="ty-section-title">
          {{ t("settings.appSection") }}
        </h2>
        <div class="mt-4 space-y-4">
          <!-- Debug Log toggle -->
          <div class="flex items-center justify-between">
            <label class="ty-label">{{ t("settings.logEnabled") }}</label>
            <div class="flex gap-2">
              <button
                v-for="opt in logToggleOptions"
                :key="String(opt.value)"
                class="ty-btn-sm"
                :class="
                  settings.logEnabled === opt.value
                    ? opt.value
                      ? 'ty-btn-toggle-active'
                      : 'ty-btn-toggle-active-off'
                    : 'ty-btn-secondary'
                "
                @click="settings.setLogEnabled(opt.value)"
              >
                {{ opt.label }}
              </button>
            </div>
          </div>

          <!-- Log Level select -->
          <div class="flex items-center justify-between">
            <div>
              <label class="ty-label">{{ t("settings.logLevel") }}</label>
              <p
                v-if="!settings.logEnabled"
                class="text-xs text-base-content/50 mt-0.5"
              >
                {{ t("settings.logLevelHint") }}
              </p>
            </div>
            <TySelect
              :model-value="settings.logLevel"
              :options="logLevelOptions"
              :disabled="!settings.logEnabled"
              class="w-auto min-w-[8.5rem]"
              @update:model-value="settings.setLogLevel($event as LogLevelId)"
            />
          </div>

          <!-- Open log folder -->
          <div class="flex items-center justify-between">
            <label class="ty-label">{{ t("settings.logsFolder") }}</label>
            <button
              type="button"
              class="ty-btn-sm ty-btn-secondary"
              @click="openLogsFolder"
            >
              <FontAwesomeIcon
                :icon="['fas', 'folder-open']"
                class="mr-1.5 size-3.5"
                aria-hidden="true"
              />
              {{ t("settings.logsFolder") }}
            </button>
          </div>

          <!-- View logs in-app -->
          <div class="flex items-center justify-between">
            <label class="ty-label">{{ t("settings.viewLogs") }}</label>
            <button
              type="button"
              class="ty-btn-sm ty-btn-secondary"
              @click="showLogViewer = true"
            >
              <FontAwesomeIcon
                :icon="['fas', 'file-lines']"
                class="mr-1.5 size-3.5"
                aria-hidden="true"
              />
              {{ t("settings.viewLogs") }}
            </button>
          </div>
        </div>
      </section>
    </div>

    <section
      class="ty-card min-w-0 rounded-xl p-4 sm:p-5"
      aria-labelledby="serial-debug-heading"
    >
      <h2 id="serial-debug-heading" class="ty-section-title">
        {{ t("settings.serialLogsSection") }}
      </h2>
      <div class="mt-4 space-y-4">
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <label class="ty-label">{{
              t("serialDebug.autoSave.label")
            }}</label>
            <p class="mt-0.5 text-xs text-[var(--ty-text-muted)]">
              {{ t("serialDebug.autoSave.description") }}
            </p>
          </div>
          <input
            type="checkbox"
            :checked="sd.autoSave"
            class="size-4 shrink-0 cursor-pointer"
            @change="toggleAutoSave"
          />
        </div>
        <div class="flex items-center justify-between gap-4">
          <div class="flex min-w-0 flex-1 items-center gap-2">
            <label class="ty-label shrink-0">{{
              t("serialDebug.autoSave.dirLabel")
            }}</label>
            <div class="path-scroll min-w-0 flex-1 overflow-x-auto">
              <span
                v-if="sd.autoSaveDir"
                class="whitespace-nowrap text-xs text-[var(--ty-text-muted)]"
                >{{ sd.autoSaveDir }}</span
              >
            </div>
          </div>
          <button
            type="button"
            class="ty-btn-sm ty-btn-secondary shrink-0"
            @click="sd.pickAutoSaveDir()"
          >
            {{ t("serialDebug.autoSave.pickDir") }}
          </button>
        </div>
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <label class="ty-label">{{
              t("serialDebug.autoSave.timestamp")
            }}</label>
            <span class="font-mono text-xs text-[var(--ty-text-muted)]">
              {{
                sd.autoSaveTimestamp
                  ? t("serialDebug.autoSave.timestampFmtOn")
                  : t("serialDebug.autoSave.timestampFmtOff")
              }}
            </span>
          </div>
          <input
            type="checkbox"
            v-model="sd.autoSaveTimestamp"
            class="size-4 cursor-pointer"
          />
        </div>
      </div>
    </section>

    <section
      class="ty-card min-w-0 rounded-xl p-4 sm:p-5"
      aria-labelledby="about-heading"
    >
      <h2 id="about-heading" class="ty-section-title">
        {{ t("settings.about") }}
      </h2>
      <p class="mt-3 text-sm text-[var(--ty-text)]">
        {{ t("settings.version", { version: appVersion }) }}
      </p>
      <div class="mt-4 flex min-w-0 flex-col gap-2 sm:flex-row sm:flex-wrap">
        <button
          type="button"
          class="ty-btn-secondary inline-flex min-h-11 w-full justify-center rounded-xl px-4 sm:w-auto"
          @click="openOpensourceLicenses"
        >
          {{ t("settings.opensource") }}
        </button>
        <button
          v-if="isTauriRuntime()"
          type="button"
          class="ty-btn-secondary inline-flex min-h-11 w-full items-center justify-center gap-2 rounded-xl px-4 sm:w-auto"
          @click="showUpdateDialog = true"
        >
          <FontAwesomeIcon
            :icon="['fas', 'arrows-rotate']"
            class="size-4"
            aria-hidden="true"
          />
          {{ t("settings.checkUpdate") }}
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

/* "关闭" active state — danger red, distinct from primary "开启" */
.ty-btn-toggle-active-off {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 0.5rem;
  min-height: 2.75rem;
  border-radius: 0.5rem;
  padding: 0.5rem 0.75rem;
  font-size: 0.875rem;
  font-weight: 600;
  cursor: pointer;
  color: #fff;
  border: 1px solid var(--ty-danger);
  background-color: var(--ty-danger);
  box-shadow:
    0 1px 2px rgba(15, 23, 42, 0.08),
    0 2px 8px color-mix(in srgb, var(--ty-danger) 24%, transparent);
  transition:
    background-color 0.18s ease,
    border-color 0.18s ease,
    box-shadow 0.18s ease;
}
.ty-btn-toggle-active-off:hover:not(:disabled) {
  background-color: color-mix(in srgb, var(--ty-danger) 85%, #000);
  border-color: color-mix(in srgb, var(--ty-danger) 85%, #000);
}
</style>
