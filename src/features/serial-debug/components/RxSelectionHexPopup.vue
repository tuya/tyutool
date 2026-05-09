<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { formatHexDump } from '@/features/serial-debug/hex-format';
import type { HexBytesPerRow } from '@/features/serial-debug/types';

const s = useSerialDebugStore();
const { t } = useI18n();

const mode = ref<'hex' | 'ascii'>('hex');
const bytesPerRow = ref<HexBytesPerRow>(16);
const editable = ref('');

watch(
  () => s.hexPopup.open,
  (open) => {
    if (open) {
      mode.value = s.hexPopup.initialMode;
      const text = new TextDecoder('utf-8', { fatal: false }).decode(s.hexPopup.bytes);
      editable.value = mode.value === 'hex' ? formatHexDump(s.hexPopup.bytes, bytesPerRow.value) : text;
    }
  },
);

watch([mode, bytesPerRow], () => {
  if (!s.hexPopup.open) return;
  if (mode.value === 'hex') {
    editable.value = formatHexDump(s.hexPopup.bytes, bytesPerRow.value);
  } else {
    editable.value = new TextDecoder('utf-8', { fatal: false }).decode(s.hexPopup.bytes);
  }
});

const title = computed(() => mode.value === 'hex' ? t('serialDebug.hexPopup.titleHex') : t('serialDebug.hexPopup.titleAscii'));

async function copy(): Promise<void> {
  try { await navigator.clipboard.writeText(editable.value); } catch { /* ignore */ }
}
</script>

<template>
  <div v-if="s.hexPopup.open" class="overlay fixed inset-0 z-50 flex items-center justify-center bg-black/40" @click.self="s.closeHexPopup()">
    <div class="dialog w-[min(90vw,640px)] rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-4 shadow-xl">
      <header class="flex items-center justify-between mb-3">
        <h2 class="text-base font-semibold">{{ title }}</h2>
        <button type="button" class="btn-icon" @click="s.closeHexPopup()">×</button>
      </header>
      <div class="flex items-center gap-2 mb-2">
        <div class="mode flex rounded-md border border-[var(--ty-border)]">
          <button type="button" :class="{ active: mode === 'hex' }" class="mode-btn" @click="mode = 'hex'">Hex</button>
          <button type="button" :class="{ active: mode === 'ascii' }" class="mode-btn" @click="mode = 'ascii'">ASCII</button>
        </div>
        <div v-if="mode === 'hex'" class="row flex rounded-md border border-[var(--ty-border)]">
          <button type="button" :class="{ active: bytesPerRow === 8 }" class="mode-btn" @click="bytesPerRow = 8">8</button>
          <button type="button" :class="{ active: bytesPerRow === 16 }" class="mode-btn" @click="bytesPerRow = 16">16</button>
          <button type="button" :class="{ active: bytesPerRow === 32 }" class="mode-btn" @click="bytesPerRow = 32">32</button>
        </div>
        <button type="button" class="ml-auto btn-secondary" @click="copy">{{ t('serialDebug.hexPopup.copy') }}</button>
      </div>
      <textarea v-model="editable" class="textarea w-full h-[18rem] rounded-md border border-[var(--ty-border)] bg-[var(--ty-canvas)] p-2 font-mono text-xs"></textarea>
    </div>
  </div>
</template>

<style scoped>
.btn-icon { width: 2rem; height: 2rem; display: grid; place-items: center; border-radius: 0.375rem; }
.btn-icon:hover { background: var(--ty-surface-muted); }
.mode-btn { padding: 0.25rem 0.625rem; font-size: 0.75rem; }
.mode-btn.active { background: var(--ty-primary); color: white; font-weight: 600; }
.btn-secondary { padding: 0.375rem 0.75rem; border: 1px solid var(--ty-border); border-radius: 0.375rem; font-size: 0.8125rem; }
</style>
