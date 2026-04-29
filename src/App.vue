<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { RouterLink, RouterView, useRoute } from 'vue-router';
import TyConfirmDialog from '@/components/TyConfirmDialog.vue';
import TyToast from '@/components/TyToast.vue';
import { useAutoUpdate } from '@/composables/useAutoUpdate';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { APP_VERSION } from '@/config/app';
import { rLog } from '@/utils/log';
import appLogo from '@/assets/logo.png';

const route = useRoute();
const { t } = useI18n();
useAutoUpdate();

onMounted(() => {
  rLog.info(`[Frontend] tyutool v${APP_VERSION} initialized`);
  rLog.info(
    `[Frontend] Platform: ${navigator.platform}, Lang: ${navigator.language}, Tauri: ${isTauriRuntime()}`
  );
});

const fullBleedMain = computed(() => route.meta.layout === 'fullBleed');

const nav = computed(() => [
  {
    name: 'flash' as const,
    to: '/flash',
    label: t('app.nav.flash'),
    faIcon: ['fas', 'microchip'] as [string, string],
  },
  {
    name: 'settings' as const,
    to: '/settings',
    label: t('app.nav.settings'),
    faIcon: ['fas', 'gear'] as [string, string],
  },
]);
</script>

<template>
  <div
    class="flex h-dvh max-h-dvh min-w-0 flex-col overflow-hidden md:flex-row"
    :style="{ color: 'var(--ty-text)', backgroundColor: 'var(--ty-canvas)' }"
  >
    <aside
      class="flex w-full min-w-0 shrink-0 flex-col border-[var(--ty-border)] bg-[var(--ty-surface)] md:h-full md:w-[15.5rem] md:max-h-none md:border-b-0 md:border-r"
      :aria-label="t('app.mainNav')"
    >
      <div class="border-b border-[var(--ty-border)] px-4 py-3.5 md:px-4 md:py-5">
        <div class="flex items-center gap-3">
          <div class="flex size-10 shrink-0 items-center justify-center" aria-hidden="true">
            <img :src="appLogo" alt="tyutool logo" class="size-10 rounded-xl" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="truncate text-sm font-semibold tracking-tight">tyutool</div>
          </div>
        </div>
      </div>
      <nav class="flex min-w-0 flex-row gap-1 p-2 md:flex-1 md:flex-col md:gap-1 md:overflow-visible" role="navigation">
        <RouterLink
          v-for="item in nav"
          :key="item.to"
          :to="item.to"
          class="flex min-h-11 min-w-0 flex-1 cursor-pointer items-center justify-center gap-2 rounded-xl px-3 py-2.5 text-sm font-medium transition-colors duration-200 md:flex-none md:justify-start md:border-l-[3px] md:border-transparent md:pl-2.5"
          :class="
            route.path === item.to
              ? 'bg-[color-mix(in_srgb,var(--ty-primary)_18%,transparent)] font-semibold text-[var(--ty-primary)] shadow-sm ring-1 ring-[color-mix(in_srgb,var(--ty-primary)_32%,transparent)] md:border-[var(--ty-primary)]'
              : 'text-[var(--ty-text-muted)] hover:bg-[var(--ty-surface-muted)] hover:text-[var(--ty-text)]'
          "
          :aria-current="route.path === item.to ? 'page' : undefined"
        >
          <FontAwesomeIcon :icon="item.faIcon" class="size-5 shrink-0" aria-hidden="true" />
          <span class="min-w-0 truncate">{{ item.label }}</span>
        </RouterLink>
      </nav>
    </aside>
    <main
      class="main-scroll min-h-0 min-w-0 flex-1 overflow-x-hidden"
      :class="
        fullBleedMain
          ? 'flex flex-col px-3 py-2 sm:px-4 sm:py-3 md:px-5 md:py-3 max-lg:overflow-y-auto lg:overflow-hidden'
          : 'overflow-y-auto px-4 py-5 sm:px-6 sm:py-6 md:px-8 md:py-8'
      "
      role="main"
      tabindex="-1"
    >
      <div class="mx-auto w-full min-w-0 max-w-5xl" :class="fullBleedMain ? 'flex min-h-0 flex-1 flex-col' : ''">
        <transition name="ty-route" mode="out-in">
          <div
            :key="route.fullPath"
            class="min-w-0"
            :class="fullBleedMain ? 'flex min-h-0 w-full flex-1 flex-col' : ''"
          >
            <RouterView />
          </div>
        </transition>
      </div>
    </main>
    <TyConfirmDialog />
    <TyToast />
  </div>
</template>

<style scoped>
.main-scroll {
  scrollbar-gutter: stable;
}
</style>
