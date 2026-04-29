<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { APP_VERSION } from '@/config/app';
import { desktopAppLogDirHint } from '@/config/tauri-desktop-paths';
import { useSettingsStore, resolveLocale } from '@/stores/settings';
import type { LogLevelId, LocalePreference } from '@/stores/settings';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { showConfirmDialog } from '@/composables/confirmDialog';
import UpdateDialog from './UpdateDialog.vue';
import TySelect, { type TySelectOption } from '@/components/TySelect.vue';

const { locale, t } = useI18n();
const settings = useSettingsStore();

const appVersion = APP_VERSION;
const showUpdateDialog = ref(false);

const logToggleOptions = computed(() => [
  { value: true, label: t('settings.logOn') },
  { value: false, label: t('settings.logOff') },
]);

const logLevelOptions = computed(() => [
  { value: 'error', label: 'Error' },
  { value: 'warn', label: 'Warn' },
  { value: 'info', label: 'Info' },
  { value: 'debug', label: 'Debug' },
  { value: 'trace', label: 'Trace' },
]);

const localeOptions = computed<TySelectOption[]>(() => [
  { value: 'auto', label: t('settings.langAuto') },
  { value: 'zh-CN', label: t('settings.langZh') },
  { value: 'en', label: t('settings.langEn') },
]);

const localeValue = computed({
  get: () => settings.locale,
  set: (val: string) => {
    settings.setLocale(val as LocalePreference);
    locale.value = resolveLocale(val as LocalePreference);
  },
});

// Sync vue-i18n locale when settings locale changes (e.g. from Tauri store load)
watch(
  () => settings.locale,
  pref => {
    locale.value = resolveLocale(pref);
  }
);

async function openLogsFolder(): Promise<void> {
  if (!isTauriRuntime() && import.meta.env.DEV) {
    try {
      const res = await fetch('/__dev/open-app-log-dir', { method: 'POST' });
      const text = await res.text();
      let data: { ok?: boolean; error?: string };
      try {
        data = JSON.parse(text) as { ok?: boolean; error?: string };
      } catch {
        await showConfirmDialog({
          title: t('settings.logsFolderWebDevTitle'),
          message: t('settings.logsFolderDevOpenFailed', {
            detail: text.slice(0, 200),
          }),
          kind: 'info',
          okLabel: t('settings.logsFolderWebDevOk'),
          showCancel: false,
        });
        return;
      }
      if (res.ok && data.ok) {
        return;
      }
      await showConfirmDialog({
        title: t('settings.logsFolderWebDevTitle'),
        message: t('settings.logsFolderDevOpenFailed', {
          detail: data.error ?? `${res.status}`,
        }),
        kind: 'info',
        okLabel: t('settings.logsFolderWebDevOk'),
        showCancel: false,
      });
    } catch (e) {
      await showConfirmDialog({
        title: t('settings.logsFolderWebDevTitle'),
        message: t('settings.logsFolderDevOpenFailed', {
          detail: e instanceof Error ? e.message : String(e),
        }),
        kind: 'info',
        okLabel: t('settings.logsFolderWebDevOk'),
        showCancel: false,
      });
    }
    return;
  }
  if (!isTauriRuntime()) {
    await showConfirmDialog({
      title: t('settings.logsFolderWebDevTitle'),
      message: t('settings.logsFolderWebDevMessage', { path: desktopAppLogDirHint() }),
      kind: 'info',
      okLabel: t('settings.logsFolderWebDevOk'),
      showCancel: false,
    });
    return;
  }
  try {
    const { appLogDir } = await import('@tauri-apps/api/path');
    const { revealItemInDir } = await import('@tauri-apps/plugin-opener');
    const dir = await appLogDir();
    const { info } = await import('@tauri-apps/plugin-log');
    await info(`openLogsFolder: dir=${dir}`);
    await revealItemInDir(dir);
    await info('openLogsFolder: revealItemInDir returned OK');
  } catch (e) {
    const { error: logError } = await import('@tauri-apps/plugin-log');
    await logError(`openLogsFolder failed: ${e}`);
  }
}

async function openOpensourceLicenses(): Promise<void> {
  try {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl('https://github.com/tuya/tyutool/blob/main/LICENSE');
  } catch (_e) {
    window.open('https://github.com/tuya/tyutool/blob/main/LICENSE', '_blank');
  }
}
</script>

