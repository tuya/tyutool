<script setup lang="ts">
const props = withDefaults(
  defineProps<{
    modelValue: boolean;
    disabled?: boolean;
    size?: "sm" | "md";
  }>(),
  {
    disabled: false,
    size: "md",
  },
);

const emit = defineEmits<{
  (e: "update:modelValue", value: boolean): void;
}>();

function toggle(): void {
  if (props.disabled) return;
  emit("update:modelValue", !props.modelValue);
}

function onKeydown(event: KeyboardEvent): void {
  if (props.disabled) return;
  if (event.key === " " || event.key === "Enter") {
    event.preventDefault();
    toggle();
  }
}
</script>

<template>
  <button
    type="button"
    role="switch"
    class="ty-switch"
    :class="[
      `ty-switch--${size}`,
      modelValue ? 'ty-switch--on' : 'ty-switch--off',
    ]"
    :aria-checked="modelValue"
    :disabled="disabled"
    @click="toggle"
    @keydown="onKeydown"
  >
    <span class="ty-switch__track" aria-hidden="true">
      <span class="ty-switch__thumb" />
    </span>
  </button>
</template>

<style scoped>
.ty-switch {
  display: inline-flex;
  min-height: 2.75rem;
  min-width: 2.75rem;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  cursor: pointer;
  color: var(--ty-text);
  transition: opacity 0.18s ease;
}

.ty-switch:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}

.ty-switch__track {
  position: relative;
  display: inline-flex;
  align-items: center;
  border-radius: 9999px;
  background: color-mix(
    in srgb,
    var(--ty-text-muted) 28%,
    var(--ty-surface-muted)
  );
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--ty-border) 84%, transparent),
    0 1px 2px rgba(15, 23, 42, 0.1);
  transition:
    background-color 0.2s ease,
    box-shadow 0.2s ease;
}

.ty-switch__thumb {
  position: absolute;
  left: 0.1875rem;
  border-radius: 9999px;
  background: #fff;
  box-shadow:
    0 1px 2px rgba(15, 23, 42, 0.2),
    0 0 0 1px rgba(255, 255, 255, 0.25);
  transition:
    transform 0.2s ease,
    width 0.2s ease,
    height 0.2s ease;
}

.ty-switch--sm .ty-switch__track {
  width: 2.25rem;
  height: 1.25rem;
}

.ty-switch--sm .ty-switch__thumb {
  width: 0.875rem;
  height: 0.875rem;
}

.ty-switch--md .ty-switch__track {
  width: 2.625rem;
  height: 1.5rem;
}

.ty-switch--md .ty-switch__thumb {
  width: 1.125rem;
  height: 1.125rem;
}

.ty-switch--sm.ty-switch--on .ty-switch__thumb {
  transform: translateX(1rem);
}

.ty-switch--md.ty-switch--on .ty-switch__thumb {
  transform: translateX(1.125rem);
}

.ty-switch--on .ty-switch__track {
  background: linear-gradient(
    135deg,
    var(--ty-primary) 0%,
    var(--ty-primary-hover) 100%
  );
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--ty-primary) 36%, transparent),
    0 3px 10px color-mix(in srgb, var(--ty-primary) 22%, transparent);
}

.ty-switch:focus-visible {
  outline: none;
  box-shadow:
    0 0 0 2px var(--ty-ring-offset),
    0 0 0 4px var(--ty-ring);
  border-radius: 9999px;
}

@media (prefers-reduced-motion: reduce) {
  .ty-switch,
  .ty-switch__track,
  .ty-switch__thumb {
    transition-duration: 0.01ms !important;
  }
}
</style>
