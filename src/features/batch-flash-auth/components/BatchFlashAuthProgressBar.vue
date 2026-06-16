<!-- src/features/batch-flash/components/BatchProgressBar.vue -->
<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  total: number;
  success: number;
  fail: number;
}>();

const successPct = computed(() =>
  props.total === 0 ? 0 : (props.success / props.total) * 100,
);
const failPct = computed(() =>
  props.total === 0 ? 0 : (props.fail / props.total) * 100,
);
</script>

<template>
  <div
    class="relative h-2 w-full overflow-hidden rounded-full"
    :style="{ backgroundColor: 'var(--ty-border)' }"
    role="progressbar"
    :aria-valuenow="success"
    :aria-valuemax="total"
  >
    <div
      class="absolute inset-y-0 left-0 transition-all duration-300"
      :style="{
        width: successPct + '%',
        backgroundColor: 'var(--ty-success)',
      }"
    />
    <div
      class="absolute inset-y-0 transition-all duration-300"
      :style="{
        left: successPct + '%',
        width: failPct + '%',
        backgroundColor: 'var(--ty-danger)',
      }"
    />
  </div>
</template>
