<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import SerialDebugLogView from './SerialDebugLogView.vue';
import type { SubWindow, HexBytesPerRow } from '@/features/serial-debug/types';
import { MAX_SUB_WINDOW_NAME_LENGTH } from '@/features/serial-debug/constants';

const props = defineProps<{
  subWindows: SubWindow[];
  activeId: string | null;
  hexView: boolean;
  hexBytesPerRow: HexBytesPerRow;
  ansiEnabled: boolean;
}>();

const emit = defineEmits<{
  selectTab: [id: string];
  closeTab: [id: string];
  clearTab: [id: string];
}>();

const { t } = useI18n();

const activeWindow = computed(() => props.subWindows.find(sw => sw.id === props.activeId));
const activeLines = computed(() => activeWindow.value?.lines ?? []);

function tabLabel(name: string): string {
  return name.length > MAX_SUB_WINDOW_NAME_LENGTH
    ? name.slice(0, MAX_SUB_WINDOW_NAME_LENGTH) + '…'
    : name;
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!-- Empty state -->
    <template v-if="subWindows.length === 0">
      <div class="flex h-full items-center justify-center text-sm text-[var(--ty-text-muted)]">
        {{ t('serialDebug.subWindow.emptyHint', 'Add a filter keyword from the left pane to start split-view') }}
      </div>
    </template>

    <!-- Tabs + log view -->
    <template v-else>
      <!-- Tab bar -->
      <div
        role="tablist"
        class="ops-tabs flex shrink-0 flex-wrap gap-1 rounded-xl p-1"
      >
        <button
          v-for="sw in subWindows"
          :key="sw.id"
          type="button"
          role="tab"
          :aria-selected="sw.id === activeId"
          :title="sw.name"
          class="ops-tab flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-sm transition-all duration-200"
          :class="sw.id === activeId ? 'ops-tab-active' : 'ops-tab-inactive'"
          @click="emit('selectTab', sw.id)"
        >
          <span>{{ tabLabel(sw.name) }}</span>
          <span
            class="flex size-4 shrink-0 items-center justify-center rounded hover:bg-[color-mix(in_srgb,currentColor_15%,transparent)]"
            :title="t('serialDebug.subWindow.closeTab', 'Close')"
            @click.stop="emit('closeTab', sw.id)"
          >×</span>
        </button>
      </div>

      <!-- Active sub-window log view -->
      <SerialDebugLogView
        :key="activeId ?? ''"
        :lines="activeLines"
        :hex-view="hexView"
        :hex-bytes-per-row="hexBytesPerRow"
        :ansi-enabled="ansiEnabled"
        :export-title="activeWindow?.name"
        @clear="emit('clearTab', activeId ?? '')"
      />
    </template>
  </div>
</template>
