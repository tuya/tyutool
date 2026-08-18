// @vitest-environment happy-dom
import { createPinia, setActivePinia } from "pinia";
import { createApp, defineComponent, nextTick } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SerialDebugTransport } from "./transport";
import { __setSerialDebugTransportForTest } from "./transport";
import {
  AUTO_SAVE_BACKFILL_PAGE_SIZE,
  AUTO_SAVE_FLUSH_MAX_CHARS,
} from "./constants";
import { useSerialDebugStore } from "@/stores/serial-debug";
import { useSerialAutoSave } from "./useSerialAutoSave";
import { formatTs } from "./utils";
import type { SerialDebugLine, SerialDebugSessionPage } from "./types";

const invokeSpy = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeSpy,
}));

vi.mock("vue-i18n", async (importOriginal) => {
  const actual = await importOriginal<typeof import("vue-i18n")>();
  return {
    ...actual,
    useI18n: () => ({
      t: (key: string, params?: { msg?: string }) =>
        params?.msg ? `${key}: ${params.msg}` : key,
    }),
  };
});

type SessionPageHandler = (
  start: number,
  limit: number,
) => Promise<SerialDebugSessionPage>;

/** Default: no session archive at all (port never opened). */
const noArchive: SessionPageHandler = async () => {
  throw new Error("no session");
};

let sessionPageHandler: SessionPageHandler = noArchive;

/**
 * What the fake archive answers when the store archives a `sys` line: the
 * `lineNo` it was written as, or null for "not archived". Set per test.
 */
let sysLineArchivePosition: number | null = null;

/** Holds the sys-line archive write open, to model a slow round trip. */
let sysLineGate: Promise<void> | null = null;

/** Serve `lines` as an archive; `growBy` fakes a session that keeps growing. */
function archivePages(
  lines: readonly SerialDebugLine[],
  growBy = 0,
): SessionPageHandler {
  let calls = 0;
  return async (start, limit) => {
    const totalLines = lines.length + calls * growBy;
    calls += 1;
    return {
      totalLines,
      start,
      items: lines.slice(start, start + limit),
    };
  };
}

function fakeTransport(): SerialDebugTransport {
  return {
    async open() {},
    async close() {},
    async send() {},
    async clearSession() {},
    async appendSysLine() {
      if (sysLineGate) await sysLineGate;
      return sysLineArchivePosition;
    },
    async addFilter() {
      throw new Error("not implemented");
    },
    async removeFilter() {},
    async readFilterMatches() {
      throw new Error("not implemented");
    },
    async readSessionPage(start, limit) {
      return await sessionPageHandler(start, limit);
    },
    async setArchiveLimit() {},
    onChunk() {
      return () => {};
    },
    onChunkBatch() {
      return () => {};
    },
    onDisconnect() {
      return () => {};
    },
    onFilterUpdated() {
      return () => {};
    },
    onArchiveCapped() {
      return () => {};
    },
  };
}

async function flushMicrotasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await nextTick();
}

/** Enough microtask rounds for a multi-page backfill to run to completion. */
async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i += 1) {
    await flushMicrotasks();
  }
}

const BACKLOG_LINE_CHARS = 512;
// Mirrors the store's estimatedChars (text.length + 48) and the drain budget.
const BACKLOG_LINES_PER_APPEND = Math.floor(
  AUTO_SAVE_FLUSH_MAX_CHARS / (BACKLOG_LINE_CHARS + 48),
);

/** Feed `lineCount` lines of BACKLOG_LINE_CHARS chars through the store. */
function feedBacklog(
  s: ReturnType<typeof useSerialDebugStore>,
  lineCount: number,
): void {
  const sliceLines = 200;
  for (let sent = 0; sent < lineCount; sent += sliceLines) {
    const n = Math.min(sliceLines, lineCount - sent);
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from(`${"x".repeat(BACKLOG_LINE_CHARS)}\n`.repeat(n))],
    });
  }
}

