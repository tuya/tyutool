<script setup lang="ts">
defineOptions({ name: 'SerialDebugPage' })
import { onActivated, onDeactivated, watch } from 'vue';
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

  const flushedUpToId = newLines[newLines.length - 1].id;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('append_text_file', { path, content });
    // Only advance the watermark — never roll it back if onActivated already moved it forward.
    if (flushedUpToId > lastFlushedLineId) lastFlushedLineId = flushedUpToId;
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

async function finalFlushAndStop(keepSession = false): Promise<void> {
  stopInterval();
  await flush();
  if (!keepSession) {
    s.sessionAutoSavePath = null;
  }
}

onActivated(() => {
  if (s.open && s.sessionAutoSavePath) {
    // Resuming after navigation: all existing lines were already flushed on deactivate.
    // Restore the watermark so the next flush only picks up lines that arrived while away.
    lastFlushedLineId = s.lines.length > 0 ? s.lines[s.lines.length - 1].id : 0;
    stopInterval();
    autoSaveInterval = setInterval(() => { void flush(); }, 5000);
  } else if (s.open) {
    startAutoSave();
  }
});

onDeactivated(() => {
  // keepSession=true: port stays open across navigation, preserve the auto-save file path.
  void finalFlushAndStop(s.open);
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
