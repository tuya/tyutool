import { createApp, watch } from 'vue';
import { createPinia } from 'pinia';
import { library } from '@fortawesome/fontawesome-svg-core';
import { FontAwesomeIcon } from '@fortawesome/vue-fontawesome';
import {
  faArrowsRotate,
  faChevronDown,
  faCircle,
  faCircleArrowUp,
  faCircleCheck,
  faCircleExclamation,
  faCircleInfo,
  faCircleXmark,
  faCopy,
  faFolderOpen,
  faGear,
  faKey,
  faMicrochip,
  faPlug,
  faPlugCircleXmark,
  faPlus,
  faPowerOff,
  faScroll,
  faTerminal,
  faTrash,
  faTriangleExclamation,
  faXmark,
} from '@fortawesome/free-solid-svg-icons';
import '@fontsource-variable/inter/index.css';
import './assets/main.css';
import App from './App.vue';
import { i18n } from './i18n';
import { router } from './router';
import { useSettingsStore, resolveLocale } from './stores/settings';

library.add(
  faArrowsRotate,
  faChevronDown,
  faCircle,
  faCircleArrowUp,
  faCircleCheck,
  faCircleExclamation,
  faCircleInfo,
  faCircleXmark,
  faCopy,
  faFolderOpen,
  faGear,
  faKey,
  faMicrochip,
  faPlug,
  faPlugCircleXmark,
  faPlus,
  faPowerOff,
  faScroll,
  faTerminal,
  faTrash,
  faTriangleExclamation,
  faXmark
);

const app = createApp(App);
const pinia = createPinia();

app.component('FontAwesomeIcon', FontAwesomeIcon);
app.use(pinia);
app.use(i18n);
app.use(router);

// Initialize settings store (theme + locale side-effects)
const settings = useSettingsStore();
settings.init();

// Sync i18n locale with settings store on startup and change
// settings.locale may be 'auto' | 'zh-CN' | 'en'; resolve to concrete locale
watch(
  () => settings.locale,
  v => {
    const resolved = resolveLocale(v);
    i18n.global.locale.value = resolved;
    document.documentElement.lang = resolved === 'zh-CN' ? 'zh-CN' : 'en';
  },
  { immediate: true }
);

app.mount('#app');

// Scan serial ports once at startup, after settings (including locale) are fully loaded
// so that log messages use the correct language from the start.
import { useFlashStore } from './stores/flash';
void settings.ready().then(async () => {
  const flash = useFlashStore();
  await flash.loadWorkspace();
  flash.startWorkspacePersistence();
  void flash.refreshDevice();
});
