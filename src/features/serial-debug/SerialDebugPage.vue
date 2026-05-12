<script setup lang="ts">
import { onUnmounted } from 'vue';
import { useSerialDebugStore } from '@/stores/serial-debug';
import SerialDebugConnectionBar from './components/SerialDebugConnectionBar.vue';
import SerialDebugLogView from './components/SerialDebugLogView.vue';
import SerialDebugSendBar from './components/SerialDebugSendBar.vue';
import RxSelectionHexPopup from './components/RxSelectionHexPopup.vue';

const s = useSerialDebugStore();

onUnmounted(() => {
  if (s.open) void s.closePort();
});
</script>

<template>
  <div class="flex min-h-0 min-w-0 w-full flex-1 flex-col gap-2">
    <SerialDebugConnectionBar />
    <SerialDebugLogView
      :lines="s.lines"
      :hexView="s.hexView"
      :hexBytesPerRow="s.hexBytesPerRow"
      :ansiEnabled="s.ansiEnabled"
      exportTitle="serial-debug-main"
      @clear="s.clear()"
    />
    <SerialDebugSendBar />
    <RxSelectionHexPopup />
  </div>
</template>
