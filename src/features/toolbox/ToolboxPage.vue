<!-- src/features/toolbox/ToolboxPage.vue -->
<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import TyBatchFlashAuthIcon from "@/components/icons/TyBatchFlashAuthIcon.vue";
import { TOOLBOX_TOOLS } from "./tools";

const { t } = useI18n();

const tools = computed(() =>
  TOOLBOX_TOOLS.map((tool) => ({
    ...tool,
    name: t(tool.nameKey),
    desc: t(tool.descKey),
  })),
);
</script>

<template>
  <div class="flex min-w-0 flex-col gap-3 sm:gap-4">
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
          <FontAwesomeIcon :icon="['fas', 'toolbox']" class="size-5" />
        </div>
        <div class="min-w-0">
          <p class="page-header-section">{{ t("toolbox.section") }}</p>
          <h1 class="page-header-title mt-0.5">{{ t("app.nav.toolbox") }}</h1>
        </div>
        <div
          class="page-header-divider ml-1 hidden h-8 w-px shrink-0 sm:block"
          aria-hidden="true"
        />
        <p class="page-header-desc relative hidden max-w-[26rem] sm:block">
          {{ t("toolbox.subtitle") }}
        </p>
      </div>
    </header>

    <div class="grid min-w-0 gap-3 sm:grid-cols-2 sm:gap-4">
      <RouterLink
        v-for="tool in tools"
        :key="tool.to"
        :to="tool.to"
        class="ty-card group flex cursor-pointer items-center gap-4 rounded-xl p-4 transition-colors hover:bg-[var(--ty-surface-muted)]"
      >
        <div
          class="flex size-11 shrink-0 items-center justify-center rounded-xl bg-[color-mix(in_srgb,var(--ty-primary)_12%,transparent)] ring-1 ring-[color-mix(in_srgb,var(--ty-primary)_22%,transparent)]"
        >
          <TyBatchFlashAuthIcon
            v-if="tool.icon === 'batch-flash-auth'"
            class="size-5 text-[var(--ty-primary)]"
          />
        </div>
        <div class="min-w-0 flex-1">
          <p class="text-sm font-semibold text-[var(--ty-text)]">
            {{ tool.name }}
          </p>
          <p class="mt-0.5 text-xs leading-snug text-[var(--ty-text-muted)]">
            {{ tool.desc }}
          </p>
        </div>
        <FontAwesomeIcon
          :icon="['fas', 'chevron-right']"
          class="size-3.5 shrink-0 text-[var(--ty-text-muted)] transition-transform group-hover:translate-x-0.5"
          aria-hidden="true"
        />
      </RouterLink>
    </div>
  </div>
</template>
