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
  <div>
    <h1 class="text-lg font-semibold text-[var(--ty-text)]">
      {{ t("app.nav.toolbox") }}
    </h1>
    <div class="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
      <RouterLink
        v-for="tool in tools"
        :key="tool.to"
        :to="tool.to"
        class="flex cursor-pointer items-center gap-4 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-4 transition-colors hover:bg-[var(--ty-surface-muted)]"
      >
        <div
          class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-[color-mix(in_srgb,var(--ty-primary)_12%,transparent)]"
        >
          <TyBatchFlashAuthIcon
            v-if="tool.icon === 'batch-flash-auth'"
            class="size-5 text-[var(--ty-primary)]"
          />
        </div>
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium text-[var(--ty-text)]">
            {{ tool.name }}
          </p>
          <p class="mt-0.5 text-xs leading-snug text-[var(--ty-text-muted)]">
            {{ tool.desc }}
          </p>
        </div>
        <FontAwesomeIcon
          :icon="['fas', 'chevron-right']"
          class="size-3.5 shrink-0 text-[var(--ty-text-muted)]"
          aria-hidden="true"
        />
      </RouterLink>
    </div>
  </div>
</template>
