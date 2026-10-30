<script setup lang="ts">
import TyPageHeader from "@/components/TyPageHeader.vue";
import { useI18n } from "vue-i18n";
import { provideFirmwareFlash } from "./context";
import { isTauriRuntime } from "@/runtime";
import FirmwareFlashAside from "./components/FirmwareFlashAside.vue";
import FirmwareFlashConnection from "./components/FirmwareFlashConnection.vue";
import FirmwareFlashOperations from "./components/FirmwareFlashOperations.vue";

const { t } = useI18n();
provideFirmwareFlash();

async function resetWindowSize(): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("reset_main_window_layout");
  } catch (_e) {
    /* silently ignore — window API may not be permitted */
  }
}
</script>

<template>
  <div class="flex min-h-0 min-w-0 w-full flex-1 flex-col gap-2 md:gap-3">
    <TyPageHeader :title="t('flash.pageTitle')" :section="t('flash.tool')">
      {{ t("flash.introLead") }}{{ t("flash.introRest") }}
      <template v-if="isTauriRuntime()" #actions>
        <button
          type="button"
          class="page-header-btn rounded-md px-2.5 py-1.5 text-xs"
          :aria-label="t('app.resetWindowSize')"
          @click="resetWindowSize"
        >
          {{ t("app.resetWindowSize") }}
        </button>
      </template>
    </TyPageHeader>

    <FirmwareFlashConnection />

    <div
      class="flash-workspace grid min-h-0 flex-1 grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1.05fr)_minmax(0,1fr)]"
    >
      <FirmwareFlashOperations />
      <FirmwareFlashAside />
    </div>
  </div>
</template>
