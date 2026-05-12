<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import type { WatchChip } from '../types';

const s = useSerialDebugStore();
const { t } = useI18n();

// ── popover state ──────────────────────────────────────────────────────────
const showPopover = ref(false);
const addKeyword = ref('');
const addUseRegex = ref(false);
const addError = ref('');
const popoverRef = ref<HTMLElement | null>(null);
const inputRef = ref<HTMLInputElement | null>(null);

function openPopover(): void {
  showPopover.value = true;
  addKeyword.value = '';
  addUseRegex.value = false;
  addError.value = '';
}

function closePopover(): void {
  showPopover.value = false;
}

function onDocMousedown(e: MouseEvent): void {
  if (!popoverRef.value?.contains(e.target as Node)) {
    closePopover();
  }
}

watch(showPopover, (open) => {
  if (open) {
    document.addEventListener('mousedown', onDocMousedown);
    // Focus input after DOM update
    setTimeout(() => { inputRef.value?.focus(); }, 0);
  } else {
    document.removeEventListener('mousedown', onDocMousedown);
  }
});

onUnmounted(() => { document.removeEventListener('mousedown', onDocMousedown); });

// ── add logic ──────────────────────────────────────────────────────────────
const previewCount = computed<number | null>(() => {
  const kw = addKeyword.value.trim();
  if (!kw) return null;
  if (addUseRegex.value) {
    let re: RegExp;
    try { re = new RegExp(kw); } catch { return null; }
    return s.lines.filter((l) => re.test(l.text)).length;
  }
  return s.lines.filter((l) => l.text.includes(kw)).length;
});

function submitAdd(): void {
  addError.value = '';
  const result = s.addChip(addKeyword.value, addUseRegex.value);
  if (result === 'ok') {
    closePopover();
  } else if (result === 'duplicate') {
    addError.value = t('serialDebug.chip.dupWarning');
  } else {
    addError.value = t('serialDebug.chip.invalidRegex');
  }
}

function onInputKey(ev: KeyboardEvent): void {
  if (ev.key === 'Enter') {
    ev.preventDefault();
    submitAdd();
  } else if (ev.key === 'Escape') {
    closePopover();
  }
}

// ── chip helpers ───────────────────────────────────────────────────────────
const MODE_ICONS: Record<string, string> = {
  highlight: '●',
  filter: '◑',
  off: '○',
};

const chipMatchCounts = computed<Map<string, number>>(() => {
  const map = new Map<string, number>();
  for (const chip of s.watchChips) {
    if (chip.mode === 'off') {
      map.set(chip.id, 0);
    } else {
      map.set(chip.id, s.lines.filter((l) => s.matchChipKeyword(l, chip)).length);
    }
  }
  return map;
});

function modeTitle(chip: WatchChip): string {
  if (chip.mode === 'highlight') return t('serialDebug.chip.modeHighlight');
  if (chip.mode === 'filter') return t('serialDebug.chip.modeFilter');
  return t('serialDebug.chip.modeOff');
}
</script>

<template>
  <div class="chip-bar flex items-center gap-2 flex-wrap px-2 py-1.5 border-b border-[var(--ty-border)] bg-[var(--ty-surface)]">
    <!-- chip list -->
    <div v-for="chip in s.watchChips" :key="chip.id" class="chip-item">
      <!-- mode icon (click=cycle) -->
      <button
        type="button"
        class="chip-mode-btn"
        :style="{ color: chip.color }"
        :title="modeTitle(chip)"
        @click="s.cycleChipMode(chip.id)"
      >{{ MODE_ICONS[chip.mode] }}</button>

      <!-- keyword label -->
      <span class="chip-label" :title="chip.keyword">{{ chip.keyword }}</span>

      <!-- regex badge -->
      <span v-if="chip.useRegex" class="chip-badge">.*</span>

      <!-- match count -->
      <span v-if="chip.mode !== 'off'" class="chip-count">{{ chipMatchCounts.get(chip.id) ?? 0 }}</span>

      <!-- remove -->
      <button type="button" class="chip-remove" @click="s.removeChip(chip.id)" aria-label="Remove">×</button>
    </div>

    <!-- add button + popover -->
    <div ref="popoverRef" class="add-wrap relative">
      <button
        type="button"
        class="btn-add"
        :title="t('serialDebug.chip.addBtn')"
        @click="showPopover ? closePopover() : openPopover()"
      >＋</button>

      <div v-if="showPopover" class="add-popover absolute bottom-full left-0 z-50 mb-1">
        <div class="flex items-center gap-1">
          <input
            ref="inputRef"
            v-model="addKeyword"
            type="text"
            class="pop-input"
            :placeholder="t('serialDebug.chip.placeholder')"
            @keydown="onInputKey"
          />
          <button
            type="button"
            class="pop-toggle"
            :class="{ active: addUseRegex }"
            @click="addUseRegex = !addUseRegex"
            :title="t('serialDebug.chip.regexLabel')"
          >.*</button>
        </div>
        <div v-if="addError" class="pop-error">{{ addError }}</div>
        <div class="flex items-center justify-between gap-2 mt-1">
          <span class="pop-preview">{{ (previewCount ?? 0) }} match{{ (previewCount ?? 0) === 1 ? '' : 'es' }}</span>
          <button type="button" class="pop-add-btn" :disabled="!addKeyword.trim()" @click="submitAdd">
            {{ t('serialDebug.chip.addBtn') }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ── chip item ─────────────────────────────────────────────────────────── */
.chip-item {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.1875rem 0.5rem 0.1875rem 0.25rem;
  border: 1px solid var(--ty-border);
  border-radius: 9999px;
  background: var(--ty-canvas);
  font-size: 0.75rem;
  line-height: 1;
}

.chip-mode-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.125rem;
  height: 1.125rem;
  font-size: 0.75rem;
  cursor: pointer;
  border-radius: 50%;
  transition: opacity 0.15s ease;
  flex-shrink: 0;
}
.chip-mode-btn:hover { opacity: 0.7; }

