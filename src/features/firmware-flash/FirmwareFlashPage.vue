<script setup lang="ts">
import TyPageHeader from "@/components/TyPageHeader.vue";
import { useI18n } from "vue-i18n";
import { provideFirmwareFlash } from "./context";
import { isTauriRuntime } from "@/runtime";
import FirmwareFlashAside from "./components/FirmwareFlashAside.vue";
import FirmwareFlashConnection from "./components/FirmwareFlashConnection.vue";
import FirmwareFlashOperations from "./components/FirmwareFlashOperations.vue";
import { nextTick, ref } from "vue";
import type { OpKind } from "./types";

const { t } = useI18n();
const ctx = provideFirmwareFlash();
const operationsRef = ref<InstanceType<typeof FirmwareFlashOperations> | null>(
  null,
);
async function showConfiguration(op: OpKind): Promise<void> {
  ctx.activeTab = op;
  await nextTick();
  operationsRef.value?.focusConfiguration();
}

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
  <div class="flex min-h-0 min-w-0 w-full flex-1 flex-col gap-2">
    <TyPageHeader :title="t('flash.pageTitle')">
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
      class="flash-workspace grid min-h-0 flex-1 grid-cols-1 gap-[10px] lg:grid-cols-[minmax(0,3fr)_minmax(15rem,1fr)]"
    >
      <FirmwareFlashOperations ref="operationsRef" />
      <FirmwareFlashAside @configure="showConfiguration" />
    </div>
  </div>
</template>
