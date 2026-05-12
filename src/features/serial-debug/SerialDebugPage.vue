<script setup lang="ts">
import { onUnmounted, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { sanitizePortName, makeStamp, formatTs } from '@/features/serial-debug/context';
import { stripAnsi } from '@/features/serial-debug/ansi-parse';
import SerialDebugConnectionBar from './components/SerialDebugConnectionBar.vue';
import SerialDebugLogView from './components/SerialDebugLogView.vue';
import SerialDebugSendBar from './components/SerialDebugSendBar.vue';
import RxSelectionHexPopup from './components/RxSelectionHexPopup.vue';

const s = useSerialDebugStore();
const { t } = useI18n();

// ── auto-save ─────────────────────────────────────────────────────────────

let autoSaveInterval: ReturnType<typeof setInterval> | null = null;
let lastFlushedLineId = 0;
let flushInFlight = false;

function stopInterval(): void {
  if (autoSaveInterval !== null) {
    clearInterval(autoSaveInterval);
    autoSaveInterval = null;
  }
}

async function flush(): Promise<void> {
  if (flushInFlight) return;
  const path = s.sessionAutoSavePath;
  if (!path) return;

  const newLines = s.lines.filter((l) => l.id > lastFlushedLineId);
  if (newLines.length === 0) return;

  flushInFlight = true;
  const content = newLines.map((l) => {
    const dir = l.direction === 'tx' ? 'TX ' : l.direction === 'rx' ? 'RX ' : 'SYS';
    if (s.autoSaveTimestamp) {
      return `[${formatTs(l.tsMs)}] [${dir}] ${stripAnsi(l.text)}`;
    }
    return stripAnsi(l.text);
  }).join('\n') + '\n';

  try {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('append_text_file', { path, content });
    lastFlushedLineId = newLines[newLines.length - 1].id;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    s.appendSysLine(t('serialDebug.autoSave.errWrite', { msg }));
    stopInterval();
    s.sessionAutoSavePath = null;
  } finally {
    flushInFlight = false;
  }
}

function startAutoSave(): void {
  if (!s.autoSave || !s.autoSaveDir) return;
  stopInterval();
  const portDir = sanitizePortName(s.port);
  const filename = `serial-debug-${makeStamp()}.txt`;
  // path separator: Tauri on all platforms accepts forward slash
  s.sessionAutoSavePath = `${s.autoSaveDir}/${portDir}/${filename}`;
  lastFlushedLineId = 0;
  autoSaveInterval = setInterval(() => { void flush(); }, 5000);
}

async function finalFlushAndStop(): Promise<void> {
  stopInterval();
  await flush();
  s.sessionAutoSavePath = null;
}

onUnmounted(() => {
  void finalFlushAndStop().then(() => {
    if (s.open) void s.closePort();
  });
});

// Start/stop when port opens or closes
watch(
  () => s.open,
  async (isOpen) => {
    if (isOpen) {
      startAutoSave();
    } else {
      await finalFlushAndStop();
    }
  },
);

// Start/stop when user toggles autoSave or changes directory mid-session
watch(
  [() => s.autoSave, () => s.autoSaveDir],
  async ([newAutoSave, newDir]) => {
    if (!newAutoSave || !newDir) {
      if (s.sessionAutoSavePath) await finalFlushAndStop();
    } else if (s.open && !s.sessionAutoSavePath) {
      startAutoSave();
    }
  },
);
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
