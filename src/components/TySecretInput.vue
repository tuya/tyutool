<script setup lang="ts">
import { computed, watch } from "vue";
import { useCopyFeedback } from "@/composables/useCopyFeedback";
import { useI18n } from "vue-i18n";

/**
 * Text input for sensitive credentials (device UUID, auth key, license, …).
 *
 * Differences from a plain <input>:
 *   - one-click copy button writes the value to the clipboard, swapping
 *     the icon to a checkmark for ~1.4s as confirmation
 *   - never auto-completed or remembered by the browser
 */

const props = defineProps<{
  modelValue: string;
  id: string;
  placeholder?: string;
  disabled?: boolean;
  ariaDescribedby?: string;
  ariaInvalid?: boolean;
  noCopy?: boolean;
  label?: string;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
}>();

const { t } = useI18n();

const { state, message, copy, reset } = useCopyFeedback();
const copied = computed(() => state.value === "copied");
const feedbackId = computed(() => `${props.id}-copy-feedback`);
const copyLabel = computed(() =>
  t("common.copyField", {
    field: props.label || props.placeholder || props.id,
  }),
);
watch(() => props.modelValue, reset);
function onInput(e: Event) {
  emit("update:modelValue", (e.target as HTMLInputElement).value);
}
function copyValue() {
  if (props.modelValue) void copy(props.modelValue);
}
</script>

<template>
  <div>
    <div class="relative flex items-stretch">
      <input
        :id="id"
        type="text"
        :value="modelValue"
        :placeholder="placeholder"
        :disabled="disabled"
        :aria-describedby="
          [ariaDescribedby, message ? feedbackId : undefined]
            .filter(Boolean)
            .join(' ') || undefined
        "
        :aria-invalid="ariaInvalid"
        :class="noCopy ? 'pr-2' : 'pr-9'"
        class="ops-text-input min-w-0 flex-1 font-mono py-1.5"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        autocorrect="off"
        data-1p-ignore
        data-lpignore="true"
        data-bwignore="true"
        @input="onInput"
      />
      <div
        v-if="!noCopy"
        class="pointer-events-none absolute inset-y-0 right-1.5 flex items-center"
      >
        <button
          type="button"
          class="pointer-events-auto flex size-7 items-center justify-center rounded-md transition-colors hover:bg-[var(--ty-surface-muted)] disabled:opacity-40"
          :class="
            copied
              ? 'text-[var(--ty-success)]'
              : 'text-[var(--ty-text-muted)] hover:text-[var(--ty-text)]'
          "
          :title="copied ? t('common.copied') : t('common.copy')"
          :aria-label="copyLabel"
          :disabled="disabled || !modelValue || state === 'copying'"
          @click="copyValue"
        >
          <FontAwesomeIcon
            :icon="['fas', copied ? 'check' : 'copy']"
            class="size-3.5"
            aria-hidden="true"
          />
        </button>
      </div>
    </div>
    <p
      v-if="message"
      :id="feedbackId"
      :role="state === 'failed' ? 'alert' : 'status'"
      class="mt-1 text-xs"
      :class="
        state === 'failed'
          ? 'text-[var(--ty-danger)]'
          : 'text-[var(--ty-success)]'
      "
    >
      {{ message }}
    </p>
  </div>
</template>