describe("useSerialAutoSave", () => {
  let app: ReturnType<typeof createApp> | null = null;
  let host: HTMLDivElement | null = null;

  beforeEach(() => {
    vi.useFakeTimers();
    invokeSpy.mockReset();
    sessionPageHandler = noArchive;
    sysLineArchivePosition = null;
    sysLineGate = null;
    setActivePinia(createPinia());
    __setSerialDebugTransportForTest(fakeTransport());
  });

  afterEach(() => {
    app?.unmount();
    host?.remove();
    app = null;
    host = null;
    __setSerialDebugTransportForTest(null);
    vi.useRealTimers();
  });

  function mountAutoSave(s: ReturnType<typeof useSerialDebugStore>): void {
    host = document.createElement("div");
    document.body.appendChild(host);
    app = createApp(
      defineComponent({
        setup() {
          useSerialAutoSave(s);
          return () => null;
        },
      }),
    );
    app.mount(host);
  }

  function writtenContents(): string[] {
    return invokeSpy.mock.calls.map(
      (call) => (call[1] as { content: string }).content,
    );
  }

  it("waits for an in-flight flush to finish before dropping the session path", async () => {
    const s = useSerialDebugStore();
    host = document.createElement("div");
    document.body.appendChild(host);
    app = createApp(
      defineComponent({
        setup() {
          useSerialAutoSave(s);
          return () => null;
        },
      }),
    );
    app.mount(host);

    s.port = "/dev/ttyUSB0";
    s.autoSave = true;
    s.autoSaveDir = "/logs";
    s.open = true;
    await nextTick();
    expect(s.sessionAutoSavePath).not.toBeNull();

    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("first\n")],
    });

    let releaseFirstWrite: (() => void) | null = null;
    invokeSpy
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            releaseFirstWrite = resolve;
          }),
      )
      .mockResolvedValueOnce(undefined);

    await vi.advanceTimersByTimeAsync(5000);
    expect(invokeSpy).toHaveBeenCalledTimes(1);

    s.appendChunk({
      direction: "rx",
      tsMs: 1001,
      bytes: [...Buffer.from("second\n")],
    });
    s.open = false;
    await nextTick();

    expect(s.sessionAutoSavePath).not.toBeNull();

    (releaseFirstWrite as (() => void) | null)?.();
    await flushMicrotasks();

    expect(invokeSpy).toHaveBeenCalledTimes(2);
    expect(s.sessionAutoSavePath).toBeNull();
  });

  it("drains a large close-time backlog in bounded append batches", async () => {
    const s = useSerialDebugStore();
    host = document.createElement("div");
    document.body.appendChild(host);
    app = createApp(
      defineComponent({
        setup() {
          useSerialAutoSave(s);
          return () => null;
        },
      }),
    );
    app.mount(host);

    s.port = "/dev/ttyUSB0";
    s.autoSave = true;
    s.autoSaveDir = "/logs";
    s.open = true;
    await nextTick();

    const lineText = "x".repeat(2048);
    const lineCount = Math.ceil(AUTO_SAVE_FLUSH_MAX_CHARS / 2048) + 8;
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from(`${`${lineText}\n`.repeat(lineCount)}`)],
    });

    invokeSpy.mockResolvedValue(undefined);

    s.open = false;
    await nextTick();
    // The final flush first awaits the (empty) archive backfill, so give it more
    // than a couple of microtask rounds.
    await settle();

    expect(invokeSpy.mock.calls.length).toBeGreaterThan(1);
    expect(s.sessionAutoSavePath).toBeNull();
    for (const call of invokeSpy.mock.calls) {
      expect((call[1] as { content: string }).content.length).toBeLessThan(
        AUTO_SAVE_FLUSH_MAX_CHARS * 2,
      );
    }
  });

  it("drains the whole backlog within one periodic tick", async () => {
    const s = useSerialDebugStore();
    host = document.createElement("div");
    document.body.appendChild(host);
    app = createApp(
      defineComponent({
        setup() {
          useSerialAutoSave(s);
          return () => null;
        },
      }),
    );
    app.mount(host);

    s.port = "/dev/ttyUSB0";
    s.autoSave = true;
    s.autoSaveDir = "/logs";
    s.open = true;
    await nextTick();

    invokeSpy.mockResolvedValue(undefined);
    feedBacklog(s, BACKLOG_LINES_PER_APPEND * 5 + 10);

    await vi.advanceTimersByTimeAsync(5000);
    await flushMicrotasks();

    // A single append per tick would leave most of this behind forever.
    expect(invokeSpy.mock.calls.length).toBeGreaterThanOrEqual(5);
    expect(s.drainPendingAutoSaveLines(Infinity)).toEqual([]);
  });

  it("stops after the per-tick append cap and resumes on the next tick", async () => {
    const s = useSerialDebugStore();
    host = document.createElement("div");
    document.body.appendChild(host);
    app = createApp(
      defineComponent({
        setup() {
          useSerialAutoSave(s);
          return () => null;
        },
      }),
    );
    app.mount(host);

    s.port = "/dev/ttyUSB0";
    s.autoSave = true;
    s.autoSaveDir = "/logs";
    s.open = true;
    await nextTick();

    invokeSpy.mockResolvedValue(undefined);
    feedBacklog(s, BACKLOG_LINES_PER_APPEND * 20);

    await vi.advanceTimersByTimeAsync(5000);
    await flushMicrotasks();

    // 20 batches of backlog, capped at 16 appends for this tick.
    const firstTickAppends = invokeSpy.mock.calls.length;
    expect(firstTickAppends).toBe(16);

    await vi.advanceTimersByTimeAsync(5000);
    await flushMicrotasks();

    expect(invokeSpy.mock.calls.length).toBeGreaterThan(firstTickAppends);
    expect(s.drainPendingAutoSaveLines(Infinity)).toEqual([]);
  });

  it("backfills the session archive when auto-save is enabled mid-session", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    s.port = "/dev/ttyUSB0";
    s.open = true;
    await nextTick();
    expect(s.sessionAutoSavePath).toBeNull();

    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("before-1\n")],
    });
    // The leak fix means these lines were never held in memory — the archive is
    // the only place they can be recovered from.
    expect(s.drainPendingAutoSaveLines(Infinity)).toEqual([]);

    sessionPageHandler = archivePages([
      { lineNo: 1, tsMs: 1000, direction: "rx", text: "before-1" },
      { lineNo: 2, tsMs: 1001, direction: "tx", text: "before-2" },
      { lineNo: 3, tsMs: 1002, direction: "sys", text: "before-3" },
    ]);
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    expect(s.sessionAutoSavePath).not.toBeNull();
    expect(invokeSpy).toHaveBeenCalledTimes(1);
    expect(invokeSpy).toHaveBeenCalledWith("append_text_file", {
      path: s.sessionAutoSavePath,
      content:
        `[${formatTs(1000)}] [RX ] before-1\n` +
        `[${formatTs(1001)}] [TX ] before-2\n` +
        `[${formatTs(1002)}] [SYS] before-3\n`,
    });
  });

  it("writes backfilled and live lines in the same format, oldest first", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    s.port = "/dev/ttyUSB0";
    // The format switch must apply to both halves of the file, not just live.
    s.autoSaveTimestamp = false;
    s.open = true;
    await nextTick();

    sessionPageHandler = archivePages([
      { lineNo: 1, tsMs: 1000, direction: "rx", text: "old" },
    ]);
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("new\n")],
    });
    await vi.advanceTimersByTimeAsync(5000);
    await settle();

    expect(writtenContents()).toEqual(["old\n", "new\n"]);
  });

  // The archive stores the cap notice as a sentinel so the wording can come from
  // the i18n catalogue; the saved file must contain the translation, never the
  // marker. Both halves of the file are checked — the pre-enable backfill and
  // the live queue.
  it("translates the archive cap sentinel on both the backfill and the live route", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    // Mirrors serial_debug_archive_cap_sentinel in
    // crates/tyutool-core/src/serial_debug.rs.
    const SOH = String.fromCharCode(1);
    const sentinel = `${SOH}tyutool:archive-capped:128${SOH}`;

    s.port = "/dev/ttyUSB0";
    s.open = true;
    await nextTick();

    sessionPageHandler = archivePages([
      { lineNo: 1, tsMs: 1000, direction: "sys", text: sentinel },
    ]);
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    await s.appendSysLine(sentinel);
    await vi.advanceTimersByTimeAsync(5000);
    await settle();

    const contents = writtenContents();
    expect(contents).toHaveLength(2);
    for (const content of contents) {
      expect(content).not.toContain(SOH);
      expect(content).not.toContain("archive-capped");
      expect(content).toContain("128 MiB");
    }
  });

  it("pages the backfill and stops at the snapshot taken when auto-save started", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    s.port = "/dev/ttyUSB0";
    s.open = true;
    await nextTick();

    const archived: SerialDebugLine[] = Array.from(
      { length: AUTO_SAVE_BACKFILL_PAGE_SIZE + 250 },
      (_, i) => ({
        lineNo: i + 1,
        tsMs: 1000,
        direction: "rx" as const,
        text: `a-${i}`,
      }),
    );
    // totalLines keeps growing while the backfill pages through the archive; the
    // snapshot from the first page is what caps the backfilled half.
    sessionPageHandler = archivePages(archived, 100);
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    const lineCounts = writtenContents().map(
      (content) => content.trimEnd().split("\n").length,
    );
    expect(lineCounts).toEqual([AUTO_SAVE_BACKFILL_PAGE_SIZE, 250]);
  });

  it("holds live lines back until the backfill has been written", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    s.port = "/dev/ttyUSB0";
    s.open = true;
    await nextTick();

    let releasePage: ((page: SerialDebugSessionPage) => void) | null = null;
    sessionPageHandler = () =>
      new Promise<SerialDebugSessionPage>((resolve) => {
        releasePage = resolve;
      });
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("live\n")],
    });
    await vi.advanceTimersByTimeAsync(5000);
    await settle();

    // The periodic flush fired but must not overtake the pending backfill.
    expect(invokeSpy).not.toHaveBeenCalled();

    (releasePage as ((page: SerialDebugSessionPage) => void) | null)?.({
      totalLines: 1,
      start: 0,
      items: [{ lineNo: 1, tsMs: 1000, direction: "rx", text: "old" }],
    });
    await settle();

    expect(writtenContents()).toEqual([
      `[${formatTs(1000)}] [RX ] old\n`,
      `[${formatTs(2000)}] [RX ] live\n`,
    ]);
  });

  /**
   * The handoff race, from the duplicate side: a line the archive already held
   * when the snapshot was taken, but whose chunk event only reached the frontend
   * after the session file existed. It is in the backfilled half *and* in the
   * live queue, and only its `archivedBefore` says so.
   */
  it("writes a line archived before the snapshot but delivered after it exactly once", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    s.port = "/dev/ttyUSB0";
    s.open = true;
    await nextTick();

    let releasePage: ((page: SerialDebugSessionPage) => void) | null = null;
    sessionPageHandler = () =>
      new Promise<SerialDebugSessionPage>((resolve) => {
        releasePage = resolve;
      });
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    // Archived as line 3 (so the archive held 2 lines before its chunk), which
    // is inside the snapshot below — the backfill will write it.
    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("straddler\n")],
      archivedBefore: 2,
    });

    (releasePage as ((page: SerialDebugSessionPage) => void) | null)?.({
      totalLines: 3,
      start: 0,
      items: [
        { lineNo: 1, tsMs: 1000, direction: "rx", text: "a" },
        { lineNo: 2, tsMs: 1001, direction: "rx", text: "b" },
        { lineNo: 3, tsMs: 2000, direction: "rx", text: "straddler" },
      ],
    });
    await settle();
    await vi.advanceTimersByTimeAsync(5000);
    await settle();

    const file = writtenContents().join("");
    expect(file.split("straddler")).toHaveLength(2); // i.e. exactly one hit
    expect(file).toBe(
      `[${formatTs(1000)}] [RX ] a\n` +
        `[${formatTs(1001)}] [RX ] b\n` +
        `[${formatTs(2000)}] [RX ] straddler\n`,
    );
  });

  /**
   * The same race from the gap side: a line archived *after* the snapshot is the
   * live half's responsibility, and the discard pass must leave it alone even
   * though it arrived while the backfill was still running.
   */
  it("keeps a line archived after the snapshot that arrived during the backfill", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    s.port = "/dev/ttyUSB0";
    s.open = true;
    await nextTick();

    let releasePage: ((page: SerialDebugSessionPage) => void) | null = null;
    sessionPageHandler = () =>
      new Promise<SerialDebugSessionPage>((resolve) => {
        releasePage = resolve;
      });
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    // The archive held 2 lines before this chunk, i.e. it became line 3 — one
    // past the snapshot of 2 that the backfill is about to report.
    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("after-snapshot\n")],
      archivedBefore: 2,
    });

    (releasePage as ((page: SerialDebugSessionPage) => void) | null)?.({
      totalLines: 2,
      start: 0,
      items: [
        { lineNo: 1, tsMs: 1000, direction: "rx", text: "a" },
        { lineNo: 2, tsMs: 1001, direction: "rx", text: "b" },
      ],
    });
    await settle();
    await vi.advanceTimersByTimeAsync(5000);
    await settle();

    expect(writtenContents().join("")).toBe(
      `[${formatTs(1000)}] [RX ] a\n` +
        `[${formatTs(1001)}] [RX ] b\n` +
        `[${formatTs(2000)}] [RX ] after-snapshot\n`,
    );
  });

  /**
   * A sys line reaches the live queue before it reaches the archive, so its
   * position is only known once the backend answers. The discard has to wait for
   * that answer — here it is still outstanding when the backfill finishes.
   */
  it("waits for an in-flight sys line's archive position before discarding", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    s.port = "/dev/ttyUSB0";
    s.open = true;
    await nextTick();

    let releasePage: ((page: SerialDebugSessionPage) => void) | null = null;
    sessionPageHandler = () =>
      new Promise<SerialDebugSessionPage>((resolve) => {
        releasePage = resolve;
      });
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    let releaseSysLine: (() => void) | null = null;
    sysLineGate = new Promise<void>((resolve) => {
      releaseSysLine = resolve;
    });
    sysLineArchivePosition = 2; // archived as line 2, inside the snapshot
    void s.appendSysLine("connected");
    await settle();

    (releasePage as ((page: SerialDebugSessionPage) => void) | null)?.({
      totalLines: 2,
      start: 0,
      items: [
        { lineNo: 1, tsMs: 1000, direction: "rx", text: "a" },
        { lineNo: 2, tsMs: 1001, direction: "sys", text: "connected" },
      ],
    });
    await settle();

    (releaseSysLine as (() => void) | null)?.();
    await settle();
    await vi.advanceTimersByTimeAsync(5000);
    await settle();

    const file = writtenContents().join("");
    expect(file.split("connected")).toHaveLength(2); // exactly one hit
    expect(file).toBe(
      `[${formatTs(1000)}] [RX ] a\n` + `[${formatTs(1001)}] [SYS] connected\n`,
    );
  });

  /**
   * A sys line the archive refused (it is capped) exists only in the live view,
   * so the backfill can never hold a copy and it must survive the discard.
   */
  it("keeps a sys line the capped archive refused", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    s.port = "/dev/ttyUSB0";
    s.open = true;
    await nextTick();

    let releasePage: ((page: SerialDebugSessionPage) => void) | null = null;
    sessionPageHandler = () =>
      new Promise<SerialDebugSessionPage>((resolve) => {
        releasePage = resolve;
      });
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    sysLineArchivePosition = null; // archive capped: nothing was written
    await s.appendSysLine("port closed");

    (releasePage as ((page: SerialDebugSessionPage) => void) | null)?.({
      totalLines: 1,
      start: 0,
      items: [{ lineNo: 1, tsMs: 1000, direction: "rx", text: "a" }],
    });
    await settle();
    await vi.advanceTimersByTimeAsync(5000);
    await settle();

    expect(writtenContents().join("")).toContain("[SYS] port closed\n");
  });

  /**
   * Once the archive is capped its line count freezes, so every later chunk
   * reports the same `archivedBefore` — which is exactly the snapshot. The
   * predicate is `<`, so those lines are kept and the file keeps growing where
   * the archive stopped.
   */
  it("keeps recording live lines after the archive has stopped at its cap", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    // Mirrors serial_debug_archive_cap_sentinel in
    // crates/tyutool-core/src/serial_debug.rs.
    const SOH = String.fromCharCode(1);
    const sentinel = `${SOH}tyutool:archive-capped:16${SOH}`;

    s.port = "/dev/ttyUSB0";
    s.open = true;
    await nextTick();

    // A capped archive: 2 lines, frozen there for the rest of the session.
    sessionPageHandler = archivePages([
      { lineNo: 1, tsMs: 1000, direction: "rx", text: "a" },
      { lineNo: 2, tsMs: 1001, direction: "sys", text: sentinel },
    ]);
    invokeSpy.mockResolvedValue(undefined);

    s.autoSave = true;
    s.autoSaveDir = "/logs";
    await nextTick();
    await settle();

    // Frozen counter: both chunks report the same position as the snapshot.
    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("after-cap-1\n")],
      archivedBefore: 2,
    });
    s.appendChunk({
      direction: "rx",
      tsMs: 2001,
      bytes: [...Buffer.from("after-cap-2\n")],
      archivedBefore: 2,
    });
    await vi.advanceTimersByTimeAsync(5000);
    await settle();

    const written = writtenContents().join("").trimEnd().split("\n");
    expect(written).toHaveLength(4);
    expect(written[0]).toBe(`[${formatTs(1000)}] [RX ] a`);
    // The cap notice is translated on the way out, never the raw sentinel.
    expect(written[1]).toContain("16 MiB");
    expect(written[1]).not.toContain(SOH);
    expect(written.slice(2)).toEqual([
      `[${formatTs(2000)}] [RX ] after-cap-1`,
      `[${formatTs(2001)}] [RX ] after-cap-2`,
    ]);
  });

  it("starts recording normally when there is no session archive", async () => {
    const s = useSerialDebugStore();
    mountAutoSave(s);

    // sessionPageHandler defaults to rejecting — port never opened, no archive.
    s.port = "/dev/ttyUSB0";
    s.autoSave = true;
    s.autoSaveDir = "/logs";
    s.open = true;
    await nextTick();
    await settle();

    expect(s.sessionAutoSavePath).not.toBeNull();
    expect(invokeSpy).not.toHaveBeenCalled();
    expect(
      s.lines.some((line) =>
        line.text.startsWith("serialDebug.autoSave.errWrite"),
      ),
    ).toBe(false);

    invokeSpy.mockResolvedValue(undefined);
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("live\n")],
    });
    await vi.advanceTimersByTimeAsync(5000);
    await settle();

    expect(writtenContents()).toEqual([`[${formatTs(1000)}] [RX ] live\n`]);
  });
});
