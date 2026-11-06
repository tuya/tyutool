<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  useId,
  watch,
} from "vue";
import { useI18n } from "vue-i18n";
import { useFloatingPanel } from "@/composables/useFloatingPanel";

defineOptions({ inheritAttrs: false });

const MIN_BAUD_RATE = 1;
const MAX_BAUD_RATE = 0xffffffff;
const MAX_BAUD_RATE_LENGTH = String(MAX_BAUD_RATE).length;

const props = withDefaults(
  defineProps<{
    id: string;
    modelValue: number;
    options: readonly number[];
    disabled?: boolean;
  }>(),
  {
    disabled: false,
  },
);

const emit = defineEmits<{
  (event: "update:modelValue", value: number): void;
  (event: "validity-change", value: boolean): void;
}>();

const { t } = useI18n();
const rootRef = ref<HTMLElement | null>(null);
const inputRef = ref<HTMLInputElement | null>(null);
const listRef = ref<HTMLUListElement | null>(null);
const feedbackRef = ref<HTMLElement | null>(null);
const componentId = useId();
const listId = `ty-baud-list-${componentId}`;
const feedbackId = `ty-baud-feedback-${componentId}`;
const hintId = `ty-baud-hint-${componentId}`;

const open = ref(false);
const activeIndex = ref(-1);
const draft = ref(String(props.modelValue));
const lastValid = ref(props.modelValue);
const focusValue = ref(props.modelValue);
const focused = ref(false);
const invalid = ref(false);
const feedbackVisible = ref(false);

const { style: listStyle } = useFloatingPanel(
  listRef,
  () => open.value,
  () => rootRef.value?.getBoundingClientRect() ?? null,
);
const { style: feedbackStyle } = useFloatingPanel(
  feedbackRef,
  () => feedbackVisible.value,
  () => rootRef.value?.getBoundingClientRect() ?? null,
);

const describedBy = computed(() =>
  feedbackVisible.value ? feedbackId : hintId,
);
const inputTitle = computed(() =>
  feedbackVisible.value ? t("common.baud.invalid") : t("common.baud.inputHint"),
);

function parseBaud(value: string): number | null {
  if (!/^\d+$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) &&
    parsed >= MIN_BAUD_RATE &&
    parsed <= MAX_BAUD_RATE
    ? parsed
    : null;
}

watch(
  () => parseBaud(draft.value) !== null,
  (valid) => emit("validity-change", valid),
  { immediate: true },
);

function clearFeedback() {
  feedbackVisible.value = false;
}

function hideValidation() {
  invalid.value = false;
  clearFeedback();
}

function showValidation() {
  invalid.value = true;
  feedbackVisible.value = true;
}

function closeList() {
  open.value = false;
  activeIndex.value = -1;
}

function showList() {
  if (props.disabled) return;
  hideValidation();
  open.value = true;
  const selected = props.options.findIndex((rate) => rate === lastValid.value);
  activeIndex.value = selected >= 0 ? selected : props.options.length ? 0 : -1;
  inputRef.value?.focus({ preventScroll: true });
}

function toggleList() {
  if (open.value) closeList();
  else showList();
}

function commitDraft() {
  const parsed = parseBaud(draft.value);
  if (parsed === null) {
    draft.value = String(lastValid.value);
    showValidation();
    return false;
  }
  draft.value = String(parsed);
  lastValid.value = parsed;
  hideValidation();
  if (parsed !== props.modelValue) emit("update:modelValue", parsed);
  return true;
}

function onInput(event: Event) {
  const input = event.target as HTMLInputElement;
  draft.value = input.value;

  const parsed = parseBaud(input.value);
  invalid.value = parsed === null;
  clearFeedback();
  if (parsed !== null && parsed !== props.modelValue) {
    emit("update:modelValue", parsed);
  }
}

function selectRate(rate: number) {
  draft.value = String(rate);
  lastValid.value = rate;
  hideValidation();
  closeList();
  if (rate !== props.modelValue) emit("update:modelValue", rate);
  inputRef.value?.focus({ preventScroll: true });
}

