import { onActivated, onDeactivated, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { useSerialDebugStore } from "@/stores/serial-debug";
import { AUTO_SAVE_FLUSH_MAX_CHARS } from "./constants";
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
  let flushInFlight = false;
  let currentFlushPromise: Promise<void> | null = null;
  let replayFlushRequested = false;
  let replayFlushDrainAll = false;

  function stopInterval(): void {
    if (autoSaveInterval !== null) {
      clearInterval(autoSaveInterval);
      autoSaveInterval = null;
    }
  }

  async function flush(drainAll = false): Promise<void> {
    if (flushInFlight) {
      replayFlushRequested = true;
      replayFlushDrainAll ||= drainAll;
      await currentFlushPromise;
      return;
    }

    currentFlushPromise = (async () => {
      flushInFlight = true;
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        let keepDraining = drainAll;
        while (true) {
          const path = s.sessionAutoSavePath;
          if (!path) break;

          const newLines = s.drainPendingAutoSaveLines(
            AUTO_SAVE_FLUSH_MAX_CHARS,
          );
          if (newLines.length === 0) break;

          const content =
            newLines
              .map((l) => {
                const dir =
                  l.direction === "tx"
                    ? "TX "
                    : l.direction === "rx"
                      ? "RX "
                      : "SYS";
                if (s.autoSaveTimestamp) {
                  return `[${formatTs(l.tsMs)}] [${dir}] ${stripAnsi(l.text)}`;
                }
                return stripAnsi(l.text);
              })
              .join("\n") + "\n";

          await invoke("append_text_file", { path, content });

          if (keepDraining) continue;
          if (!replayFlushRequested) break;
          keepDraining = replayFlushDrainAll;
          replayFlushRequested = false;
          replayFlushDrainAll = false;
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        s.appendSysLine(t("serialDebug.autoSave.errWrite", { msg }));
        stopInterval();
        s.sessionAutoSavePath = null;
      } finally {
        flushInFlight = false;
        currentFlushPromise = null;
        if (replayFlushRequested && s.sessionAutoSavePath) {
          const rerunDrainAll = replayFlushDrainAll;
          replayFlushRequested = false;
          replayFlushDrainAll = false;
          await flush(rerunDrainAll);
        } else {
          replayFlushRequested = false;
          replayFlushDrainAll = false;
        }
      }
    })();

    await currentFlushPromise;
  }

  function startAutoSave(): void {
    if (!s.autoSave || !s.autoSaveDir) return;
    stopInterval();
    const portDir = sanitizePortName(s.port);
    const filename = `serial-debug-${makeStamp()}.txt`;
    // path separator: Tauri on all platforms accepts forward slash
    s.sessionAutoSavePath = `${s.autoSaveDir}/${portDir}/${filename}`;
    autoSaveInterval = setInterval(() => {
      void flush();
    }, 5000);
  }

  async function finalFlushAndStop(keepSession = false): Promise<void> {
    stopInterval();
    await flush(true);
    if (!keepSession) {
      s.sessionAutoSavePath = null;
    }
  }

  onActivated(() => {
    if (s.open && s.sessionAutoSavePath) {
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
