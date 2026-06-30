<!-- src/features/batch-flash/components/BatchFlashToolbar.vue -->
<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import BatchFlashAuthPortFilterModal from "./BatchFlashAuthPortFilterModal.vue";
import { showConfirmDialog } from "@/composables/confirmDialog";

const { t } = useI18n();
const store = useBatchFlashAuthStore();
const filterOpen = ref(false);

async function handleStart() {
  const idleCount = store.slots.length - store.currentStats.active;
  if (idleCount > 8) {
    const parts: string[] = [
      t("batchFlashAuth.dialog.aboutToOperate", { count: idleCount }),
    ];
    if (store.opMode === "flash-then-auth" && store.firmwarePath) {
      parts.push(
        t("batchFlashAuth.dialog.firmwareLine", { path: store.firmwarePath }),
      );
    }
    if (store.authConfig.excelPath) {
      parts.push(
        t("batchFlashAuth.dialog.excelLine", {
          path: store.authConfig.excelPath,
        }),
      );
    }
    const ok = await showConfirmDialog({
      title:
        store.opMode === "auth-only"
          ? t("batchFlashAuth.dialog.confirmAuthOnly")
          : t("batchFlashAuth.dialog.confirmFlashThenAuth"),
      message: parts.join("\n"),
      kind: "warning",
    });
    if (!ok) return;
  }
  await store.startBatch();
}

async function handleAutoAssign() {
  await store.autoAssign();
}
</script>

<template>
  <div class="flex items-center gap-2">
    <!-- Left: functional controls -->
    <div class="flex items-center gap-2">
      <button
        type="button"
        class="ty-btn-secondary flex items-center gap-1.5 text-sm"
        :disabled="store.isBusy"
        :title="
          store.isBusy
            ? t('batchFlashAuth.toolbar.autoAssignBusy')
            : t('batchFlashAuth.toolbar.autoAssignHint')
        "
        @click="handleAutoAssign"
      >
        <FontAwesomeIcon :icon="['fas', 'rotate']" class="size-3.5" />
        {{ t("batchFlashAuth.toolbar.autoAssign") }}
      </button>

      <button
        type="button"
        class="ty-btn-secondary relative flex items-center gap-1.5 text-sm"
        @click="filterOpen = true"
      >
        <FontAwesomeIcon :icon="['fas', 'filter']" class="size-3.5" />
        {{ t("batchFlashAuth.toolbar.filter") }}
        <span
          v-if="store.filterActive"
          class="absolute -right-1.5 -top-1.5 flex h-4 w-4 items-center justify-center rounded-full text-[10px] font-bold"
          :style="{ backgroundColor: 'var(--ty-accent)', color: '#fff' }"
          >{{ store.filterConfig.blockedPorts.length }}</span
        >
      </button>

      <button
        type="button"
        class="ty-btn-secondary flex items-center gap-1.5 text-sm"
        :disabled="!store.canReadAll"
        :title="
          store.isBusy
            ? t('batchFlashAuth.toolbar.readAllBusy')
            : t('batchFlashAuth.toolbar.readAllHint')
        "
        @click="store.readAll()"
      >
        <FontAwesomeIcon :icon="['fas', 'magnifying-glass']" class="size-3.5" />
        {{ t("batchFlashAuth.toolbar.readAll") }}
      </button>
    </div>

    <div class="flex-1" />

    <!-- Right: action buttons -->
    <div class="flex items-center gap-2">
      <button
        type="button"
        class="ty-btn-secondary text-sm"
        :disabled="!store.canCancel"
        @click="store.cancelAll()"
      >
        {{ t("batchFlashAuth.toolbar.cancel") }}
      </button>

      <button
        type="button"
        class="ty-btn-secondary text-sm"
        :disabled="!store.canRetry"
        :title="
          !store.canRetry
            ? t('batchFlashAuth.toolbar.noFailedPorts')
            : undefined
        "
        @click="store.retryFailed()"
      >
        {{ t("batchFlashAuth.toolbar.retry") }}
      </button>

      <button
        type="button"
        class="ty-btn-primary-solid text-sm"
        :disabled="!store.canStart"
        :title="
          !store.authConfig.excelPath
            ? t('batchFlashAuth.toolbar.selectExcelFirst')
            : store.excelError
              ? t('batchFlashAuth.toolbar.excelInvalid')
              : store.excelStats?.remaining === 0
                ? t('batchFlashAuth.toolbar.excelExhausted')
                : t('batchFlashAuth.toolbar.noIdlePorts')
        "
        @click="handleStart"
      >
        <FontAwesomeIcon :icon="['fas', 'play']" class="mr-1 size-3" />
        {{ t("batchFlashAuth.toolbar.start") }}
      </button>
    </div>
  </div>

  <BatchFlashAuthPortFilterModal
    :open="filterOpen"
    @close="filterOpen = false"
  />
</template>
