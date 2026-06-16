<!-- src/features/batch-flash/components/BatchDonutChart.vue -->
<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  total: number;
  success: number;
  fail: number;
}>();

// r=48, circumference = 2π×48 ≈ 301.59
const C = 2 * Math.PI * 48;

const successArc = computed(() => {
  if (props.total === 0) return 0;
  return Math.min(C * (props.success / props.total), C);
});

const failArc = computed(() => {
  if (props.total === 0) return 0;
  return Math.min(C * (props.fail / props.total), C - successArc.value);
});

const pct = computed(() =>
  props.total === 0
    ? "-"
    : Math.round((props.success / props.total) * 100) + "%",
);
</script>

<template>
  <svg viewBox="0 0 120 120" class="w-20 h-20 shrink-0" aria-hidden="true">
    <!-- Background ring -->
    <circle
      cx="60"
      cy="60"
      r="48"
      fill="none"
      stroke="var(--ty-border)"
      stroke-width="14"
    />
    <!-- Success arc -->
    <circle
      v-if="total > 0"
      cx="60"
      cy="60"
      r="48"
      fill="none"
      stroke="var(--ty-success)"
      stroke-width="14"
      :stroke-dasharray="`${successArc} ${C - successArc}`"
      stroke-linecap="butt"
      transform="rotate(-90 60 60)"
    />
    <!-- Fail arc -->
    <circle
      v-if="total > 0 && failArc > 0"
      cx="60"
      cy="60"
      r="48"
      fill="none"
      stroke="var(--ty-danger)"
      stroke-width="14"
      :stroke-dasharray="`${failArc} ${C - failArc}`"
      :stroke-dashoffset="`${-successArc}`"
      stroke-linecap="butt"
      transform="rotate(-90 60 60)"
    />
    <!-- Center text -->
    <text
      x="60"
      y="56"
      text-anchor="middle"
      font-size="18"
      font-weight="600"
      fill="var(--ty-text)"
    >
      {{ pct }}
    </text>
    <text
      x="60"
      y="72"
      text-anchor="middle"
      font-size="11"
      fill="var(--ty-text-muted)"
    >
      成功率
    </text>
  </svg>
</template>
