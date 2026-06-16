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
  const idleCount = store.slots.filter((s) => s.status === "idle").length;
  if (idleCount > 8) {
    const excelInfo = store.authConfig.excelPath
      ? `\n授权表：${store.authConfig.excelPath}`
      : "";
    const firmwareInfo =
      store.opMode === "flash-then-auth" && store.firmwarePath
        ? `\n固件：${store.firmwarePath}`
        : "";
    const ok = await showConfirmDialog({
      title: store.opMode === "auth-only" ? "确认批量授权" : "确认批量烧录",
      message: `即将对 ${idleCount} 个端口并行操作${firmwareInfo}${excelInfo}`,
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
          store.isBusy ? '任务进行中，不可自动分配' : '扫描并添加可用串口'
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
        :title="!store.canRetry ? '暂无失败端口' : undefined"
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
            ? '请先选择授权表'
            : !store.slots.some((s) => s.status === 'idle')
              ? '暂无空闲串口'
              : undefined
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
