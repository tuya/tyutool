<script setup lang="ts">
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';

const s = useSerialDebugStore();
const { t } = useI18n();
const historyIndex = ref(-1);

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
</script>

<template>
  <div class="send-bar flex items-center gap-2 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-2">
    <div class="mode-toggle flex rounded-lg border border-[var(--ty-border)]">
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
    <label class="toggle flex items-center gap-1 text-xs">
      <input type="checkbox" v-model="s.sendAppendCrlf" :disabled="s.sendMode === 'hex'" />
      <span>\r\n</span>
    </label>
    <button type="button" class="btn-primary" :disabled="!s.open || !s.sendInput" @click="() => { void s.send(); s.sendInput = ''; historyIndex = -1; }">
      {{ t('serialDebug.send.sendBtn') }}
    </button>
  </div>
</template>

<style scoped>
.input { border: 1px solid var(--ty-border); background: var(--ty-canvas); border-radius: 0.5rem; padding: 0.5rem; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
.mode-btn { padding: 0.375rem 0.75rem; font-size: 0.8rem; font-weight: 600; }
.mode-btn.active { background: var(--ty-primary); color: white; }
.btn-primary { padding: 0.5rem 1rem; background: var(--ty-primary); color: white; border-radius: 0.5rem; font-weight: 600; }
.btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