<template>
  <div class="flex min-h-0 min-w-0 flex-col gap-3 sm:gap-4">
    <header
      class="page-header relative flex min-w-0 flex-col gap-3 overflow-hidden p-3 sm:flex-row sm:items-center sm:p-3.5"
    >
      <div class="page-header-bg pointer-events-none absolute inset-0" aria-hidden="true" />
      <div class="relative flex min-w-0 flex-1 items-center gap-3">
        <div class="page-header-icon flex size-10 shrink-0 items-center justify-center rounded-xl" aria-hidden="true">
          <FontAwesomeIcon :icon="['fas', 'gear']" class="size-5" />
        </div>
        <div class="min-w-0">
          <p class="page-header-section">{{ t('settings.section') }}</p>
          <h1 class="page-header-title mt-0.5">{{ t('settings.title') }}</h1>
        </div>
        <div class="page-header-divider ml-1 hidden h-8 w-px shrink-0 sm:block" aria-hidden="true" />
        <p class="page-header-desc relative hidden max-w-[26rem] sm:block">
          {{ t('settings.subtitle') }}
        </p>
      </div>
    </header>

    <div class="grid min-w-0 grid-cols-1 gap-3 md:grid-cols-2 md:items-stretch md:gap-4">
      <section class="ty-card min-w-0 rounded-xl p-4 sm:p-5" aria-labelledby="appearance-heading">
        <h2 id="appearance-heading" class="ty-section-title">
          {{ t('settings.appearance') }}
        </h2>
        <fieldset class="mt-4 space-y-3">
          <legend class="sr-only">{{ t('settings.themeLegend') }}</legend>
          <div class="space-y-2">
            <div class="text-sm font-medium text-[var(--ty-text)]">
              {{ t('settings.theme') }}
            </div>
            <div class="flex flex-wrap gap-x-5 gap-y-3">
              <label class="flex cursor-pointer items-center gap-2.5 text-sm text-[var(--ty-text)]">
                <input v-model="settings.theme" type="radio" name="theme" value="light" class="size-4 shrink-0" />
                {{ t('settings.themeLight') }}
              </label>
              <label class="flex cursor-pointer items-center gap-2.5 text-sm text-[var(--ty-text)]">
                <input v-model="settings.theme" type="radio" name="theme" value="dark" class="size-4 shrink-0" />
                {{ t('settings.themeDark') }}
              </label>
              <label class="flex cursor-pointer items-center gap-2.5 text-sm text-[var(--ty-text)]">
                <input v-model="settings.theme" type="radio" name="theme" value="system" class="size-4 shrink-0" />
                {{ t('settings.themeSystem') }}
              </label>
            </div>
          </div>
        </fieldset>
        <div class="mt-6 space-y-2">
          <label for="settings-locale" class="block text-sm font-medium text-[var(--ty-text)]">{{
            t('settings.language')
          }}</label>
          <TySelect
            id="settings-locale"
            v-model="localeValue"
            :options="localeOptions"
            class="w-full max-w-md"
            style="height: 2.5rem"
          />
          <p class="text-xs text-[var(--ty-text-muted)]">
            {{ t('settings.languageHint') }}
          </p>
        </div>
      </section>

      <section class="ty-card min-w-0 rounded-xl p-4 sm:p-5" aria-labelledby="app-heading">
        <h2 id="app-heading" class="ty-section-title">
          {{ t('settings.appSection') }}
        </h2>
        <div class="mt-4 space-y-4">
          <!-- Debug Log toggle -->
          <div class="flex items-center justify-between">
            <label class="ty-label">{{ t('settings.logEnabled') }}</label>
            <div class="flex gap-2">
              <button
                v-for="opt in logToggleOptions"
                :key="String(opt.value)"
                class="ty-btn-sm"
                :class="settings.logEnabled === opt.value ? 'ty-btn-toggle-active' : 'ty-btn-secondary'"
                @click="settings.setLogEnabled(opt.value)"
              >
                {{ opt.label }}
              </button>
            </div>
          </div>

          <!-- Log Level select -->
          <div class="flex items-center justify-between">
            <div>
              <label class="ty-label">{{ t('settings.logLevel') }}</label>
              <p v-if="!settings.logEnabled" class="text-xs text-base-content/50 mt-0.5">
                {{ t('settings.logLevelHint') }}
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
            <label class="ty-label">{{ t('settings.logsFolder') }}</label>
            <button type="button" class="ty-btn-sm ty-btn-secondary" @click="openLogsFolder">
              <FontAwesomeIcon :icon="['fas', 'folder-open']" class="mr-1.5 size-3.5" aria-hidden="true" />
              {{ t('settings.logsFolder') }}
            </button>
          </div>
        </div>
      </section>
    </div>

    <section class="ty-card min-w-0 rounded-xl p-4 sm:p-5" aria-labelledby="about-heading">
      <h2 id="about-heading" class="ty-section-title">
        {{ t('settings.about') }}
      </h2>
      <p class="mt-3 text-sm text-[var(--ty-text)]">
        {{ t('settings.version', { version: appVersion }) }}
      </p>
      <div class="mt-4 flex min-w-0 flex-col gap-2 sm:flex-row sm:flex-wrap">
        <button
          type="button"
          class="ty-btn-secondary inline-flex min-h-11 w-full justify-center rounded-xl px-4 sm:w-auto"
          @click="openOpensourceLicenses"
        >
          {{ t('settings.opensource') }}
        </button>
        <button
          v-if="isTauriRuntime()"
          type="button"
          class="ty-btn-secondary inline-flex min-h-11 w-full items-center justify-center gap-2 rounded-xl px-4 sm:w-auto"
          @click="showUpdateDialog = true"
        >
          <FontAwesomeIcon :icon="['fas', 'arrows-rotate']" class="size-4" aria-hidden="true" />
          {{ t('settings.checkUpdate') }}
        </button>
      </div>
    </section>

    <UpdateDialog :open="showUpdateDialog" @close="showUpdateDialog = false" />
  </div>
</template>
