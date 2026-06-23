<script setup lang="ts">
defineOptions({ name: "SerialDebugPage" });
import { useSerialDebugStore } from "@/stores/serial-debug";
import { useSerialAutoSave } from "./useSerialAutoSave";
import SerialDebugConnectionBar from "./components/SerialDebugConnectionBar.vue";
import SerialDebugLogView from "./components/SerialDebugLogView.vue";
import SerialDebugSendBar from "./components/SerialDebugSendBar.vue";
import SerialDebugRxSelectionHexPopup from "./components/SerialDebugRxSelectionHexPopup.vue";

const s = useSerialDebugStore();
useSerialAutoSave(s);
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
    <SerialDebugRxSelectionHexPopup />
  </div>
</template>
