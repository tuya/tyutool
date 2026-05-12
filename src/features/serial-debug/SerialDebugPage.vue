<script setup lang="ts">
import { onUnmounted, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { sanitizePortName } from '@/features/serial-debug/context';
import { stripAnsi } from '@/features/serial-debug/ansi-parse';
import SerialDebugConnectionBar from './components/SerialDebugConnectionBar.vue';
import SerialDebugLogView from './components/SerialDebugLogView.vue';
import SerialDebugSendBar from './components/SerialDebugSendBar.vue';
import RxSelectionHexPopup from './components/RxSelectionHexPopup.vue';

const s = useSerialDebugStore();
const { t } = useI18n();

onUnmounted(() => {
  if (s.open) void s.closePort();
});

// ── auto-save ─────────────────────────────────────────────────────────────

let autoSaveInterval: ReturnType<typeof setInterval> | null = null;
let lastFlushedLineId = 0;

function makeStamp(): string {
  const now = new Date();
  return `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, '0')}${String(now.getDate()).padStart(2, '0')}-${String(now.getHours()).padStart(2, '0')}${String(now.getMinutes()).padStart(2, '0')}${String(now.getSeconds()).padStart(2, '0')}`;
}

function formatTs(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const ss = String(d.getSeconds()).padStart(2, '0');
  const mmm = String(d.getMilliseconds()).padStart(3, '0');
  return `${hh}:${mm}:${ss}.${mmm}`;
}

function stopInterval(): void {
  if (autoSaveInterval !== null) {
    clearInterval(autoSaveInterval);
    autoSaveInterval = null;
  }
}

async function flush(): Promise<void> {
  const path = s.sessionAutoSavePath;
  if (!path) return;

  const newLines = s.lines.filter((l) => l.id > lastFlushedLineId);
  if (newLines.length === 0) return;

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
  }
}

function startAutoSave(): void {
  if (!s.autoSave || !s.autoSaveDir) return;
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

onUnmounted(() => {
  stopInterval();
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
