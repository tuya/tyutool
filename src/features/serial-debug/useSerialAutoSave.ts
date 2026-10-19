import { onUnmounted, watch } from "vue";
import { useI18n } from "vue-i18n";
import { invoke } from "@tauri-apps/api/core";
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
 *   - page deactivation       → keep flushing while the serial session remains open
 *   - component unmount       → stop the interval and unregister store callbacks
 *
 * Kept out of the page component so the component template stays focused
 * on layout. Tauri-only (the invoke() call); harmless in web/dev runtime
 * because s.autoSaveDir is never populated there.
 */
export function useSerialAutoSave(s: SerialDebugStore): void {
  const { t } = useI18n();
  const transport = serialDebugTransport();

  let autoSaveInterval: ReturnType<typeof setInterval> | null = null;
  let currentFlushPromise: Promise<void> | null = null;
  let backfillPromise: Promise<void> | null = null;
  let transition = Promise.resolve();
  let autoSaveWriteError: Error | null = null;
  let sessionAutoSaveDir: string | null = null;

  if (s.sessionAutoSavePath) {
    const portPath = `/${sanitizePortName(s.port)}/`;
    const portPathIndex = s.sessionAutoSavePath.lastIndexOf(portPath);
    sessionAutoSaveDir =
      portPathIndex >= 0 ? s.sessionAutoSavePath.slice(0, portPathIndex) : null;
  }

  function enqueueTransition(task: () => Promise<void>): void {
    transition = transition.then(task, task).catch((e) => {
      rLog.warn(
        `[SerialDebug] auto-save transition failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    });
  }

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
   * Handoff with the live queue — neither duplicated nor gapped:
   * `sessionAutoSavePath` is set *before* this runs, so live queuing has already
   * started when the snapshot (`totalLines` of the first page) is taken, and the
   * two halves therefore overlap in time. The overlap is resolved by position,
   * not by timing: every live line carries the archive position it was written
   * at (`DebugChunk.archivedBefore`), so the store can discard exactly the
   * queued lines this backfill already covers — no more, no less. See
   * `dropBackfilledAutoSaveLines`.
   *
   * The discard is handed the number of archive lines this backfill *actually
   * wrote*, not the snapshot: a page that fails halfway (unreadable archive,
   * rejected write) then still leaves whatever the live queue holds of the
   * missing range in the file, instead of discarding it as "already written".
   * Nothing live can reach the file before the backfill finishes (`flush` awaits
   * `backfillPromise`), so waiting until then costs no ordering.
   */
  async function backfillFromArchive(path: string): Promise<void> {
    let start = 0;
    let total: number | null = null;
    try {
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
    } finally {
      // A sys line reaches the live queue before it reaches the archive, so its
      // position can still be in flight; wait for those answers, or the discard
      // would have to guess.
      await s.settleAutoSaveMarkers();
      if (s.sessionAutoSavePath === path) {
        s.dropBackfilledAutoSaveLines(start);
      }
    }
  }

  async function flush(
    drainAll = false,
    throughSeq = s.autoSaveQueueHighWaterMark(),
    propagateFailure = false,
  ): Promise<void> {
    if (currentFlushPromise) {
      try {
        await currentFlushPromise;
      } catch (e) {
        if (propagateFailure) throw e;
        return;
      }
      if (
        propagateFailure &&
        autoSaveWriteError &&
        s.peekPendingAutoSaveLines(1, throughSeq).length > 0
      ) {
        throw autoSaveWriteError;
      }
      return flush(drainAll, throughSeq, propagateFailure);
    }

    const current = (async () => {
      try {
        // Ordering guarantee: nothing live may reach the file before the
        // pre-enable backfill has finished, otherwise the file would read
        // newest-then-oldest around the handoff.
        if (backfillPromise) await backfillPromise;
        let appends = 0;
        while (true) {
          const path = s.sessionAutoSavePath;
          if (!path) break;
          const newLines = s.peekPendingAutoSaveLines(
            AUTO_SAVE_FLUSH_MAX_CHARS,
            throughSeq,
          );
          if (newLines.length === 0) break;
          await invoke("append_text_file", {
            path,
            content: formatAutoSaveBlock(newLines),
          });
          s.commitPendingAutoSaveLines(newLines.length);
          appends += 1;
          // A periodic flush yields after its share. A close or boundary flush
          // drains only its captured sequence range, so later live input cannot
          // hold the operation open forever.
          if (!drainAll && appends >= AUTO_SAVE_FLUSH_MAX_APPENDS_PER_TICK)
            break;
        }
        if (
          Number.isFinite(throughSeq) &&
          s.peekPendingAutoSaveLines(1, throughSeq).length > 0 &&
          !s.sessionAutoSavePath
        ) {
          throw (
            autoSaveWriteError ?? new Error("Auto-save session is unavailable")
          );
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        autoSaveWriteError = e instanceof Error ? e : new Error(msg);
        s.appendSysLine(t("serialDebug.autoSave.errWrite", { msg }));
        stopInterval();
        s.sessionAutoSavePath = null;
        sessionAutoSaveDir = null;
        if (propagateFailure) throw e;
      }
    })();
    currentFlushPromise = current;
    try {
      await current;
    } finally {
      if (currentFlushPromise === current) currentFlushPromise = null;
    }
  }

  /**
   * Renew the write authorization for the auto-save directory.
   *
   * The Rust-side `DialogPathRegistry` only authorizes writes for 10 min after
   * the last write to a registered path, and the dir is otherwise registered
   * only when the user picks it (or when the workspace is restored). Never
   * rejects: a failed renewal is not itself actionable, and the first append
   * will surface the refusal through `serialDebug.autoSave.errWrite`.
   */
  async function registerAutoSaveDir(dir: string): Promise<void> {
    try {
      await invoke("register_dialog_path", { path: dir });
    } catch (e) {
      rLog.warn(
        `[SerialDebug] auto-save dir re-register failed: ${
          e instanceof Error ? e.message : String(e)
        }`,
      );
    }
  }

  function startAutoSave(): void {
    if (!s.autoSave || !s.autoSaveDir) return;
    // Idempotent: when autoSave, autoSaveDir and open all flip in one tick both
    // watchers below fire, and Vue runs them in trigger order — so this can be
    // reached twice for the same session. A second call would start a second
    // backfill whose bookkeeping races the first one's.
    if (s.sessionAutoSavePath !== null) return;
    stopInterval();
    const dir = s.autoSaveDir;
    const portDir = sanitizePortName(s.port);
    const filename = `serial-debug-${makeStamp()}-${crypto.randomUUID()}.txt`;
    // path separator: Tauri on all platforms accepts forward slash
    const path = `${dir}/${portDir}/${filename}`;
    autoSaveWriteError = null;
    sessionAutoSaveDir = dir;
    s.sessionAutoSavePath = path;
    // The dialog-path grant that authorizes writes under the auto-save dir
    // expires 10 min after the last authorized write, so a session started
    // after a long idle gap (port closed, reopened much later) needs a fresh
    // grant before the first append. Everything that writes goes through
    // backfillPromise first, so chaining here is enough to order it.
    backfillPromise = registerAutoSaveDir(dir)
      .then(() => backfillFromArchive(path))
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

  async function finalFlushAndStop(): Promise<void> {
    stopInterval();
    await flush(true);
    s.sessionAutoSavePath = null;
    sessionAutoSaveDir = null;
  }

  s.registerBeforeClearAutoSave((throughSeq) => {
    if (!s.sessionAutoSavePath && !autoSaveWriteError) return Promise.resolve();
    return flush(true, throughSeq, true);
  });
  s.registerAfterBackendDrainAutoSave((throughSeq) => flush(true, throughSeq));
  onUnmounted(() => {
    s.registerBeforeClearAutoSave(null);
    s.registerAfterBackendDrainAutoSave(null);
    stopInterval();
  });

  // Start/stop when port opens or closes
  watch(
    () => s.open,
    (isOpen) => {
      enqueueTransition(async () => {
        if (isOpen) {
          if (s.autoSave && s.autoSaveDir && !s.sessionAutoSavePath)
            startAutoSave();
        } else if (s.sessionAutoSavePath) {
          await finalFlushAndStop();
        }
      });
    },
  );

  // Start/stop when user toggles autoSave or changes directory mid-session
  watch([() => s.autoSave, () => s.autoSaveDir], ([newAutoSave, newDir]) => {
    enqueueTransition(async () => {
      const path = s.sessionAutoSavePath;
      if (path && (!newAutoSave || !newDir || sessionAutoSaveDir !== newDir)) {
        await flush(true, s.autoSaveQueueHighWaterMark(), true);
        s.sessionAutoSavePath = null;
        sessionAutoSaveDir = null;
      }
      if (newAutoSave && newDir && s.open && !s.sessionAutoSavePath) {
        startAutoSave();
      }
    });
  });
}
