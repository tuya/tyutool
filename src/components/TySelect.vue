<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue';

/** Fixed-position hint near hovered option (avoids list overflow + paint-order clipping). */
type FloatingOptionTip = { text: string; top: number; left: number };

export interface TySelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  /** Shown as hover flyout on this option (e.g. serial port probe hints). */
  optionTooltip?: string;
}

const props = withDefaults(
  defineProps<{
    modelValue: string;
    options: TySelectOption[];
    disabled?: boolean;
    placeholder?: string;
    id?: string;
  }>(),
  {
    disabled: false,
    placeholder: '—',
  }
);

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'open'): void;
}>();

const open = ref(false);
const triggerRef = ref<HTMLButtonElement | null>(null);
const listRef = ref<HTMLUListElement | null>(null);
const activeIndex = ref(-1);

const floatingOptionTip = ref<FloatingOptionTip | null>(null);
let floatingTipHideTimer: ReturnType<typeof setTimeout> | null = null;

function clearFloatingTipHideTimer() {
  if (floatingTipHideTimer !== null) {
    clearTimeout(floatingTipHideTimer);
    floatingTipHideTimer = null;
  }
}

function scheduleHideFloatingTip() {
  clearFloatingTipHideTimer();
  floatingTipHideTimer = setTimeout(() => {
    floatingOptionTip.value = null;
    floatingTipHideTimer = null;
  }, 120);
}

function showFloatingOptionTip(e: MouseEvent, text: string) {
  clearFloatingTipHideTimer();
  const el = e.currentTarget as HTMLElement | null;
  if (!el) {
    return;
  }
  const r = el.getBoundingClientRect();
  const margin = 8;
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const maxTipWidth = Math.min(18 * 16, vw - margin * 2);
  // Place beside the row, vertically centered with the hovered option.
  const top = Math.max(margin, Math.min(r.top + r.height / 2, vh - margin));
  const left = Math.max(margin, Math.min(r.right + margin, vw - margin - maxTipWidth));
  floatingOptionTip.value = {
    text,
    left,
    top,
  };
}

function onOptionLiMouseEnter(e: MouseEvent, opt: TySelectOption, idx: number) {
  if (opt.disabled) {
    return;
  }
  activeIndex.value = idx;
  if (opt.optionTooltip) {
    showFloatingOptionTip(e, opt.optionTooltip);
  } else {
    floatingOptionTip.value = null;
  }
}

function onOptionLiMouseLeave() {
  scheduleHideFloatingTip();
}

function onFloatingTipMouseEnter() {
  clearFloatingTipHideTimer();
}

function onFloatingTipMouseLeave() {
  scheduleHideFloatingTip();
}

function onListScroll() {
  clearFloatingTipHideTimer();
  floatingOptionTip.value = null;
}

// 下拉列表的定位（fixed，避免影响父布局）
const listStyle = ref({ top: '0px', left: '0px', minWidth: '0px' });

function updateListPosition() {
  const rect = triggerRef.value?.getBoundingClientRect();
  if (!rect) return;
  listStyle.value = {
    top: `${rect.bottom + 4}px`,
    left: `${rect.left}px`,
    minWidth: `${rect.width}px`,
  };
}

const selectedLabel = computed(() => {
  const opt = props.options.find(o => o.value === props.modelValue);
  return opt ? opt.label : (props.placeholder ?? '—');
});

function toggle() {
  if (props.disabled) return;
  if (!open.value) {
    updateListPosition();
    emit('open');
  }
  open.value = !open.value;
}

function select(opt: TySelectOption) {
  if (opt.disabled) return;
  emit('update:modelValue', opt.value);
  open.value = false;
  clearFloatingTipHideTimer();
  floatingOptionTip.value = null;
  triggerRef.value?.focus();
}

function close() {
  open.value = false;
  clearFloatingTipHideTimer();
  floatingOptionTip.value = null;
}

// 点击外部关闭
function onClickOutside(e: MouseEvent) {
  const root = triggerRef.value?.closest('.ty-select-root');
  const list = listRef.value;
  if (root && !root.contains(e.target as Node) && list && !list.contains(e.target as Node)) {
    close();
  }
}

