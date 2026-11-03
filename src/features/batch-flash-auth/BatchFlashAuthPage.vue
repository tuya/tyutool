<!-- src/features/batch-flash-auth/BatchFlashAuthPage.vue -->
<script setup lang="ts">
import TyPageHeader from "@/components/TyPageHeader.vue";
import { computed, onMounted, onUnmounted, ref, unref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { onBeforeRouteLeave, useRouter } from "vue-router";
import TyPortActivityIndicator from "@/components/TyPortActivityIndicator.vue";
import { useFeaturePortIndicators } from "@/features/serial-port-indicators/useFeaturePortIndicators";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import BatchFlashAuthDashboard from "./components/BatchFlashAuthDashboard.vue";
import BatchFlashAuthConfig from "./components/BatchFlashAuthConfig.vue";
import BatchAuthConfig from "./components/BatchAuthConfig.vue";
import BatchFlashAuthToolbar from "./components/BatchFlashAuthToolbar.vue";
import BatchFlashAuthSlotList from "./components/BatchFlashAuthSlotList.vue";
import DisclaimerModal from "./components/DisclaimerModal.vue";
import { DISCLAIMER_KEY } from "./disclaimer";

const { t } = useI18n();
const store = useBatchFlashAuthStore();
const portIndicators = useFeaturePortIndicators();
const indicatorPaletteMode = computed(() => unref(portIndicators.paletteMode));
const indicatorActivePorts = computed(() => unref(portIndicators.activePorts));

const router = useRouter();
const pageLocked = ref(false);

onBeforeRouteLeave(() => !pageLocked.value);

const showDisclaimer = ref(false);

function onDisclaimerConfirm(dontShowAgain: boolean) {
  if (dontShowAgain) {
    localStorage.setItem(DISCLAIMER_KEY, "true");
  }
  showDisclaimer.value = false;
}

function onDisclaimerCancel() {
  router.push("/toolbox");
}

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
  if (localStorage.getItem(DISCLAIMER_KEY) !== "true") {
    showDisclaimer.value = true;
  }
  await store.loadPersistedData();
  await store.ensureListener();
  await store.autoAssign();
});

onUnmounted(() => {
  if (!store.isBusy) store.cleanup();
});
</script>

<template>
  <div class="flex flex-col gap-3">
    <DisclaimerModal
      :show="showDisclaimer"
      @confirm="onDisclaimerConfirm"
      @cancel="onDisclaimerCancel"
    />
    <TyPageHeader :title="t('batchFlashAuth.title')">
      <template #actions>
        <TyPortActivityIndicator
          :indicator="portIndicators.indicatorForFeature('toolbox')"
          :active-ports="indicatorActivePorts"
          :palette-mode="indicatorPaletteMode"
          feature="toolbox"
          surface="batch-flash-auth-header"
        />
        <button
          type="button"
          class="inline-flex items-center gap-2 px-3 py-2 text-sm"
          :class="pageLocked ? 'ty-btn-primary-solid' : 'ty-btn-secondary'"
          :aria-pressed="pageLocked"
          :title="
            pageLocked
              ? t('batchFlashAuth.pageLockedHint')
              : t('batchFlashAuth.lockPage')
          "
          @click="pageLocked = !pageLocked"
        >
          <FontAwesomeIcon
            :icon="['fas', pageLocked ? 'lock' : 'lock-open']"
            class="size-3.5"
            aria-hidden="true"
          />
          {{
            pageLocked
              ? t("batchFlashAuth.unlockPage")
              : t("batchFlashAuth.lockPage")
          }}
        </button>
      </template>
    </TyPageHeader>

    <BatchFlashAuthDashboard />

    <div class="sticky top-0 z-10 bg-[var(--ty-canvas)] py-2">
      <BatchFlashAuthToolbar />
    </div>

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
        class="space-y-2 border-t border-[var(--ty-border)] p-2.5"
      >
        <BatchFlashAuthConfig />
        <BatchAuthConfig />
      </div>
    </section>

    <BatchFlashAuthSlotList />
  </div>
</template>
