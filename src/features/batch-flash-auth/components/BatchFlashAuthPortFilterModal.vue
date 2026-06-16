<!-- src/features/batch-flash/components/BatchPortFilterModal.vue -->
<script setup lang="ts">
import { ref } from "vue";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";

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
            串口过滤
          </h2>
          <button
            type="button"
            class="cursor-pointer text-xl text-[var(--ty-text-muted)] hover:text-[var(--ty-text)]"
            aria-label="关闭"
            @click="$emit('close')"
          >
            ×
          </button>
        </div>

        <p class="mb-3 text-xs text-[var(--ty-text-muted)]">
          添加要屏蔽的串口名称（精确匹配，Windows 不区分大小写）。
          规则生效后，自动分配时将跳过这些串口。
        </p>

        <!-- Add port input -->
        <div class="mb-4 flex gap-2">
          <input
            v-model="newPort"
            type="text"
            placeholder="如 COM1 或 /dev/ttyS0"
            class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-2.5 py-1.5 text-sm text-[var(--ty-text)] placeholder:text-[var(--ty-text-muted)]"
            @keydown.enter="addPort"
          />
          <button
            type="button"
            class="ty-btn-secondary px-3 text-sm"
            @click="addPort"
          >
            添加
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
              aria-label="移除"
              @click="store.removeBlockedPort(port)"
            >
              ×
            </button>
          </div>
        </div>
        <p v-else class="text-xs text-[var(--ty-text-muted)]">暂无过滤规则</p>
      </div>
    </div>
  </Teleport>
</template>
