<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import TySelect from '@/components/TySelect.vue';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';

const emit = defineEmits<{ close: [] }>();
const s = useSerialDebugStore();
const { t } = useI18n();

const dataBitsOptions = computed(() => [
  { value: 'five', label: '5' },
  { value: 'six', label: '6' },
  { value: 'seven', label: '7' },
  { value: 'eight', label: '8' },
]);
const parityOptions = computed(() => [
  { value: 'none', label: t('serialDebug.conn.parityNone') },
  { value: 'odd', label: t('serialDebug.conn.parityOdd') },
  { value: 'even', label: t('serialDebug.conn.parityEven') },
]);
const stopBitsOptions = computed(() => [
  { value: 'one', label: '1' },
  { value: 'onePointFive', label: '1.5' },
  { value: 'two', label: '2' },
]);

function onKeydown(e: KeyboardEvent): void {
  if (e.key === 'Escape') emit('close');
}
onMounted(() => window.addEventListener('keydown', onKeydown));
onUnmounted(() => window.removeEventListener('keydown', onKeydown));

async function toggleAutoSave(): Promise<void> {
  if (!s.autoSave && !s.autoSaveDir) {
    // No path yet — pick one first, then enable
    s.autoSave = true; // temporarily set so pickAutoSaveDir can roll it back on cancel
    await s.pickAutoSaveDir();
  } else {
    s.autoSave = !s.autoSave;
  }
}
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
    role="dialog"
    aria-modal="true"
    :aria-label="t('serialDebug.conn.settings')"
    @click.self="emit('close')"
  >
    <div class="ty-card w-[min(90vw,400px)] overflow-hidden">
      <div class="flex items-center justify-between border-b border-[var(--ty-border)] px-4 py-3">
        <h2 class="text-sm font-semibold text-[var(--ty-text)]">{{ t('serialDebug.conn.settings') }}</h2>
        <button
          type="button"
          class="page-header-btn flex size-8 items-center justify-center rounded-lg"
          aria-label="close"
          @click="emit('close')"
        >
          <FontAwesomeIcon :icon="['fas', 'xmark']" class="size-3.5" />
        </button>
      </div>
      <div class="flex flex-col gap-4 p-4">
        <div class="grid grid-cols-3 gap-3">
          <label class="field cursor-pointer">
            <span class="conn-field-label mb-1 text-xs font-semibold">{{ t('serialDebug.conn.dataBits') }}</span>
            <TySelect v-model="s.dataBits" :options="dataBitsOptions" :disabled="s.open" />
          </label>
          <label class="field cursor-pointer">
            <span class="conn-field-label mb-1 text-xs font-semibold">{{ t('serialDebug.conn.parity') }}</span>
            <TySelect v-model="s.parity" :options="parityOptions" :disabled="s.open" />
          </label>
          <label class="field cursor-pointer">
            <span class="conn-field-label mb-1 text-xs font-semibold">{{ t('serialDebug.conn.stopBits') }}</span>
            <TySelect v-model="s.stopBits" :options="stopBitsOptions" :disabled="s.open" />
          </label>
        </div>

        <label class="check-row flex cursor-pointer items-center gap-2 text-sm">
          <input type="checkbox" v-model="s.autoRelease" class="shrink-0" />
          <span class="text-[var(--ty-text)]">{{ t('serialDebug.conn.autoRelease') }}</span>
          <span class="cursor-help text-[var(--ty-text-muted)]" :title="t('serialDebug.conn.autoReleaseTip')">ⓘ</span>
        </label>

        <label class="check-row flex cursor-pointer items-center gap-2 text-sm">
          <input type="checkbox" v-model="s.hexView" class="shrink-0" />
          <span class="text-[var(--ty-text)]">{{ t('serialDebug.conn.hexView') }}</span>
        </label>

        <label class="check-row flex cursor-pointer items-center gap-2 text-sm">
          <input type="checkbox" v-model="s.ansiEnabled" class="shrink-0" />
          <span class="text-[var(--ty-text)]">{{ t('serialDebug.conn.ansiParse') }}</span>
          <span class="cursor-help text-[var(--ty-text-muted)]" :title="t('serialDebug.conn.ansiParseTip')">ⓘ</span>
        </label>

        <template v-if="isTauriRuntime()">
          <hr class="border-[var(--ty-border)]" />
          <div class="flex flex-col gap-3">
            <span class="text-xs font-semibold text-[var(--ty-text)]">{{ t('serialDebug.autoSave.label') }}</span>

            <label class="check-row flex cursor-pointer items-center gap-2 text-sm">
              <input type="checkbox" :checked="s.autoSave" class="shrink-0" @change="toggleAutoSave" />
              <span class="text-[var(--ty-text)]">{{ t('serialDebug.autoSave.description') }}</span>
            </label>

            <div class="flex flex-col gap-1">
              <span class="text-xs font-semibold text-[var(--ty-text-muted)]">{{ t('serialDebug.autoSave.dirLabel') }}</span>
              <div class="flex gap-2">
                <input
                  type="text"
                  class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-canvas)] px-2 py-1 text-xs text-[var(--ty-text)]"
                  readonly
                  :value="s.autoSaveDir || ''"
                  :placeholder="t('serialDebug.autoSave.dirLabel')"
                />
                <button type="button" class="btn-tool shrink-0" @click="s.pickAutoSaveDir()">
                  {{ t('serialDebug.autoSave.pickDir') }}
                </button>
              </div>
            </div>

            <label class="check-row flex cursor-pointer items-center gap-2 text-sm">
              <input type="checkbox" v-model="s.autoSaveTimestamp" class="shrink-0" />
              <span class="text-[var(--ty-text)]">{{ t('serialDebug.autoSave.timestamp') }}</span>
            </label>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.field { display: flex; flex-direction: column; }
.btn-tool {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.125rem 0.5rem;
  border: 1px solid var(--ty-border);
  border-radius: 0.375rem;
  background: transparent;
  cursor: pointer;
  font-size: 0.8125rem;
  color: var(--ty-text-muted);
  white-space: nowrap;
}
.btn-tool:hover { background: var(--ty-surface-muted); color: var(--ty-text); }
</style>
