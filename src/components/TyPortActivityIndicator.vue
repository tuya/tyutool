<script setup lang="ts">
import { computed } from "vue";
import {
  resolvePortIndicatorColor,
  type FeaturePortIndicator,
  type IndicatorPaletteMode,
} from "@/features/serial-port-indicators/model";

const props = defineProps<{
  indicator: FeaturePortIndicator;
  activePorts: string[];
  paletteMode: IndicatorPaletteMode;
  feature?: string;
  surface?: string;
}>();

const firstPort = computed(() => props.indicator.ports[0] ?? "");
const tooltip = computed(() => props.indicator.ports.join(", "));
const visible = computed(
  () =>
    props.indicator.enabled &&
    props.indicator.active &&
    firstPort.value.trim().length > 0,
);
const dotColor = computed(() =>
  resolvePortIndicatorColor(
    firstPort.value,
    props.activePorts,
    props.paletteMode,
  ),
);
</script>

<template>
  <span
    v-if="visible"
    class="inline-flex items-center gap-1.5"
    :title="tooltip"
    :aria-label="tooltip"
    :data-port-indicator-feature="feature"
    :data-port-indicator-surface="surface"
  >
    <span
      class="inline-block size-2 shrink-0 rounded-full ring-1 ring-black/10 dark:ring-white/20"
      :style="{ backgroundColor: dotColor }"
      aria-hidden="true"
    />
    <span
      v-if="indicator.displayMode === 'count'"
      class="min-w-[1rem] text-[10px] font-semibold leading-none text-[var(--ty-text-muted)]"
    >
      {{ indicator.count }}
    </span>
  </span>
</template>
