import { createApp, watch } from 'vue';
import { createPinia } from 'pinia';
import '@fontsource-variable/inter/index.css';
import './assets/main.css';
import App from './App.vue';
import { bootstrapApp } from './app-init';
import { i18n } from './i18n';
import { registerFontAwesome } from './icons';
import { router } from './router';
import { useSettingsStore, resolveLocale } from './stores/settings';

const app = createApp(App);
const pinia = createPinia();

registerFontAwesome(app);
app.use(pinia);
app.use(i18n);
app.use(router);

const settings = useSettingsStore();
settings.init();

watch(
  () => settings.locale,
  v => {
    const resolved = resolveLocale(v);
    i18n.global.locale.value = resolved;
    document.documentElement.lang = resolved === 'zh-CN' ? 'zh-CN' : 'en';
  },
  { immediate: true },
);

app.mount('#app');
void bootstrapApp();
