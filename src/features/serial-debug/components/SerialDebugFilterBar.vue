<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';

const s = useSerialDebugStore();
const { t } = useI18n();

async function openWindow(): Promise<void> {
  const kind = await s.openFilterWindow();
  if (kind === 'inline') {
    // Web mode: the main log pane already applies the filter via filterMode + filterText,
    // so there is no secondary window to render. Surface a sys-line hint so the user
    // understands the button's behavior in this runtime.
    s.appendSysLine(t('serialDebug.filter.inlineAppliedHint'));
  }
}
</script>

<template>
  <div class="filter-bar flex items-center gap-2 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-2">
    <FontAwesomeIcon :icon="['fas', 'magnifying-glass']" class="text-[var(--ty-text-muted)]" />
    <input
      type="text"
      class="input flex-1"
      :placeholder="t('serialDebug.filter.placeholder')"
      v-model="s.filterText"
    />
    <div class="mode-group flex rounded-lg border border-[var(--ty-border)]">
      <button type="button" :class="{ active: s.filterMode === 'off' }" class="mode-btn" @click="s.filterMode = 'off'">{{ t('serialDebug.filter.off') }}</button>
      <button type="button" :class="{ active: s.filterMode === 'include' }" class="mode-btn" @click="s.filterMode = 'include'">{{ t('serialDebug.filter.include') }}</button>
      <button type="button" :class="{ active: s.filterMode === 'exclude' }" class="mode-btn" @click="s.filterMode = 'exclude'">{{ t('serialDebug.filter.exclude') }}</button>
    </div>
    <button type="button" class="btn-secondary" @click="openWindow">{{ isTauriRuntime() ? t('serialDebug.filter.openInWindow') : t('serialDebug.filter.openInline') }}</button>
  </div>
</template>

<style scoped>
.input { border: 1px solid var(--ty-border); background: var(--ty-canvas); border-radius: 0.5rem; padding: 0.375rem 0.5rem; font-size: 0.875rem; }
.mode-btn { padding: 0.375rem 0.625rem; font-size: 0.75rem; }
.mode-btn.active { background: var(--ty-primary); color: white; font-weight: 600; }
.btn-secondary { padding: 0.375rem 0.75rem; border: 1px solid var(--ty-border); border-radius: 0.5rem; font-size: 0.8125rem; }
</style>
