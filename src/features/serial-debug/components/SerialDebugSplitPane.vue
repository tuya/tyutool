<script setup lang="ts">
import { Splitpanes, Pane } from 'splitpanes'
import 'splitpanes/dist/splitpanes.css'
import { ref, onMounted } from 'vue'

const STORAGE_KEY = 'serial-debug-split-ratio'
const DEFAULT_SPLIT = 50

const leftSize = ref(DEFAULT_SPLIT)

onMounted(() => {
  const stored = localStorage.getItem(STORAGE_KEY)
  if (stored) {
    const n = parseFloat(stored)
    if (n > 10 && n < 90) leftSize.value = n
  }
})

function onResize(sizes: { size: number }[]) {
  if (sizes[0]) {
    localStorage.setItem(STORAGE_KEY, String(sizes[0].size))
  }
}
</script>

<template>
  <Splitpanes class="ty-splitpanes flex-1 min-h-0" @resize="onResize">
    <Pane :size="leftSize" :min-size="20">
      <div class="h-full flex flex-col overflow-hidden">
        <slot name="left" />
      </div>
    </Pane>
    <Pane :size="100 - leftSize" :min-size="20">
      <div class="h-full flex flex-col overflow-hidden">
        <slot name="right" />
      </div>
    </Pane>
  </Splitpanes>
</template>

<style scoped>
/* Override splitpanes splitter to match ty-* theme */
.ty-splitpanes :deep(.splitpanes__splitter) {
  background-color: var(--ty-border);
  position: relative;
  flex-shrink: 0;
  box-sizing: border-box;
  width: 5px;
  cursor: col-resize;
  transition: background-color 0.15s ease;
}

.ty-splitpanes :deep(.splitpanes__splitter:hover) {
  background-color: var(--ty-primary);
}

/* Remove default-theme pseudo-element handles; use plain bar only */
.ty-splitpanes :deep(.splitpanes__splitter::before),
.ty-splitpanes :deep(.splitpanes__splitter::after) {
  display: none;
}
</style>