// 键盘操作
function onKeydown(e: KeyboardEvent) {
  if (!open.value) {
    if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown') {
      e.preventDefault();
      updateListPosition();
      open.value = true;
      activeIndex.value = props.options.findIndex(o => o.value === props.modelValue);
      if (activeIndex.value < 0) activeIndex.value = 0;
    }
    return;
  }

  if (e.key === 'Escape') {
    e.preventDefault();
    close();
    triggerRef.value?.focus();
  } else if (e.key === 'ArrowDown') {
    e.preventDefault();
    let next = activeIndex.value + 1;
    while (next < props.options.length && props.options[next].disabled) next++;
    if (next < props.options.length) activeIndex.value = next;
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    let prev = activeIndex.value - 1;
    while (prev >= 0 && props.options[prev].disabled) prev--;
    if (prev >= 0) activeIndex.value = prev;
  } else if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault();
    if (activeIndex.value >= 0 && activeIndex.value < props.options.length) {
      select(props.options[activeIndex.value]);
    }
  } else if (e.key === 'Tab') {
    close();
  }
}

// 打开时滚动选中项到可见区域
watch(open, async val => {
  if (!val) {
    clearFloatingTipHideTimer();
    floatingOptionTip.value = null;
  }
  if (val) {
    activeIndex.value = props.options.findIndex(o => o.value === props.modelValue);
    if (activeIndex.value < 0) activeIndex.value = 0;
    await nextTick();
    const li = listRef.value?.querySelector<HTMLElement>('[aria-selected="true"]');
    li?.scrollIntoView({ block: 'nearest' });
  }
});

watch(activeIndex, async () => {
  await nextTick();
  const li = listRef.value?.querySelectorAll<HTMLElement>('[role="option"]')[activeIndex.value];
  li?.scrollIntoView({ block: 'nearest' });
});

// 滚动/resize 时更新位置（并关闭悬浮提示，避免坐标错位）
function onScrollOrResize() {
  clearFloatingTipHideTimer();
  floatingOptionTip.value = null;
  if (open.value) updateListPosition();
}

onMounted(() => {
  document.addEventListener('mousedown', onClickOutside);
  window.addEventListener('scroll', onScrollOrResize, true);
  window.addEventListener('resize', onScrollOrResize);
});
onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onClickOutside);
  window.removeEventListener('scroll', onScrollOrResize, true);
  window.removeEventListener('resize', onScrollOrResize);
  clearFloatingTipHideTimer();
});
</script>

<template>
  <div class="ty-select-root" :class="{ 'ty-select-open': open, 'ty-select-disabled': disabled }">
    <button
      :id="id"
      ref="triggerRef"
      type="button"
      class="ty-select-trigger"
      :disabled="disabled"
      :aria-haspopup="'listbox'"
      :aria-expanded="open"
      @click="toggle"
      @keydown="onKeydown"
    >
      <span class="ty-select-value">{{ selectedLabel }}</span>
      <span class="ty-select-arrow" aria-hidden="true">
        <svg width="10" height="6" viewBox="0 0 10 6" fill="none">
          <path
            d="M1 1l4 4 4-4"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </span>
    </button>

    <Teleport to="body">
      <Transition name="ty-select-drop">
        <ul
          v-if="open"
          ref="listRef"
          role="listbox"
          class="ty-select-list"
          :style="listStyle"
          @keydown="onKeydown"
          @scroll.passive="onListScroll"
        >
          <li
            v-for="(opt, idx) in options"
            :key="opt.value"
            role="option"
            class="ty-select-option"
            :class="{
              'ty-select-option--selected': opt.value === modelValue,
              'ty-select-option--active': idx === activeIndex,
              'ty-select-option--disabled': opt.disabled,
            }"
            :aria-selected="opt.value === modelValue"
            :aria-disabled="opt.disabled"
            :title="opt.optionTooltip || undefined"
            @mouseenter="onOptionLiMouseEnter($event, opt, idx)"
            @mouseleave="onOptionLiMouseLeave"
            @mousedown.prevent="select(opt)"
          >
            <span class="ty-select-option-text">{{ opt.label }}</span>
          </li>
        </ul>
      </Transition>
    </Teleport>

    <Teleport to="body">
      <div
        v-if="floatingOptionTip"
        class="ty-select-floating-tip"
        role="tooltip"
        :style="{
          top: `${floatingOptionTip.top}px`,
          left: `${floatingOptionTip.left}px`,
        }"
        @mouseenter="onFloatingTipMouseEnter"
        @mouseleave="onFloatingTipMouseLeave"
      >
        {{ floatingOptionTip.text }}
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.ty-select-root {
  position: relative;
  display: inline-block;
}

