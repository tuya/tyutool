<script setup lang="ts">
/**
 * Shared connection bar shell used by firmware-flash and serial-debug.
 *
 * Gives both features the same surface, spacing and named slots:
 *   - icon    optional 40×40 icon wrapped in .conn-icon-wrap
 *   - status  label + status dot + text (left side, after icon)
 *   - fields  main form area (grows to fill)
 *   - actions optional right-aligned action buttons
 *
 * Visual sizing is unified — both consumers used to drift on padding
 * (p-3/3.5 vs p-2/2.5), which caused a small layout jitter when the
 * sidebar route changed.
 */

defineProps<{
  ariaLabel?: string;
}>();
</script>

<template>
  <section
    class="conn-bar flex min-w-0 shrink-0 flex-wrap items-center gap-x-4 gap-y-3 p-3"
    :aria-label="ariaLabel"
  >
    <!-- Status block (icon + label + dot + text + divider) -->
    <div class="relative flex shrink-0 items-center gap-3">
      <slot name="icon" />
      <div class="shrink-0">
        <slot name="status" />
      </div>
      <div
        class="conn-divider ml-1 hidden h-8 w-px shrink-0 sm:block"
        aria-hidden="true"
      />
    </div>

    <!-- Fields (grows) -->
    <div
      class="conn-fields flex min-w-0 flex-[1_1_20rem] flex-wrap items-center gap-x-3 gap-y-2"
    >
      <slot name="fields" />
    </div>

    <!-- Actions (optional, right-aligned) -->
    <div
      v-if="$slots.actions"
      class="conn-actions ml-auto flex min-w-0 flex-wrap items-center gap-2"
    >
      <slot name="actions" />
    </div>
  </section>
</template>
