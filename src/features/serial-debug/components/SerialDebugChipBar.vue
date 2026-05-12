<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';

const s = useSerialDebugStore();
const { t } = useI18n();

// ── add-popover state ─────────────────────────────────────────────────
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
  if (!popoverRef.value?.contains(e.target as Node)) closePopover();
}

watch(showPopover, (open) => {
  if (open) {
    document.addEventListener('mousedown', onDocMousedown);
    setTimeout(() => { inputRef.value?.focus(); }, 0);
  } else {
    document.removeEventListener('mousedown', onDocMousedown);
  }
});

onUnmounted(() => { document.removeEventListener('mousedown', onDocMousedown); });

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
  if (ev.key === 'Enter') { ev.preventDefault(); submitAdd(); }
  else if (ev.key === 'Escape') closePopover();
}

// ── tab match counts ──────────────────────────────────────────────────
const chipMatchCounts = computed<Map<string, number>>(() => {
  const map = new Map<string, number>();
  for (const chip of s.watchChips) {
    map.set(chip.id, s.lines.filter((l) => s.matchChipKeyword(l, chip)).length);
  }
  return map;
});

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
</script>

<template>
  <div class="tab-bar flex items-center gap-0.5 px-2 border-b border-[var(--ty-border)] bg-[var(--ty-surface)] overflow-x-auto">
    <!-- "全部" tab -->
    <button
      type="button"
      class="tab-item"
      :class="{ 'tab-active': s.activeChipId === null }"
      @click="s.setActiveChip(null)"
    >
      {{ t('serialDebug.chip.tabAll') }}
    </button>

    <!-- filter tabs -->
    <div
      v-for="chip in s.watchChips"
      :key="chip.id"
      class="tab-item tab-chip"
      :class="{ 'tab-active': s.activeChipId === chip.id }"
      @click="s.setActiveChip(chip.id)"
    >
      <span class="tab-dot" :style="{ background: chip.color }" />
      <span class="tab-keyword" :title="chip.keyword">{{ chip.keyword }}</span>
      <span v-if="chip.useRegex" class="tab-regex">.*</span>
      <span class="tab-count" :style="{ color: chip.color }">{{ chipMatchCounts.get(chip.id) ?? 0 }}</span>
      <button
        type="button"
        class="tab-close"
        @click.stop="s.removeChip(chip.id)"
        :aria-label="t('serialDebug.chip.removeTab')"
      >×</button>
    </div>

    <!-- add button -->
    <div ref="popoverRef" class="add-wrap relative ml-1">
      <button
        type="button"
        class="tab-add"
        :class="{ 'tab-add-active': showPopover }"
        @click="showPopover ? closePopover() : openPopover()"
        :title="t('serialDebug.chip.addBtn')"
      >
        <FontAwesomeIcon :icon="['fas', 'plus']" class="size-2.5" />
      </button>

      <div v-if="showPopover" class="add-popover">
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
        <div class="flex items-center justify-between gap-2 mt-1.5">
          <span class="pop-preview">
            {{ previewCount !== null ? `${previewCount} match${previewCount === 1 ? '' : 'es'}` : '' }}
          </span>
          <button type="button" class="pop-add-btn" :disabled="!addKeyword.trim()" @click="submitAdd">
            {{ t('serialDebug.chip.addBtn') }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ── tab bar ─────────────────────────────────────────────────────────── */
.tab-bar {
  min-height: 2rem;
  scrollbar-width: none;
}
.tab-bar::-webkit-scrollbar { display: none; }

.tab-item {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.25rem 0.625rem;
  border-radius: 0.5rem 0.5rem 0 0;
  border: 1px solid transparent;
  border-bottom: none;
  font-size: 0.75rem;
  cursor: pointer;
  white-space: nowrap;
  color: var(--ty-text-muted);
  background: transparent;
  transition: color 0.15s, background-color 0.15s;
  flex-shrink: 0;
}
.tab-item:hover { color: var(--ty-text); background: var(--ty-surface-muted, color-mix(in srgb, var(--ty-text) 6%, transparent)); }
.tab-active {
  color: var(--ty-text);
  background: var(--ty-canvas);
  border-color: var(--ty-border);
  position: relative;
}
/* cover bottom border so active tab merges with log area */
.tab-active::after {
  content: '';
  position: absolute;
  bottom: -1px;
  left: 0;
  right: 0;
  height: 1px;
  background: var(--ty-canvas);
}

.tab-dot {
  display: inline-block;
  width: 0.4rem;
  height: 0.4rem;
  border-radius: 50%;
  flex-shrink: 0;
}
.tab-keyword {
  max-width: 8rem;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: monospace;
}
.tab-regex {
  font-size: 0.6rem;
  font-weight: 700;
  color: var(--ty-text-muted);
  letter-spacing: -0.02em;
}
.tab-count {
  font-size: 0.6875rem;
  font-variant-numeric: tabular-nums;
  font-weight: 600;
  opacity: 0.85;
}
.tab-close {
  font-size: 0.875rem;
  line-height: 1;
  padding: 0 0.125rem;
  border-radius: 3px;
  background: none;
  border: none;
  cursor: pointer;
  color: var(--ty-text-muted);
  opacity: 0.6;
  transition: opacity 0.15s, background-color 0.15s;
}
.tab-close:hover { opacity: 1; background: color-mix(in srgb, var(--ty-danger) 15%, transparent); color: var(--ty-danger); }

/* ── add button ──────────────────────────────────────────────────────── */
.add-wrap { display: inline-flex; align-items: center; }
.tab-add {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.5rem;
  height: 1.5rem;
  border-radius: 50%;
  border: 1px dashed var(--ty-border);
  background: transparent;
  color: var(--ty-text-muted);
  cursor: pointer;
  transition: border-color 0.15s, color 0.15s, background-color 0.15s;
}
.tab-add:hover, .tab-add-active {
  border-color: var(--ty-primary);
  color: var(--ty-primary);
  background: color-mix(in srgb, var(--ty-primary) 8%, transparent);
}

/* ── popover ─────────────────────────────────────────────────────────── */
.add-popover {
  position: absolute;
  top: calc(100% + 0.375rem);
  left: 0;
  z-index: 50;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  padding: 0.5rem;
  border-radius: 0.625rem;
  border: 1px solid var(--ty-border);
  background: var(--ty-surface);
  box-shadow: 0 8px 24px color-mix(in srgb, #000 18%, transparent);
  width: 16rem;
}
.pop-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--ty-border);
  background: var(--ty-canvas);
  border-radius: 0.375rem;
  padding: 0.25rem 0.5rem;
  font-size: 0.8125rem;
  font-family: monospace;
  outline: none;
}
.pop-input:focus { border-color: var(--ty-primary); }
.pop-toggle {
  padding: 0.25rem 0.5rem;
  border-radius: 0.375rem;
  border: 1px solid var(--ty-border);
  background: transparent;
  font-size: 0.75rem;
  cursor: pointer;
  color: var(--ty-text-muted);
  white-space: nowrap;
  transition: background-color 0.15s, color 0.15s, border-color 0.15s;
}
.pop-toggle.active {
  background: color-mix(in srgb, var(--ty-primary) 15%, transparent);
  border-color: var(--ty-primary);
  color: var(--ty-primary);
}
.pop-preview { font-size: 0.7rem; color: var(--ty-text-muted); min-height: 1em; }
.pop-add-btn {
  padding: 0.2rem 0.625rem;
  border-radius: 0.375rem;
  border: 1px solid var(--ty-primary);
  background: color-mix(in srgb, var(--ty-primary) 15%, transparent);
  color: var(--ty-primary);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  transition: background-color 0.15s;
}
.pop-add-btn:hover:not(:disabled) { background: color-mix(in srgb, var(--ty-primary) 25%, transparent); }
.pop-add-btn:disabled { opacity: 0.4; cursor: not-allowed; }
.pop-error { font-size: 0.75rem; color: var(--ty-danger); }
</style>
