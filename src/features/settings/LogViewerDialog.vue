<script setup lang="ts">
import { ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { isTauriRuntime } from "@/runtime";
import { exportLogsAndReport } from "./report-issue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const { t } = useI18n();
const content = ref("");
const loading = ref(false);
const copied = ref(false);

const MAX_TAIL_BYTES = 256 * 1024;

async function load(): Promise<void> {
  if (!isTauriRuntime()) {
    content.value = "";
    return;
  }
  loading.value = true;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    content.value = await invoke<string>("read_log_tail", {
      maxBytes: MAX_TAIL_BYTES,
    });
  } catch (e) {
    content.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(content.value);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  } catch {
    /* ignore */
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) void load();
  },
);
</script>

<template>
  <div
    v-if="props.open"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    @click.self="emit('close')"
  >
    <div
      class="ty-card flex max-h-[80vh] w-full max-w-3xl flex-col gap-3 rounded-xl p-4 sm:p-5"
    >
      <div class="flex items-center justify-between">
        <h2 class="ty-section-title">{{ t("settings.logViewer.title") }}</h2>
        <button
          type="button"
          class="ty-btn-sm ty-btn-secondary"
          @click="emit('close')"
        >
          {{ t("settings.logViewer.close") }}
        </button>
      </div>
      <pre
        class="min-h-0 flex-1 overflow-auto rounded-lg bg-black/80 p-3 font-mono text-xs text-green-200"
        >{{ content || t("settings.logViewer.empty") }}</pre
      >
      <div class="flex flex-wrap gap-2">
        <button
          type="button"
          class="ty-btn-sm ty-btn-secondary"
          :disabled="loading"
          @click="load"
        >
          {{ t("settings.logViewer.refresh") }}
        </button>
        <button type="button" class="ty-btn-sm ty-btn-secondary" @click="copy">
          {{
            copied
              ? t("settings.logViewer.copied")
              : t("settings.logViewer.copy")
          }}
        </button>
        <button
          type="button"
          class="ty-btn-sm ty-btn-primary-solid"
          @click="exportLogsAndReport(t)"
        >
          {{ t("settings.reportIssue.button") }}
        </button>
      </div>
    </div>
  </div>
</template>
