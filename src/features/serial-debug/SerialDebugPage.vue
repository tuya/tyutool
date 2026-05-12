<script setup lang="ts">
import { computed, ref, onUnmounted } from 'vue';
import { useSerialDebugStore } from '@/stores/serial-debug';
import SerialDebugConnectionBar from './components/SerialDebugConnectionBar.vue';
import SerialDebugSplitPane from './components/SerialDebugSplitPane.vue';
import SerialDebugFilterBar from './components/SerialDebugFilterBar.vue';
import SerialDebugLogView from './components/SerialDebugLogView.vue';
import SerialDebugSubWindowPanel from './components/SerialDebugSubWindowPanel.vue';
import SerialDebugSendBar from './components/SerialDebugSendBar.vue';
import RxSelectionHexPopup from './components/RxSelectionHexPopup.vue';

const s = useSerialDebugStore();

const activeSubWindowId = ref<string | null>(null);
const subWindowNames = computed(() => s.subWindows.map((sw) => sw.name));

function handleCreate({ name, useRegex }: { name: string; useRegex: boolean }): void {
  const result = s.addSubWindow(name, useRegex);
  if (result === 'ok') {
    activeSubWindowId.value = s.subWindows[s.subWindows.length - 1].id;
  }
}

function handleCloseTab(id: string): void {
  s.removeSubWindow(id);
  if (activeSubWindowId.value === id) {
    activeSubWindowId.value = s.subWindows.length > 0
      ? s.subWindows[s.subWindows.length - 1].id
      : null;
  }
}

function handleClearTab(id: string): void {
  const sw = s.subWindows.find((w) => w.id === id);
  if (sw) sw.lines.splice(0);
}

onUnmounted(() => {
  if (s.open) void s.closePort();
});
</script>

<template>
  <div class="flex min-h-0 min-w-0 w-full flex-1 flex-col gap-2">
    <SerialDebugConnectionBar />
    <SerialDebugSplitPane class="flex-1 min-h-0">
      <template #left>
        <SerialDebugFilterBar
          :existingNames="subWindowNames"
          @create="handleCreate"
        />
        <SerialDebugLogView
          :lines="s.lines"
          :hexView="s.hexView"
          :hexBytesPerRow="s.hexBytesPerRow"
          :ansiEnabled="s.ansiEnabled"
          exportTitle="serial-debug-main"
          @clear="s.clear()"
        />
      </template>
      <template #right>
        <SerialDebugSubWindowPanel
          :subWindows="s.subWindows"
          :activeId="activeSubWindowId"
          :hexView="s.hexView"
          :hexBytesPerRow="s.hexBytesPerRow"
          :ansiEnabled="s.ansiEnabled"
          @selectTab="activeSubWindowId = $event"
          @closeTab="handleCloseTab"
          @clearTab="handleClearTab"
        />
      </template>
    </SerialDebugSplitPane>
    <SerialDebugSendBar />
    <RxSelectionHexPopup />
  </div>
</template>
