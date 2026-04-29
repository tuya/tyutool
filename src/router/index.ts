import { createRouter, createWebHistory } from 'vue-router';
import FirmwareFlashPage from '@/features/firmware-flash/FirmwareFlashPage.vue';
import { SettingsPage } from '@/features/settings';

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: '/', redirect: '/flash' },
    {
      path: '/flash',
      name: 'flash',
      component: FirmwareFlashPage,
      meta: { title: '固件烧录', layout: 'fullBleed' },
    },
    {
      path: '/settings',
      name: 'settings',
      component: SettingsPage,
      meta: { title: '设置', layout: 'default' },
    },
  ],
});

router.afterEach(to => {
  const base = 'tyutool';
  const t = to.meta.title;
  document.title = t ? `${t} · ${base}` : base;
});