.chip-label {
  max-width: 10rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  color: var(--ty-text);
}

.chip-badge {
  padding: 0 0.25rem;
  border-radius: 0.25rem;
  background: var(--ty-surface-muted, var(--ty-border));
  color: var(--ty-text-muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.6875rem;
}

.chip-count {
  color: var(--ty-text-muted);
  font-size: 0.6875rem;
}

.chip-remove {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1rem;
  height: 1rem;
  border-radius: 50%;
  font-size: 0.875rem;
  line-height: 1;
  color: var(--ty-text-muted);
  cursor: pointer;
  transition: color 0.15s ease, background-color 0.15s ease;
  flex-shrink: 0;
}
.chip-remove:hover { color: var(--ty-danger); background: color-mix(in srgb, var(--ty-danger) 12%, transparent); }

/* ── add button ────────────────────────────────────────────────────────── */
.btn-add {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.625rem;
  height: 1.625rem;
  border: 1px dashed var(--ty-border);
  border-radius: 9999px;
  font-size: 0.875rem;
  color: var(--ty-text-muted);
  cursor: pointer;
  transition: color 0.15s ease, border-color 0.15s ease, background-color 0.15s ease;
}
.btn-add:hover { color: var(--ty-primary); border-color: var(--ty-primary); background: color-mix(in srgb, var(--ty-primary) 8%, transparent); }

/* ── add popover ───────────────────────────────────────────────────────── */
.add-popover {
  min-width: 16rem;
  padding: 0.625rem;
  border: 1px solid var(--ty-border);
  border-radius: 0.75rem;
  background: var(--ty-surface);
  box-shadow: 0 4px 16px rgb(0 0 0 / 0.15);
}

.pop-input {
  flex: 1;
  border: 1px solid var(--ty-border);
  border-radius: 0.5rem;
  background: var(--ty-canvas);
  padding: 0.375rem 0.5rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.8125rem;
  color: var(--ty-text);
  min-width: 0;
}
.pop-input:focus { outline: none; border-color: var(--ty-primary); }

.pop-toggle {
  padding: 0.375rem 0.5rem;
  border: 1px solid var(--ty-border);
  border-radius: 0.5rem;
  background: var(--ty-canvas);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--ty-text-muted);
  cursor: pointer;
  transition: background-color 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  flex-shrink: 0;
}
.pop-toggle.active { background: var(--ty-primary); color: white; border-color: var(--ty-primary); }
.pop-toggle:not(.active):hover { background: var(--ty-surface-muted, var(--ty-border)); }

.pop-error {
  margin-top: 0.375rem;
  font-size: 0.75rem;
  color: var(--ty-danger);
}

.pop-preview {
  font-size: 0.75rem;
  color: var(--ty-text-muted);
}

.pop-add-btn {
  padding: 0.3125rem 0.75rem;
  background: var(--ty-primary);
  color: white;
  border-radius: 0.5rem;
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  transition: opacity 0.15s ease;
}
.pop-add-btn:hover:not(:disabled) { opacity: 0.88; }
.pop-add-btn:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