function moveActive(direction: 1 | -1) {
  if (!props.options.length) return;
  const current = activeIndex.value;
  if (current < 0) {
    activeIndex.value = direction > 0 ? 0 : props.options.length - 1;
    return;
  }
  activeIndex.value = Math.max(
    0,
    Math.min(props.options.length - 1, current + direction),
  );
}

function onKeydown(event: KeyboardEvent) {
  if (props.disabled || event.isComposing || event.keyCode === 229) return;

  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    if (!open.value) showList();
    else moveActive(event.key === "ArrowDown" ? 1 : -1);
    return;
  }

  if (event.key === "Enter") {
    event.preventDefault();
    if (open.value && activeIndex.value >= 0) {
      selectRate(props.options[activeIndex.value]);
    } else {
      commitDraft();
      closeList();
    }
    return;
  }

  if (event.key === "Escape") {
    event.preventDefault();
    draft.value = String(focusValue.value);
    lastValid.value = focusValue.value;
    hideValidation();
    closeList();
    if (focusValue.value !== props.modelValue) {
      emit("update:modelValue", focusValue.value);
    }
    inputRef.value?.focus({ preventScroll: true });
    return;
  }

  if (event.key === "Tab") closeList();
}

function onBlur() {
  focused.value = false;
  if (props.disabled) {
    hideValidation();
    closeList();
    return;
  }
  commitDraft();
  closeList();
}

function onFocus() {
  focused.value = true;
  focusValue.value = props.modelValue;
  hideValidation();
}

function onDocumentMousedown(event: MouseEvent) {
  const target = event.target as Node;
  if (rootRef.value?.contains(target) || listRef.value?.contains(target)) {
    return;
  }
  closeList();
}

function keepInputFocus(event: MouseEvent) {
  event.preventDefault();
}

watch(
  () => props.modelValue,
  (value) => {
    draft.value = String(value);
    lastValid.value = value;
    if (!focused.value) focusValue.value = value;
    hideValidation();
  },
  { immediate: true },
);

watch(
  () => props.disabled,
  (disabled) => {
    if (!disabled) return;
    closeList();
    hideValidation();
  },
);

watch([open, activeIndex], async () => {
  if (!open.value || activeIndex.value < 0) return;
  await nextTick();
  listRef.value
    ?.querySelector<HTMLElement>(`[data-index="${activeIndex.value}"]`)
    ?.scrollIntoView({ block: "nearest" });
});

onMounted(() => {
  document.addEventListener("mousedown", onDocumentMousedown);
});

onBeforeUnmount(() => {
  document.removeEventListener("mousedown", onDocumentMousedown);
  clearFeedback();
});
</script>

