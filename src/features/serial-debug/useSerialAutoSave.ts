import { onActivated, onDeactivated, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { useSerialDebugStore } from "@/stores/serial-debug";
import { sanitizePortName, makeStamp, formatTs } from "./utils";
import { stripAnsi } from "./ansi-parse";

type SerialDebugStore = ReturnType<typeof useSerialDebugStore>;

/**
 * Drive the periodic flush of the live serial buffer to disk for the
 * SerialDebugPage. Lifecycle:
 *   - port open               → create file, start a 5 s flush interval
 *   - port close              → final flush, drop the path
 *   - autoSave toggle / dir   → start/stop matching open state
 *   - onActivated (return to page)  → resume flushing; lines that arrived while away get picked up next tick
 *   - onDeactivated (leave page)    → final flush, keep file if port still open
 *
 * Kept out of the page component so the component template stays focused
 * on layout. Tauri-only (the invoke() call); harmless in web/dev runtime
 * because s.autoSaveDir is never populated there.
 */
export function useSerialAutoSave(s: SerialDebugStore): void {
  const { t } = useI18n();

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
    const content =
      newLines
        .map((l) => {
          const dir =
            l.direction === "tx" ? "TX " : l.direction === "rx" ? "RX " : "SYS";
          if (s.autoSaveTimestamp) {
            return `[${formatTs(l.tsMs)}] [${dir}] ${stripAnsi(l.text)}`;
          }
          return stripAnsi(l.text);
        })
        .join("\n") + "\n";

    const flushedUpToId = newLines[newLines.length - 1].id;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("append_text_file", { path, content });
      // Only advance the watermark — never roll it back if onActivated
      // already moved it forward.
      if (flushedUpToId > lastFlushedLineId) lastFlushedLineId = flushedUpToId;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      s.appendSysLine(t("serialDebug.autoSave.errWrite", { msg }));
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
    autoSaveInterval = setInterval(() => {
      void flush();
    }, 5000);
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
      lastFlushedLineId =
        s.lines.length > 0 ? s.lines[s.lines.length - 1].id : 0;
      stopInterval();
      autoSaveInterval = setInterval(() => {
        void flush();
      }, 5000);
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
}
