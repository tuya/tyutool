<!-- src/features/toolbox/components/ToolboxBreadcrumb.vue -->
<script setup lang="ts">
import { useI18n } from "vue-i18n";

defineProps<{ toolName: string }>();

const { t } = useI18n();
</script>

<template>
  <nav class="mb-1 flex items-center gap-1.5 text-xs" aria-label="breadcrumb">
    <RouterLink
      to="/toolbox"
      class="text-[var(--ty-text-muted)] transition-colors hover:text-[var(--ty-text)]"
    >
      {{ t("app.nav.toolbox") }}
    </RouterLink>
    <span class="text-[var(--ty-text-muted)]" aria-hidden="true">›</span>
    <span class="text-[var(--ty-text)]">{{ toolName }}</span>
  </nav>
</template>