<template>
  <div
    ref="rootRef"
    v-bind="$attrs"
    class="ty-baud-combobox conn-select min-w-0"
    :class="{
      'ty-baud-combobox-disabled': disabled,
      'ty-baud-combobox-open': open,
    }"
  >
    <input
      :id="id"
      ref="inputRef"
      :value="draft"
      type="text"
      role="combobox"
      class="ty-baud-input min-w-0"
      inputmode="numeric"
      pattern="[0-9]*"
      :maxlength="MAX_BAUD_RATE_LENGTH"
      autocomplete="off"
      spellcheck="false"
      :placeholder="t('common.baud.inputHint')"
      :title="inputTitle"
      :disabled="disabled"
      :aria-label="t('common.baud.label')"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-controls="listId"
      :aria-activedescendant="
        open && activeIndex >= 0 ? `${listId}-${activeIndex}` : undefined
      "
      :aria-invalid="invalid"
      :aria-describedby="describedBy"
      @input="onInput"
      @keydown="onKeydown"
      @focus="onFocus"
      @blur="onBlur"
    />
    <button
      type="button"
      class="ty-baud-arrow"
      tabindex="-1"
      :disabled="disabled"
      :aria-label="t('common.baud.commonValues')"
      :title="t('common.baud.commonValues')"
      @mousedown="keepInputFocus"
      @click="toggleList"
    >
      <svg
        width="10"
        height="6"
        viewBox="0 0 10 6"
        fill="none"
        aria-hidden="true"
      >
        <path
          d="M1 1l4 4 4-4"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>
    <span :id="hintId" class="sr-only">{{ t("common.baud.inputHint") }}</span>
  </div>

  <Teleport to="body">
    <ul
      v-if="open"
      :id="listId"
      ref="listRef"
      role="listbox"
      class="ty-baud-list"
      :aria-label="t('common.baud.commonValues')"
      :style="listStyle"
    >
      <li
        v-for="(rate, index) in options"
        :id="`${listId}-${index}`"
        :key="rate"
        :data-index="index"
        role="option"
        class="ty-baud-option"
        :class="{
          'ty-baud-option-active': index === activeIndex,
          'ty-baud-option-selected': rate === lastValid,
        }"
        :aria-selected="rate === lastValid"
        @mouseenter="activeIndex = index"
        @mousedown.prevent="selectRate(rate)"
      >
        {{ rate }}
      </li>
    </ul>
  </Teleport>

  <Teleport to="body">
    <div
      v-if="feedbackVisible"
      :id="feedbackId"
      ref="feedbackRef"
      role="alert"
      class="ty-baud-feedback"
      :style="feedbackStyle"
    >
      {{ t("common.baud.invalid") }}
    </div>
  </Teleport>
</template>

<style scoped>
.ty-baud-combobox {
  position: relative;
  display: flex;
  align-items: center;
  padding: 0;
  overflow: hidden;
  text-align: left;
}

.ty-baud-combobox:focus-within {
  border-color: var(--ty-primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--ty-primary) 18%, transparent);
}

.ty-baud-combobox-disabled {
  cursor: not-allowed;
  opacity: 0.5;
}

.ty-baud-input {
  flex: 1;
  height: 100%;
  min-width: 0;
  border: 0;
  outline: 0;
  background: transparent;
  padding: 0 0.375rem 0 0.625rem;
  color: var(--ty-text);
  font: inherit;
  font-variant-numeric: tabular-nums;
}

.ty-baud-input::placeholder {
  color: var(--ty-text-muted);
  opacity: 0.8;
}

.ty-baud-input:disabled {
  cursor: not-allowed;
}

.ty-baud-arrow {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  align-self: stretch;
  width: 1.75rem;
  flex: 0 0 1.75rem;
  border: 0;
  border-left: 1px solid var(--ty-border);
  background: transparent;
  color: var(--ty-text-muted);
  cursor: pointer;
  transition: transform 0.18s ease;
}

.ty-baud-arrow:disabled {
  cursor: not-allowed;
}

.ty-baud-combobox-open {
  border-color: var(--ty-primary);
}

.ty-baud-combobox-open .ty-baud-arrow {
  transform: rotate(180deg);
}

.ty-baud-list {
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

.ty-baud-option {
  display: flex;
  align-items: center;
  padding: 0.375rem 0.625rem;
  border-radius: 0.375rem;
  color: var(--ty-text);
  font-size: 0.8125rem;
  font-weight: 500;
  font-variant-numeric: tabular-nums;
  cursor: pointer;
  user-select: none;
}

.ty-baud-option-active {
  background: color-mix(
    in srgb,
    var(--ty-primary) 10%,
    var(--ty-surface-muted)
  );
}

.ty-baud-option-selected {
  color: var(--ty-primary);
  font-weight: 600;
}

.ty-baud-feedback {
  position: fixed;
  z-index: 10050;
  min-width: 8rem;
  padding: 0.4rem 0.55rem;
  border-radius: 0.4rem;
  border: 1px solid var(--ty-danger);
  background: var(--ty-surface);
  color: var(--ty-danger);
  font-size: 0.75rem;
  font-weight: 500;
  line-height: 1.35;
  box-shadow:
    0 4px 16px rgba(0, 0, 0, 0.14),
    0 1px 4px rgba(0, 0, 0, 0.08);
}
</style>
