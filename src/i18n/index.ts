import { createI18n } from 'vue-i18n';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';
import { type LocalePreference, resolveLocale } from '../stores/settings';

const STORAGE_KEY = 'tyutool-locale';
const LEGACY_LOCALE_KEY = 'tyutools-locale';

export type LocaleId = 'zh-CN' | 'en';

function getStoredLocalePref(): LocalePreference {
  let s = localStorage.getItem(STORAGE_KEY);
  if (!s) {
    const legacy = localStorage.getItem(LEGACY_LOCALE_KEY);
    if (legacy === 'zh-CN' || legacy === 'en') {
      localStorage.setItem(STORAGE_KEY, legacy);
      localStorage.removeItem(LEGACY_LOCALE_KEY);
      s = legacy;
    }
  }
  if (s === 'zh-CN' || s === 'en' || s === 'auto') {
    return s as LocalePreference;
  }
  return 'auto';
}

export const i18n = createI18n({
  legacy: false,
  locale: resolveLocale(getStoredLocalePref()),
  fallbackLocale: 'en',
  messages: {
    'zh-CN': zhCN,
    en,
  },
});
