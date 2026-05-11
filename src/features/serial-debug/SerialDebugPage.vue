<script setup lang="ts">
import { onUnmounted } from 'vue';
import { useSerialDebugStore } from '@/stores/serial-debug';
import SerialDebugConnectionBar from './components/SerialDebugConnectionBar.vue';
import SerialDebugLogPane from './components/SerialDebugLogPane.vue';
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
    <SerialDebugLogPane class="flex-1 min-h-0" />
    <SerialDebugSendBar />
    <RxSelectionHexPopup />
  </div>
</template>
