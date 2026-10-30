<script setup lang="ts">
defineProps<{ title: string; section?: string; description?: string }>();
</script>

<template>
  <header
    class="page-header flex min-w-0 shrink-0 items-center justify-between gap-4 py-1"
  >
    <div class="min-w-0">
      <p v-if="section" class="page-header-section mb-1">{{ section }}</p>
      <h1 class="page-header-title">{{ title }}</h1>
      <p v-if="description || $slots.default" class="page-header-desc mt-1">
        <slot>{{ description }}</slot>
      </p>
    </div>
    <div v-if="$slots.actions" class="flex shrink-0 items-center gap-2">
      <slot name="actions" />
    </div>
  </header>
</template>
