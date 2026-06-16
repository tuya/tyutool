<!-- src/features/batch-flash-auth/BatchFlashAuthPage.vue -->
<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { useI18n } from "vue-i18n";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import BatchFlashAuthDashboard from "./components/BatchFlashAuthDashboard.vue";
import BatchFlashAuthConfig from "./components/BatchFlashAuthConfig.vue";
import BatchAuthConfig from "./components/BatchAuthConfig.vue";
import BatchFlashAuthToolbar from "./components/BatchFlashAuthToolbar.vue";
import BatchFlashAuthSlotList from "./components/BatchFlashAuthSlotList.vue";
import ToolboxBreadcrumb from "@/features/toolbox/components/ToolboxBreadcrumb.vue";

const { t } = useI18n();
const store = useBatchFlashAuthStore();

onMounted(async () => {
  await store.loadPersistedData();
  await store.ensureListener();
});

onUnmounted(() => {
  if (!store.isBusy) store.cleanup();
});
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-3">
    <div>
      <ToolboxBreadcrumb :toolName="t('toolbox.batchFlashAuth.name')" />
      <h1 class="text-lg font-semibold text-[var(--ty-text)]">
        {{ t("batchFlashAuth.title") }}
      </h1>
      <p class="text-xs text-[var(--ty-text-muted)]">
        {{ t("batchFlashAuth.subtitle") }}
      </p>
    </div>

    <BatchFlashAuthDashboard />
    <BatchFlashAuthConfig />
    <BatchAuthConfig />
    <BatchFlashAuthToolbar />
    <BatchFlashAuthSlotList class="min-h-0 flex-1" />
  </div>
</template>
