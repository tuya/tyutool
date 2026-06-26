<!-- src/features/batch-flash-auth/BatchFlashAuthPage.vue -->
<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
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

// The Config + AuthConfig pair is auto-collapsed when a batch starts so
// the live SlotList grid gets the room; users can manually re-open while
// running. Default open so first-time users see the fields.
const configOpen = ref(true);

watch(
  () => store.isBusy,
  (busy) => {
    if (busy) configOpen.value = false;
  },
);

onMounted(async () => {
  await store.loadPersistedData();
  await store.ensureListener();
});

onUnmounted(() => {
  store.cleanup();
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

    <!-- Collapsible configuration disclosure: shrinks out of the way once
         a batch is running so the slot grid dominates the viewport. -->
    <section
      class="ty-card overflow-hidden rounded-xl"
      aria-labelledby="batch-config-heading"
    >
      <button
        type="button"
        class="flex w-full items-center gap-3 px-3.5 py-2.5 text-left transition-colors hover:bg-[var(--ty-surface-muted)]"
        :aria-expanded="configOpen"
        aria-controls="batch-config-panel"
        @click="configOpen = !configOpen"
      >
        <FontAwesomeIcon
          :icon="['fas', 'chevron-down']"
          class="size-4 shrink-0 text-[var(--ty-text-muted)] transition-transform duration-200"
          :class="configOpen ? '' : '-rotate-90'"
          aria-hidden="true"
        />
        <div class="min-w-0 flex-1">
          <h2
            id="batch-config-heading"
            class="ty-section-title text-[var(--ty-primary)]"
          >
            {{ t("batchFlashAuth.configurationSection") }}
          </h2>
          <p class="mt-0.5 text-xs text-[var(--ty-text-muted)]">
            {{ t("batchFlashAuth.configurationHint") }}
          </p>
        </div>
        <span
          class="text-xs font-medium text-[var(--ty-text-muted)]"
          aria-hidden="true"
          >{{
            configOpen
              ? t("batchFlashAuth.collapse")
              : t("batchFlashAuth.expand")
          }}</span
        >
      </button>
      <div
        v-show="configOpen"
        id="batch-config-panel"
        class="space-y-3 border-t border-[var(--ty-border)] p-3.5"
      >
        <BatchFlashAuthConfig />
        <BatchAuthConfig />
      </div>
    </section>

    <BatchFlashAuthToolbar />
    <BatchFlashAuthSlotList class="min-h-0 flex-1" />
  </div>
</template>