/* ── Trigger button ── */
.ty-select-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  width: 100%;
  height: 2.125rem;
  padding: 0 0.625rem;
  border-radius: 0.5rem;
  border: 1px solid color-mix(in srgb, var(--ty-primary) 25%, var(--ty-border));
  background-color: var(--ty-surface-muted);
  color: var(--ty-text);
  font-size: 0.8125rem;
  font-weight: 500;
  cursor: pointer;
  text-align: left;
  transition:
    border-color 0.15s ease,
    box-shadow 0.15s ease,
    background-color 0.15s ease;
}

.ty-select-trigger:focus-visible {
  outline: none;
  border-color: var(--ty-primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--ty-primary) 18%, transparent);
}

.ty-select-open .ty-select-trigger {
  border-color: var(--ty-primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--ty-primary) 18%, transparent);
}

.ty-select-disabled .ty-select-trigger {
  cursor: not-allowed;
  opacity: 0.5;
}

/* ── Value & arrow ── */
.ty-select-value {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ty-select-arrow {
  flex-shrink: 0;
  color: var(--ty-text-muted);
  transition: transform 0.18s ease;
}

.ty-select-open .ty-select-arrow {
  transform: rotate(180deg);
}

/* ── Dropdown list（Teleport 到 body，fixed 定位） ── */
.ty-select-list {
  position: fixed;
  z-index: 9999;
  max-height: 14rem;
  overflow-y: auto;
  margin: 0;
  padding: 0.25rem;
  list-style: none;
  border-radius: 0.625rem;
  border: 1px solid var(--ty-border);
  background-color: var(--ty-surface);
  box-shadow:
    0 4px 16px rgba(0, 0, 0, 0.12),
    0 1px 4px rgba(0, 0, 0, 0.08);
}

:global(.dark) .ty-select-list {
  box-shadow:
    0 4px 20px rgba(0, 0, 0, 0.5),
    0 1px 6px rgba(0, 0, 0, 0.3);
}

/* ── Option items ── */
.ty-select-option {
  position: relative;
  display: flex;
  align-items: center;
  padding: 0.375rem 0.625rem;
  border-radius: 0.375rem;
  font-size: 0.8125rem;
  font-weight: 500;
  color: var(--ty-text);
  cursor: pointer;
  user-select: none;
  transition: background-color 0.1s ease;
}

.ty-select-option-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ty-select-option--active {
  background-color: color-mix(in srgb, var(--ty-primary) 10%, var(--ty-surface-muted));
}

.ty-select-option--selected {
  color: var(--ty-primary);
  font-weight: 600;
}

.ty-select-option--selected.ty-select-option--active {
  background-color: color-mix(in srgb, var(--ty-primary) 12%, var(--ty-surface-muted));
}

.ty-select-option--disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

/* ── Transition ── */
.ty-select-drop-enter-active {
  transition:
    opacity 0.15s ease,
    transform 0.15s ease;
}

.ty-select-drop-leave-active {
  transition:
    opacity 0.1s ease,
    transform 0.1s ease;
}

.ty-select-drop-enter-from,
.ty-select-drop-leave-to {
  opacity: 0;
  transform: translateY(-4px) scale(0.98);
}

/* ── Scrollbar styling ── */
.ty-select-list::-webkit-scrollbar {
  width: 4px;
}

.ty-select-list::-webkit-scrollbar-track {
  background: transparent;
}

.ty-select-list::-webkit-scrollbar-thumb {
  background: var(--ty-border-strong, var(--ty-border));
  border-radius: 9999px;
}

/* Teleported above dropdown list (z-index above .ty-select-list 9999) */
.ty-select-floating-tip {
  position: fixed;
  z-index: 10050;
  max-width: min(18rem, calc(100vw - 1rem));
  padding: 0.4rem 0.55rem;
  border-radius: 0.4rem;
  border: 1px solid var(--ty-border);
  background: var(--ty-surface);
  color: var(--ty-text-muted);
  font-size: 0.75rem;
  font-weight: 500;
  line-height: 1.35;
  white-space: normal;
  box-shadow:
    0 4px 16px rgba(0, 0, 0, 0.14),
    0 1px 4px rgba(0, 0, 0, 0.08);
  pointer-events: auto;
  transform: translateY(-50%);
}

:global(.dark) .ty-select-floating-tip {
  box-shadow:
    0 4px 20px rgba(0, 0, 0, 0.45),
    0 1px 6px rgba(0, 0, 0, 0.25);
}
</style>
