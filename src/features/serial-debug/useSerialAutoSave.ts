import { onActivated, onDeactivated, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { useSerialDebugStore } from "@/stores/serial-debug";
import { rLog } from "@/utils/log";
import {
  AUTO_SAVE_BACKFILL_PAGE_SIZE,
  AUTO_SAVE_FLUSH_MAX_APPENDS_PER_TICK,
  AUTO_SAVE_FLUSH_MAX_CHARS,
} from "./constants";
import { serialDebugTransport } from "./transport";
import type { DebugLogLine } from "./types";
import { sanitizePortName, makeStamp, formatTs } from "./utils";
import { localizeArchiveLineText } from "./archive-line-text";
import { stripAnsi } from "./ansi-parse";

type SerialDebugStore = ReturnType<typeof useSerialDebugStore>;

/** The only fields the auto-save file format needs from a line. */
type AutoSaveLine = Pick<DebugLogLine, "direction" | "tsMs" | "text">;

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
  const transport = serialDebugTransport();

  let autoSaveInterval: ReturnType<typeof setInterval> | null = null;
  let flushInFlight = false;
  let currentFlushPromise: Promise<void> | null = null;
  let replayFlushRequested = false;
  let replayFlushDrainAll = false;
  let backfillPromise: Promise<void> | null = null;

  function stopInterval(): void {
    if (autoSaveInterval !== null) {
      clearInterval(autoSaveInterval);
      autoSaveInterval = null;
    }
  }

  /**
   * The auto-save file format. Shared by the live flush and the archive
   * backfill so that enabling auto-save mid-session cannot produce a file with
   * two different line formats (the backfilled part and the live part).
   */
  function formatAutoSaveBlock(batch: readonly AutoSaveLine[]): string {
    const withTimestamp = s.autoSaveTimestamp;
    return (
      batch
        .map((l) => {
          const dir =
            l.direction === "tx" ? "TX " : l.direction === "rx" ? "RX " : "SYS";
          // Both halves pass through here — the live queue and the archive
          // backfill — so the archive-cap sentinel is translated on either
          // route and never reaches the file verbatim.
          const text = stripAnsi(localizeArchiveLineText(l.direction, l.text));
          if (withTimestamp) {
            return `[${formatTs(l.tsMs)}] [${dir}] ${text}`;
          }
          return text;
        })
        .join("\n") + "\n"
    );
  }

  /**
   * Write the lines that already existed before auto-save was switched on.
   *
   * The complete session lives in the Rust-side archive (`.ndjson` + `.idx`),
   * which is the only full record — `pendingAutoSaveLines` is deliberately not
   * filled while no session file exists (it would grow ~11 MiB/min at 921600
   * with nothing draining it). Paged and streamed: one page is read, formatted
   * and appended before the next is requested, so memory stays bounded.
   *
   * Handoff with the live queue — deliberately "rather duplicate than gap":
   * `sessionAutoSavePath` is set *before* this runs, so live queuing has
   * already started when the snapshot (`totalLines` of the first page) is
   * taken. The backend appends a line to the archive before it emits the chunk
   * event, so every line the frontend has ever seen is already archived,
   * therefore inside the snapshot → no gap is possible. The flip side is that
   * lines archived just before the snapshot but only delivered to the frontend
   * after the path was set land in both halves and appear twice. That window
   * is one chunk-batch tick (16 ms in the store) plus one IPC round trip —
   * a few KiB at 921600 baud, tens of lines at worst.
   */
  async function backfillFromArchive(path: string): Promise<void> {
    const { invoke } = await import("@tauri-apps/api/core");
    let start = 0;
    let total: number | null = null;
    while (true) {
      const limit =
        total === null
          ? AUTO_SAVE_BACKFILL_PAGE_SIZE
          : Math.min(AUTO_SAVE_BACKFILL_PAGE_SIZE, total - start);
      if (limit <= 0) break;
      const page = await transport.readSessionPage(start, limit);
      if (total === null) total = page.totalLines;
      // Auto-save may have been switched off (or restarted) while the page was
      // in flight; that file is no longer ours to append to.
      if (s.sessionAutoSavePath !== path) return;
      if (page.items.length === 0) break;
      await invoke("append_text_file", {
        path,
        content: formatAutoSaveBlock(page.items),
      });
      start += page.items.length;
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
        // Ordering guarantee: nothing live may reach the file before the
        // pre-enable backfill has finished, otherwise the file would read
        // newest-then-oldest around the handoff.
        if (backfillPromise) await backfillPromise;
        let keepDraining = drainAll;
        let appends = 0;
        while (true) {
          const path = s.sessionAutoSavePath;
          if (!path) break;

          const newLines = s.drainPendingAutoSaveLines(
            AUTO_SAVE_FLUSH_MAX_CHARS,
          );
          if (newLines.length === 0) break;

          await invoke("append_text_file", {
            path,
            content: formatAutoSaveBlock(newLines),
          });
          appends += 1;

          if (replayFlushRequested) {
            keepDraining ||= replayFlushDrainAll;
            replayFlushRequested = false;
            replayFlushDrainAll = false;
          }
          // A periodic flush keeps draining too, otherwise one 128 KiB append
          // per 5 s tick falls permanently behind a fast port; it just yields
          // once it has done its share of the work.
          if (keepDraining) continue;
          if (appends >= AUTO_SAVE_FLUSH_MAX_APPENDS_PER_TICK) break;
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
    // Idempotent: when autoSave, autoSaveDir and open all flip in one tick both
    // watchers below fire, and Vue runs them in trigger order — so this can be
    // reached twice for the same session. A second call would start a second
    // backfill whose bookkeeping races the first one's.
    if (s.sessionAutoSavePath !== null) return;
    stopInterval();
    const portDir = sanitizePortName(s.port);
    const filename = `serial-debug-${makeStamp()}.txt`;
    // path separator: Tauri on all platforms accepts forward slash
    const path = `${s.autoSaveDir}/${portDir}/${filename}`;
    s.sessionAutoSavePath = path;
    backfillPromise = backfillFromArchive(path)
      .catch((e) => {
        // No archive yet (port never opened) or an unreadable one must not stop
        // auto-save: the live half still records from here on. Developer-only
        // diagnostic — the user has nothing to act on.
        rLog.warn(
          `[SerialDebug] auto-save backfill skipped: ${
            e instanceof Error ? e.message : String(e)
          }`,
        );
      })
      .finally(() => {
        backfillPromise = null;
      });
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
