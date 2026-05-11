<script setup lang="ts">
import { onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';

const s = useSerialDebugStore();
const { t } = useI18n();
const historyIndex = ref(-1);
const historyOpen = ref(false);
const historyWrapRef = ref<HTMLElement | null>(null);

function onDocMousedown(e: MouseEvent): void {
  if (!historyWrapRef.value?.contains(e.target as Node)) {
    historyOpen.value = false;
  }
}

watch(historyOpen, (open) => {
  if (open) document.addEventListener('mousedown', onDocMousedown);
  else document.removeEventListener('mousedown', onDocMousedown);
});
onUnmounted(() => { document.removeEventListener('mousedown', onDocMousedown); });

function onKey(ev: KeyboardEvent): void {
  if (ev.key === 'Enter') {
    ev.preventDefault();
    void s.send();
    s.sendInput = '';
    historyIndex.value = -1;
    return;
  }
  if (ev.key === 'ArrowUp') {
    if (s.sendHistory.length === 0) return;
    ev.preventDefault();
    historyIndex.value = Math.min(historyIndex.value + 1, s.sendHistory.length - 1);
    s.sendInput = s.sendHistory[historyIndex.value] ?? '';
  } else if (ev.key === 'ArrowDown') {
    if (historyIndex.value <= 0) {
      historyIndex.value = -1;
      s.sendInput = '';
      return;
    }
    ev.preventDefault();
    historyIndex.value -= 1;
    s.sendInput = s.sendHistory[historyIndex.value] ?? '';
  }
}

function selectHistory(item: string): void {
  s.sendInput = item;
  historyOpen.value = false;
}
</script>

<template>
  <div class="send-bar flex items-center gap-2 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-2">
    <div class="mode-toggle flex overflow-hidden rounded-lg border border-[var(--ty-border)]">
      <button type="button" :class="{ active: s.sendMode === 'ascii' }" class="mode-btn" @click="s.sendMode = 'ascii'">ASCII</button>
      <button type="button" :class="{ active: s.sendMode === 'hex' }" class="mode-btn" @click="s.sendMode = 'hex'">Hex</button>
    </div>
    <input
      type="text"
      class="input flex-1"
      :placeholder="t('serialDebug.send.placeholder')"
      :disabled="!s.open"
      v-model="s.sendInput"
      @keydown="onKey"
    />

    <!-- history button + dropdown -->
    <div ref="historyWrapRef" class="history-wrap relative">
      <button
        type="button"
        class="btn-icon"
        :aria-label="t('serialDebug.send.history')"
        :disabled="s.sendHistory.length === 0"
        @click="historyOpen = !historyOpen"
      >
        <FontAwesomeIcon :icon="['fas', 'clock-rotate-left']" />
      </button>
      <div v-if="historyOpen" class="history-dropdown absolute bottom-full right-0 z-50 mb-1 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] shadow-lg">
        <button
          v-for="(item, i) in s.sendHistory"
          :key="i"
          type="button"
          class="history-item"
          @click="selectHistory(item)"
        >{{ item }}</button>
      </div>
    </div>

    <label class="toggle flex cursor-pointer items-center gap-1 text-xs">
      <input type="checkbox" class="shrink-0" v-model="s.sendAppendCrlf" :disabled="s.sendMode === 'hex'" />
      <span>\r\n</span>
    </label>
    <button type="button" class="btn-primary" :disabled="!s.open || !s.sendInput" @click="() => { void s.send(); s.sendInput = ''; historyIndex = -1; }">
      {{ t('serialDebug.send.sendBtn') }}
    </button>
  </div>
</template>

<style scoped>
.input { border: 1px solid var(--ty-border); background: var(--ty-canvas); border-radius: 0.5rem; padding: 0.5rem; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
.mode-btn {
  padding: 0.375rem 0.75rem;
  font-size: 0.8rem;
  font-weight: 600;
  cursor: pointer;
  transition: background-color 0.15s ease, color 0.15s ease;
}
.mode-btn:not(.active):hover { background: var(--ty-surface-muted); }
.mode-btn.active { background: var(--ty-primary); color: white; }
.btn-primary {
  padding: 0.5rem 1rem;
  background: var(--ty-primary);
  color: white;
  border-radius: 0.5rem;
  font-weight: 600;
  cursor: pointer;
  transition: opacity 0.15s ease;
}
.btn-primary:hover:not(:disabled) { opacity: 0.88; }
.btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }
.btn-icon {
  padding: 0.5rem 0.625rem;
  border: 1px solid var(--ty-border);
  border-radius: 0.5rem;
  background: var(--ty-canvas);
  cursor: pointer;
  transition: background-color 0.15s ease;
}
.btn-icon:hover:not(:disabled) { background: var(--ty-surface-muted); }
.btn-icon:disabled { opacity: 0.4; cursor: not-allowed; }
.history-dropdown { min-width: 14rem; max-height: 16rem; overflow-y: auto; }
.history-item {
  display: block;
  width: 100%;
  text-align: left;
  padding: 0.375rem 0.75rem;
  font-size: 0.8125rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}
.history-item:hover { background: var(--ty-surface-muted); }
</style>
