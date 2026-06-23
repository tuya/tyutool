<!-- src/features/batch-flash/components/BatchPortFilterModal.vue -->
<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";

const { t } = useI18n();

defineProps<{ open: boolean }>();
defineEmits<{ close: [] }>();
const store = useBatchFlashAuthStore();
const newPort = ref("");

function addPort() {
  const p = newPort.value.trim();
  if (p) {
    store.addBlockedPort(p);
    newPort.value = "";
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      @click.self="$emit('close')"
    >
      <div
        class="w-full max-w-md rounded-2xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-5 shadow-xl"
      >
        <div class="mb-4 flex items-center justify-between">
          <h2 class="text-base font-semibold text-[var(--ty-text)]">
            {{ t("batchFlashAuth.filter.title") }}
          </h2>
          <button
            type="button"
            class="cursor-pointer text-xl text-[var(--ty-text-muted)] hover:text-[var(--ty-text)]"
            :aria-label="t('common.closeDialog')"
            @click="$emit('close')"
          >
            ×
          </button>
        </div>

        <p class="mb-3 text-xs text-[var(--ty-text-muted)]">
          {{ t("batchFlashAuth.filter.hint") }}
        </p>

        <!-- Add port input -->
        <div class="mb-4 flex gap-2">
          <input
            v-model="newPort"
            type="text"
            :placeholder="t('batchFlashAuth.filter.placeholder')"
            class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-2.5 py-1.5 text-sm text-[var(--ty-text)] placeholder:text-[var(--ty-text-muted)]"
            @keydown.enter="addPort"
          />
          <button
            type="button"
            class="ty-btn-secondary px-3 text-sm"
            @click="addPort"
          >
            {{ t("batchFlashAuth.filter.add") }}
          </button>
        </div>

        <!-- Blocked ports list -->
        <div
          v-if="store.filterConfig.blockedPorts.length > 0"
          class="flex flex-col gap-1"
        >
          <div
            v-for="port in store.filterConfig.blockedPorts"
            :key="port"
            class="flex items-center justify-between rounded-lg bg-[var(--ty-surface-muted)] px-3 py-1.5"
          >
            <span class="font-mono text-xs text-[var(--ty-text)]">{{
              port
            }}</span>
            <button
              type="button"
              class="cursor-pointer text-sm text-[var(--ty-text-muted)] hover:text-[var(--ty-danger)]"
              :aria-label="t('batchFlashAuth.filter.remove')"
              @click="store.removeBlockedPort(port)"
            >
              ×
            </button>
          </div>
        </div>
        <p v-else class="text-xs text-[var(--ty-text-muted)]">
          {{ t("batchFlashAuth.filter.empty") }}
        </p>
      </div>
    </div>
  </Teleport>
</template>
