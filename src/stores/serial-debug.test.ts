// @vitest-environment happy-dom
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { wsTransport } from "@/transport/ws-transport";
import {
  __setSerialDebugTransportForTest,
  type SerialDebugTransport,
} from "@/features/serial-debug/transport";
import type {
  DebugChunk,
  SerialDebugFilterPage,
  SerialDebugFilterUpdatePayload,
  SerialDebugSessionPage,
} from "@/features/serial-debug/types";
import { useSerialDebugStore } from "./serial-debug";
import { SerialDebugLogLineRenderer } from "@/features/serial-debug/log-line-renderer";
import { useFlashStore } from "./flash";
import { nextTick, watchEffect } from "vue";
import {
  DEFAULT_ARCHIVE_LIMIT_MIB,
  DEFAULT_VISIBLE_LOG_WINDOW_LINES,
  FILTER_PAGE_SIZE,
  HISTORY_ENTRY_PAGES,
  HISTORY_PAGE_SIZE,
  MAX_PENDING_LINE_BYTES,
} from "@/features/serial-debug/constants";
import { AUTH_ONLY_CHIP_ID } from "@/features/firmware-flash/constants";

vi.mock("@/composables/confirmDialog", () => ({
  showConfirmDialog: vi.fn(async () => true),
}));
import { showConfirmDialog } from "@/composables/confirmDialog";

async function waitForChunkFrame(): Promise<void> {
  await new Promise((r) => setTimeout(r, 20));
  await nextTick();
}

function fakeTransport(): SerialDebugTransport & {
  emitChunk: (c: DebugChunk) => void;
  emitChunkBatch: (chunks: DebugChunk[]) => void;
  emitDisconnect: (reason: string) => void;
  emitFilterUpdated: (payload: SerialDebugFilterUpdatePayload) => void;
  emitArchiveCapped: (limitMib: number) => void;
  emitChunksDropped: (droppedBytes: number) => void;
  setFilterPage: (filterId: string, page: SerialDebugFilterPage) => void;
  /** Pretend the Rust session archive holds `total` lines (`arch-1`…`arch-N`). */
  setSessionArchive: (total: number) => void;
  /** Take over `readSessionPage` entirely (gap / failure scenarios). */
  setSessionPageResponder: (
    fn: ((start: number, limit: number) => SerialDebugSessionPage) | null,
  ) => void;
  readSessionPageCalls: Array<{ start: number; limit: number }>;
  readFilterMatchesCalls: Array<{
    filterId: string;
    start?: number;
    limit?: number;
  }>;
  sent: Uint8Array[];
  archiveLimitCalls: number[];
  sysLineWrites: string[];
  opened: boolean;
} {
  const chunkListeners = new Set<(c: DebugChunk) => void>();
  const chunkBatchListeners = new Set<(chunks: DebugChunk[]) => void>();
  const discListeners = new Set<(p: { reason: string }) => void>();
  const filterListeners = new Set<
    (p: SerialDebugFilterUpdatePayload) => void
  >();
  const archiveCappedListeners = new Set<(p: { limitMib: number }) => void>();
  const chunksDroppedListeners = new Set<
    (p: { droppedBytes: number }) => void
  >();
  const sent: Uint8Array[] = [];
  const archiveLimitCalls: number[] = [];
  const sysLineWrites: string[] = [];
  const readFilterMatchesCalls: Array<{
    filterId: string;
    start?: number;
    limit?: number;
  }> = [];
  const readSessionPageCalls: Array<{ start: number; limit: number }> = [];
  let sessionArchiveTotal = 0;
  let sessionPageResponder:
    ((start: number, limit: number) => SerialDebugSessionPage) | null = null;
  let opened = false;
  let nextFilterId = 1;
  const filters = new Map<
    string,
    {
      def: SerialDebugFilterUpdatePayload["def"];
      stats: SerialDebugFilterUpdatePayload["stats"];
      page: SerialDebugFilterPage;
    }
  >();
  return {
    sent,
    archiveLimitCalls,
    sysLineWrites,
    readFilterMatchesCalls,
    get opened() {
      return opened;
    },
    async open() {
      opened = true;
    },
    async close() {
      opened = false;
    },
    async send(b) {
      sent.push(b);
    },
    async clearSession() {},
    async appendSysLine(_tsMs, text) {
      sysLineWrites.push(text);
      // No archive position: the store must then treat the line as one the
      // archive never took, i.e. never discard it during an auto-save handoff.
      return null;
    },
    async addFilter(keyword, useRegex, color) {
      const id = `filter-${nextFilterId++}`;
      const payload: SerialDebugFilterUpdatePayload = {
        def: { id, keyword, useRegex, color },
        stats: {
          filterId: id,
          status: "complete",
          scannedUntilLineNo: 0,
          totalLinesSnapshot: 0,
          totalMatches: 0,
          error: null,
        },
      };
      filters.set(id, {
        def: payload.def,
        stats: payload.stats,
        page: { filterId: id, totalMatches: 0, start: 0, items: [] },
      });
      return payload;
    },
    async removeFilter(filterId) {
      filters.delete(filterId);
    },
    async readFilterMatches(filterId, start, limit) {
      readFilterMatchesCalls.push({ filterId, start, limit });
      return (
        filters.get(filterId)?.page ?? {
          filterId,
          totalMatches: 0,
          start: 0,
          items: [],
        }
      );
    },
    async readSessionPage(start: number, limit: number) {
      readSessionPageCalls.push({ start, limit });
      if (sessionPageResponder) return sessionPageResponder(start, limit);
      // Mirrors the Rust archive: dense 1-based `lineNo`, and an out-of-range
      // `start` is silently clamped to `totalLines` (the clamped value is what
      // comes back in `page.start`).
      const clamped = Math.min(Math.max(start, 0), sessionArchiveTotal);
      const end = Math.min(clamped + limit, sessionArchiveTotal);
      return {
        totalLines: sessionArchiveTotal,
        start: clamped,
        items: Array.from({ length: Math.max(0, end - clamped) }, (_, i) => ({
          lineNo: clamped + i + 1,
          tsMs: 1_700_000_000_000 + clamped + i,
          direction: "rx" as const,
          text: `arch-${clamped + i + 1}`,
        })),
      };
    },
    async setArchiveLimit(maxBytes: number) {
      archiveLimitCalls.push(maxBytes);
    },
    onChunk(cb) {
      chunkListeners.add(cb);
      return () => chunkListeners.delete(cb);
    },
    onChunkBatch(cb) {
      chunkBatchListeners.add(cb);
      return () => chunkBatchListeners.delete(cb);
    },
    onDisconnect(cb) {
      discListeners.add(cb);
      return () => discListeners.delete(cb);
    },
    onFilterUpdated(cb) {
      filterListeners.add(cb);
      return () => filterListeners.delete(cb);
    },
    onArchiveCapped(cb) {
      archiveCappedListeners.add(cb);
      return () => archiveCappedListeners.delete(cb);
    },
    onChunksDropped(cb) {
      chunksDroppedListeners.add(cb);
      return () => chunksDroppedListeners.delete(cb);
    },
    emitChunk(c) {
      chunkListeners.forEach((l) => l(c));
    },
    emitChunkBatch(chunks) {
      chunkBatchListeners.forEach((l) => l(chunks));
    },
    emitDisconnect(reason) {
      discListeners.forEach((l) => l({ reason }));
    },
    emitFilterUpdated(payload) {
      filterListeners.forEach((l) => l(payload));
    },
    emitArchiveCapped(limitMib) {
      archiveCappedListeners.forEach((l) => l({ limitMib }));
    },
    emitChunksDropped(droppedBytes) {
      chunksDroppedListeners.forEach((l) => l({ droppedBytes }));
    },
    setFilterPage(filterId, page) {
      const entry = filters.get(filterId);
      if (entry) entry.page = page;
    },
    readSessionPageCalls,
    setSessionArchive(total) {
      sessionArchiveTotal = total;
    },
    setSessionPageResponder(fn) {
      sessionPageResponder = fn;
    },
  };
}

describe("useSerialDebugStore.appendChunk", () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it("splits bytes by \\n into lines with direction and timestamp", async () => {
    const s = useSerialDebugStore();
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("hi\nworld\n")],
    });
    expect(s.lines.length).toBe(2);
    expect(s.lines[0]).toMatchObject({
      direction: "rx",
      tsMs: 1000,
      text: "hi",
    });
    expect(s.lines[1]).toMatchObject({ direction: "rx", text: "world" });
  });

  it("buffers trailing partial line until the next newline", () => {
    const s = useSerialDebugStore();
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("hel")],
    });
    expect(s.lines.length).toBe(0);
    s.appendChunk({
      direction: "rx",
      tsMs: 1001,
      bytes: [...Buffer.from("lo\n")],
    });
    expect(s.lines.length).toBe(1);
    expect(s.lines[0].text).toBe("hello");
  });

  it("forces very long pending data into a bounded line even without newline", () => {
    const s = useSerialDebugStore();
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: new Array(MAX_PENDING_LINE_BYTES + 5).fill("a".charCodeAt(0)),
    });
    expect(s.lines.length).toBe(1);
    expect(s.lines[0].text.length).toBe(MAX_PENDING_LINE_BYTES);

    s.appendChunk({
      direction: "rx",
      tsMs: 1001,
      bytes: [0x0a],
    });
    expect(s.lines.length).toBe(2);
    expect(s.lines[1].text).toBe("aaaaa");
  });

  it("drops oldest entries past the visible live window limit", () => {
    const s = useSerialDebugStore();
    // Simulate slightly more than the visible window arriving in one batch.
    const oneLine = "x\n";
    const bytes = [
      ...Buffer.from(oneLine.repeat(DEFAULT_VISIBLE_LOG_WINDOW_LINES + 505)),
    ];
    s.appendChunk({ direction: "rx", tsMs: 1000, bytes });
    expect(s.lines.length).toBe(DEFAULT_VISIBLE_LOG_WINDOW_LINES);
  });

  it("each line owns an independent rawBytes copy", () => {
    const s = useSerialDebugStore();
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("ab\ncd\n")],
    });
    expect(s.lines.length).toBe(2);
    expect(s.lines[0].rawBytes).not.toBe(s.lines[1].rawBytes);
  });

  // `lines` is a shallowRef, so every in-place write has to publish itself via
  // triggerRef. This guards all three writers: append, append-into-a-saturated
  // window (length does not change), and the trim when the limit is lowered.
  it("publishes every in-place lines write to reactive readers", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 2;
    await nextTick();

    const seen: string[] = [];
    const stop = watchEffect(() => {
      seen.push(s.lines.map((line) => line.text).join(","));
    });
    try {
      s.appendChunk({
        direction: "rx",
        tsMs: 1000,
        bytes: [...Buffer.from("a\n")],
      });
      await nextTick();
      s.appendChunk({
        direction: "rx",
        tsMs: 1001,
        bytes: [...Buffer.from("b\n")],
      });
      await nextTick();
      // Window is full: the head is dropped for every append, so the array
      // length stops changing while the content keeps moving.
      s.appendChunk({
        direction: "rx",
        tsMs: 1002,
        bytes: [...Buffer.from("c\n")],
      });
      await nextTick();
      s.logWindowLines = 1;
      await nextTick();

      expect(seen).toEqual(["", "a", "a,b", "b,c", "c"]);
    } finally {
      stop();
    }
  });

  it("reuses one TextDecoder while processing a chunk batch", () => {
    const OriginalTextDecoder = globalThis.TextDecoder;
    let decoderConstructCount = 0;
    class CountingTextDecoder extends OriginalTextDecoder {
      constructor(label?: string, options?: TextDecoderOptions) {
        super(label, options);
        decoderConstructCount += 1;
      }
    }
    vi.stubGlobal("TextDecoder", CountingTextDecoder);

    try {
      const s = useSerialDebugStore();
      s.appendChunk({
        direction: "rx",
        tsMs: 1000,
        bytes: [...Buffer.from("one\ntwo\nthree\n")],
      });
      expect(s.lines.map((line) => line.text)).toEqual(["one", "two", "three"]);
      expect(decoderConstructCount).toBe(1);
    } finally {
      vi.unstubAllGlobals();
      globalThis.TextDecoder = OriginalTextDecoder;
    }
  });

  it("drains auto-save lines in bounded lightweight batches", () => {
    const s = useSerialDebugStore();
    s.sessionAutoSavePath = "/logs/COM1/serial-debug.txt";
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("first\nsecond\n")],
    });

    const firstBatch = s.drainPendingAutoSaveLines(1);

    expect(firstBatch).toHaveLength(1);
    expect(firstBatch[0]).toMatchObject({
      direction: "rx",
      tsMs: 1000,
      text: "first",
    });
    expect(firstBatch[0]).not.toHaveProperty("rawBytes");

    const secondBatch = s.drainPendingAutoSaveLines(1);
    expect(secondBatch).toHaveLength(1);
    expect(secondBatch[0].text).toBe("second");
  });

  it("does not queue auto-save lines while no session file is active", () => {
    const s = useSerialDebugStore();

    for (let i = 0; i < 200; i += 1) {
      s.appendChunk({
        direction: "rx",
        tsMs: 1000 + i,
        bytes: [...Buffer.from(`line-${i}\n`)],
      });
    }

    // Nothing drains the backlog while auto-save is off, so it must stay empty
    // instead of retaining every line for the whole session.
    expect(s.drainPendingAutoSaveLines(Infinity)).toEqual([]);

    // Enabling auto-save starts capturing from that point on.
    s.sessionAutoSavePath = "/logs/COM1/serial-debug.txt";
    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("after-enable\n")],
    });
    expect(
      s.drainPendingAutoSaveLines(Infinity).map((line) => line.text),
    ).toEqual(["after-enable"]);
  });

  it("discards exactly the queued lines the backfill already covered", () => {
    const s = useSerialDebugStore();
    s.sessionAutoSavePath = "/logs/COM1/serial-debug.txt";

    // Archived as line 3 → inside a backfill that stopped at line 3.
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("backfilled\n")],
      archivedBefore: 2,
    });
    // Archived as line 4 → after the backfill, the live half's job.
    s.appendChunk({
      direction: "rx",
      tsMs: 1001,
      bytes: [...Buffer.from("live\n")],
      archivedBefore: 3,
    });
    // No position at all → never in the archive, so never discarded.
    s.appendChunk({
      direction: "rx",
      tsMs: 1002,
      bytes: [...Buffer.from("unarchived\n")],
    });

    s.dropBackfilledAutoSaveLines(3);

    expect(
      s.drainPendingAutoSaveLines(Infinity).map((line) => line.text),
    ).toEqual(["live", "unarchived"]);

    // The event for a line archived before the snapshot can still be delivered
    // after the discard pass; the same predicate has to catch it at enqueue.
    s.appendChunk({
      direction: "rx",
      tsMs: 1003,
      bytes: [...Buffer.from("late\n")],
      archivedBefore: 1,
    });
    expect(s.drainPendingAutoSaveLines(Infinity)).toEqual([]);
  });

  it("stops discarding once a session clear has renumbered the archive", async () => {
    const s = useSerialDebugStore();
    s.sessionAutoSavePath = "/logs/COM1/serial-debug.txt";
    s.dropBackfilledAutoSaveLines(5);
    await s.clear();

    // The new session numbers from 1 again, so the old watermark would swallow
    // every line of it.
    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("fresh\n")],
      archivedBefore: 0,
    });
    expect(
      s.drainPendingAutoSaveLines(Infinity).map((line) => line.text),
    ).toEqual(["fresh"]);
  });
});

describe("useSerialDebugStore.send", () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it("encodes ASCII and appends \\r\\n when sendAppendCrlf is true", async () => {
    const s = useSerialDebugStore();
    (s as unknown as { open: boolean }).open = true;
    s.sendMode = "ascii";
    s.sendAppendCrlf = true;
    s.sendInput = "AT";
    await s.send();
    expect(fake.sent.length).toBe(1);
    expect(Array.from(fake.sent[0])).toEqual([0x41, 0x54, 0x0d, 0x0a]);
    expect(s.sendInput).toBe("");
  });

  it("parses Hex and ignores non-hex characters", async () => {
    const s = useSerialDebugStore();
    (s as unknown as { open: boolean }).open = true;
    s.sendMode = "hex";
    s.sendAppendCrlf = false;
    s.sendInput = "AA,BB;CC";
    await s.send();
    expect(Array.from(fake.sent[0])).toEqual([0xaa, 0xbb, 0xcc]);
  });

  it("blocks duplicate submissions until a successful send settles", async () => {
    let finish!: () => void;
    const write = vi.spyOn(fake, "send").mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const s = useSerialDebugStore();
    s.open = true;
    s.sendInput = "AT";
    const pending = s.send();
    expect(s.sending).toBe(true);
    expect(s.sendInput).toBe("AT");
    await s.send();
    expect(write).toHaveBeenCalledTimes(1);
    finish();
    await pending;
    expect(s.sending).toBe(false);
    expect(s.sendInput).toBe("");
    expect(s.sendHistory.map((entry) => entry.text)).toEqual(["AT"]);
  });

  it.each(["NEXT", "AT"])(
    "preserves an edited draft even if it is changed back to %s while sending",
    async (draft) => {
      let finish!: () => void;
      vi.spyOn(fake, "send").mockImplementation(
        () =>
          new Promise<void>((resolve) => {
            finish = resolve;
          }),
      );
      const s = useSerialDebugStore();
      s.open = true;
      s.sendInput = "AT";
      const pending = s.send();
      s.sendInput = "edited";
      s.sendInput = draft;
      finish();
      await pending;
      expect(s.sendInput).toBe(draft);
      expect(s.sendHistory.map((entry) => entry.text)).toEqual(["AT"]);
    },
  );

  it("keeps odd Hex warning/drop behavior and guards the warning await", async () => {
    let finishWarning!: () => void;
    vi.spyOn(fake, "appendSysLine").mockImplementation(
      () =>
        new Promise<number | null>((resolve) => {
          finishWarning = () => resolve(null);
        }),
    );
    const s = useSerialDebugStore();
    s.open = true;
    s.sendMode = "hex";
    s.sendInput = "AA,bb;C";
    const pending = s.send();
    expect(s.sending).toBe(true);
    await s.send();
    expect(s.lines).toHaveLength(1);
    expect(s.lines[0].direction).toBe("sys");
    expect(fake.sent).toHaveLength(0);
    finishWarning();
    await pending;
    expect(fake.sent.map((bytes) => Array.from(bytes))).toEqual([[0xaa, 0xbb]]);
    expect(s.sendInput).toBe("");
    expect(s.sendError).toBeNull();
    expect(s.sending).toBe(false);
  });

  it("does not mark an edited Hex draft invalid after its old warning settles", async () => {
    let finishWarning!: () => void;
    vi.spyOn(fake, "appendSysLine").mockImplementation(
      () =>
        new Promise<number | null>((resolve) => {
          finishWarning = () => resolve(null);
        }),
    );
    const s = useSerialDebugStore();
    s.open = true;
    s.sendMode = "hex";
    s.sendInput = "F";
    const pending = s.send();
    s.sendInput = "FF";
    finishWarning();
    await pending;
    expect(fake.sent).toHaveLength(0);
    expect(s.sendInput).toBe("FF");
    expect(s.sendError).toBeNull();
    expect(s.sending).toBe(false);
  });

  it("releases the pending guard after rejection and preserves the current draft", async () => {
    let fail!: (error: string) => void;
    const write = vi.spyOn(fake, "send").mockImplementationOnce(
      () =>
        new Promise<void>((_resolve, reject) => {
          fail = reject;
        }),
    );
    const s = useSerialDebugStore();
    s.open = true;
    s.sendInput = "AT";
    const pending = s.send();
    s.sendInput = "NEXT";
    await s.send();
    expect(write).toHaveBeenCalledTimes(1);
    fail("disconnected");
    await pending;
    expect(s.sendInput).toBe("NEXT");
    expect(s.sendError).toBeNull();
    expect(s.lines[s.lines.length - 1]?.text).toContain("disconnected");
    expect(s.sendHistory).toEqual([]);
    expect(s.sending).toBe(false);
    await s.send();
    expect(s.sendHistory.map((entry) => entry.text)).toEqual(["NEXT"]);
    expect(s.sendInput).toBe("");
    expect(s.sendError).toBeNull();
  });

  it("keeps send history, trimmed to MAX_SEND_HISTORY and deduped", async () => {
    const s = useSerialDebugStore();
    (s as unknown as { open: boolean }).open = true;
    s.sendMode = "ascii";
    s.sendAppendCrlf = false;
    s.sendInput = "A";
    await s.send();
    s.sendInput = "B";
    await s.send();
    s.sendInput = "A";
    await s.send(); // duplicate should move to front, not add a new entry
    expect(s.sendHistory.map((entry) => entry.text)).toEqual(["A", "B"]);
  });

  it("records the settings used for a pending send rather than the newer draft settings", async () => {
    let finish!: () => void;
    vi.spyOn(fake, "send").mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const s = useSerialDebugStore();
    s.open = true;
    s.sendInput = "AT";
    s.sendMode = "ascii";
    s.sendAppendCrlf = true;
    const pending = s.send();
    s.sendMode = "hex";
    s.sendAppendCrlf = false;
    finish();
    await pending;
    expect(s.sendHistory[0]).toMatchObject({
      text: "AT",
      mode: "ascii",
      appendCrlf: true,
    });
  });
});

describe("useSerialDebugStore port-manager integration", () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it("openPort acquires the port before calling transport.open", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    expect(fake.opened).toBe(true);
    expect(s.open).toBe(true);
  });

  it("openPort stays compatible with transports that do not implement onChunkBatch", async () => {
    const legacyTransport = fakeTransport();
    const { onChunkBatch, ...legacy } = legacyTransport;
    expect(typeof onChunkBatch).toBe("function");
    __setSerialDebugTransportForTest(legacy as unknown as SerialDebugTransport);

    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;

    await expect(s.openPort()).resolves.toBeUndefined();
    expect(s.open).toBe(true);
  });

  it("batches transport chunks until the next frame", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    const before = s.lines.length;

    fake.emitChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("hel")],
    });
    fake.emitChunk({
      direction: "rx",
      tsMs: 1001,
      bytes: [...Buffer.from("lo\nworld\n")],
    });

    expect(s.lines.length).toBe(before);

    await waitForChunkFrame();

    expect(s.lines.slice(before).map((line) => line.text)).toEqual([
      "hello",
      "world",
    ]);
  });

  it("shows an unfinished chunk as a live preview without duplicating completed bytes", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    fake.emitChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("hello")],
    });
    await waitForChunkFrame();
    expect(s.activeDisplayLines().map((line) => line.text)).toContain("hello");
    fake.emitChunk({
      direction: "rx",
      tsMs: 1001,
      bytes: [...Buffer.from(" world\n")],
    });
    await waitForChunkFrame();
    expect(s.activeDisplayLines().map((line) => line.text)).toContain(
      "hello world",
    );
    expect(
      s.activeDisplayLines().filter((line) => line.text.startsWith("hello"))
        .length,
    ).toBe(1);
  });

  it("corrects split UTF-8 previews and preserves exact bytes for Hex", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    const bytes = [...new TextEncoder().encode("中")];
    fake.emitChunk({ direction: "rx", tsMs: 1234, bytes: bytes.slice(0, 2) });
    await waitForChunkFrame();
    const first = s.activeDisplayLines()[s.activeDisplayLines().length - 1];
    expect(first.text).toBe("�");
    fake.emitChunk({ direction: "rx", tsMs: 1235, bytes: bytes.slice(2) });
    await waitForChunkFrame();
    const corrected = s.activeDisplayLines()[s.activeDisplayLines().length - 1];
    expect(corrected.text).toBe("中");
    expect(corrected.rawBytes).toEqual(Uint8Array.from(bytes));
    expect(corrected.id).not.toBe(first.id);
    expect(corrected.tsMs).toBe(1235);
  });

  it("keeps TX and RX previews independent and refreshes direct appendChunk previews only on input", () => {
    const s = useSerialDebugStore();
    s.appendChunk({ direction: "rx", tsMs: 11, bytes: [0x41] });
    const rx = s.activeDisplayLines()[0];
    s.appendChunk({ direction: "tx", tsMs: 22, bytes: [0x42] });
    expect(s.activeDisplayLines().map((line) => line.text)).toEqual(["A", "B"]);
    expect(s.activeDisplayLines()[0].id).not.toBe(rx.id);
    const ids = s.activeDisplayLines().map((line) => line.id);
    s.appendChunk({ direction: "rx", tsMs: 11, bytes: [] });
    expect(s.activeDisplayLines().map((line) => line.id)).toEqual(ids);
  });

  it("ignores empty queued chunks without changing preview identity or timestamp", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    fake.emitChunk({ direction: "rx", tsMs: 7, bytes: [0x41] });
    await waitForChunkFrame();
    const initial = s.activeDisplayLines()[s.activeDisplayLines().length - 1];
    expect(initial.tsMs).toBe(7);
    fake.emitChunk({ direction: "rx", tsMs: 99, bytes: [] });
    await waitForChunkFrame();
    const after = s.activeDisplayLines()[s.activeDisplayLines().length - 1];
    expect(after.id).toBe(initial.id);
    expect(after.tsMs).toBe(7);
  });

  it("refreshes the renderer cache when preview content changes", () => {
    const s = useSerialDebugStore();
    s.appendChunk({ direction: "rx", tsMs: 10, bytes: [0x41] });
    const renderer = new SerialDebugLogLineRenderer();
    const beforeId = s.activeDisplayLines()[0].id;
    s.appendChunk({ direction: "rx", tsMs: 11, bytes: [0x42] });
    const after = renderer.render(s.activeDisplayLines(), true, "")[0];
    expect(s.activeDisplayLines()[0].id).not.toBe(beforeId);
    expect(after.line.text).toBe("AB");
  });

  it("does not archive or auto-save preview bytes and completes the line once", () => {
    const s = useSerialDebugStore();
    s.sessionAutoSavePath = "/logs/COM1/a.txt";
    s.appendChunk({
      direction: "rx",
      tsMs: 10,
      bytes: [...Buffer.from("part")],
    });
    expect(s.lines).toEqual([]);
    expect(s.drainPendingAutoSaveLines()).toEqual([]);
    s.appendChunk({
      direction: "rx",
      tsMs: 11,
      bytes: [...Buffer.from("ial\n")],
    });
    expect(s.lines.filter((line) => line.text === "partial")).toHaveLength(1);
    expect(s.drainPendingAutoSaveLines()).toHaveLength(1);
  });

  it("bounds completed lines plus previews by the live window budget", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 500;
    for (let i = 0; i < 510; i++)
      s.appendChunk({
        direction: "rx",
        tsMs: i,
        bytes: [...Buffer.from(`L${i}\n`)],
      });
    s.appendChunk({ direction: "rx", tsMs: 999, bytes: [0x58] });
    expect(s.activeDisplayLines().length).toBeLessThanOrEqual(s.logWindowLines);
  });

  it("clears a queued preview at drop and close boundaries", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    fake.emitChunk({
      direction: "rx",
      tsMs: 1,
      bytes: [...Buffer.from("tail")],
    });
    await waitForChunkFrame();
    fake.emitChunksDropped(4);
    fake.emitChunk({
      direction: "rx",
      tsMs: 2,
      bytes: [...Buffer.from("new")],
    });
    await waitForChunkFrame();
    expect(s.lines.filter((line) => line.text === "tail")).toHaveLength(1);
    const preview = s.activeDisplayLines()[s.activeDisplayLines().length - 1];
    expect(preview.text).toBe("new");
    expect(preview.rawBytes).toEqual(Uint8Array.from(Buffer.from("new")));
    expect(s.activeDisplayLines().some((line) => line.text === "tailnew")).toBe(
      false,
    );
    await s.closePort();
    expect(
      s.activeDisplayLines().filter((line) => line.text === "new"),
    ).toHaveLength(1);
    expect(
      s.activeDisplayLines().filter((line) => line.text === "tail"),
    ).toHaveLength(1);
  });

  it("queues transport chunk batches with one frame flush", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    const before = s.lines.length;

    fake.emitChunkBatch([
      {
        direction: "rx",
        tsMs: 1000,
        bytes: [...Buffer.from("hel")],
      },
      {
        direction: "rx",
        tsMs: 1001,
        bytes: [...Buffer.from("lo\nworld\n")],
      },
    ]);

    expect(s.lines.length).toBe(before);

    await waitForChunkFrame();

    expect(s.lines.slice(before).map((line) => line.text)).toEqual([
      "hello",
      "world",
    ]);
  });

  // The live view is fed by the raw chunk stream and never reads the archive,
  // so the notice the archive wrote into itself can only reach the user through
  // this event. Without it the log keeps scrolling while archiving has silently
  // stopped.
  describe("archive cap notice", () => {
    // Mirrors serial_debug_archive_cap_sentinel in
    // crates/tyutool-core/src/serial_debug.rs.
    const SOH = String.fromCharCode(1);

    it("puts a translated sys line into the live view", async () => {
      const s = useSerialDebugStore();
      s.port = "/dev/ttyUSB0";
      s.baudRate = 115200;
      await s.openPort();
      const before = s.lines.length;

      fake.emitArchiveCapped(256);

      const added = s.lines.slice(before);
      expect(added).toHaveLength(1);
      expect(added[0].direction).toBe("sys");
      expect(added[0].text).toContain("256 MiB");
      // The sentinel is an internal marker — it must never reach the user.
      expect(added[0].text).not.toContain(SOH);
      expect(added[0].text).not.toContain("archive-capped");
    });

    it("does not write the notice back into the archive", async () => {
      const s = useSerialDebugStore();
      s.port = "/dev/ttyUSB0";
      s.baudRate = 115200;
      await s.openPort();
      fake.sysLineWrites.length = 0;

      fake.emitArchiveCapped(256);

      // The archive already holds the sentinel; a write-back would record the
      // same event twice (and be dropped, the archive being capped).
      expect(fake.sysLineWrites).toEqual([]);
    });

    it("keeps the gap warning after raising the limit until the archive is cleared", async () => {
      const s = useSerialDebugStore();
      s.port = "/dev/ttyUSB0";
      await s.openPort();
      expect(s.archiveLimitReached).toBe(false);

      fake.emitArchiveCapped(256);
      expect(s.archiveLimitReached).toBe(true);
      s.archiveLimitMib = 512;
      await nextTick();
      expect(fake.archiveLimitCalls).toContain(512 * 1024 * 1024);
      expect(s.archiveLimitReached).toBe(true);

      await s.clear();
      expect(s.archiveLimitReached).toBe(false);
    });

    it("stops listening once the session is torn down", async () => {
      const s = useSerialDebugStore();
      s.port = "/dev/ttyUSB0";
      s.baudRate = 115200;
      await s.openPort();
      await s.closePort();
      const before = s.lines.length;

      fake.emitArchiveCapped(256);

      expect(s.lines.length).toBe(before);
    });
  });

  // Dropping a chunk is bad; splicing the bytes either side of the gap into one
  // line is worse — it fabricates a log line the device never printed, and the
  // user has no way to tell. The gap has to become a line boundary.
  describe("dropped chunk notice", () => {
    const SOH = String.fromCharCode(1);

    async function openedStore() {
      const s = useSerialDebugStore();
      s.port = "/dev/ttyUSB0";
      s.baudRate = 115200;
      await s.openPort();
      return s;
    }

    it("closes the open line so the halves are never joined", async () => {
      const s = await openedStore();
      fake.emitChunk({
        direction: "rx",
        tsMs: 1000,
        bytes: [...Buffer.from("before-gap")],
      });
      await waitForChunkFrame();
      const before = s.lines.length;

      fake.emitChunksDropped(4096);
      fake.emitChunk({
        direction: "rx",
        tsMs: 1001,
        bytes: [...Buffer.from("after-gap\n")],
      });
      await waitForChunkFrame();

      const added = s.lines.slice(before);
      expect(added.map((line) => line.direction)).toEqual(["rx", "sys", "rx"]);
      expect(added[0].text).toBe("before-gap");
      expect(added[2].text).toBe("after-gap");
      expect(
        added.every((line) => !line.text.includes("before-gapafter-gap")),
      ).toBe(true);
    });

    it("emits queued pre-gap chunks before the notice, not after", async () => {
      const s = await openedStore();
      const before = s.lines.length;

      // Still sitting in the 16 ms coalescing queue when the drop arrives.
      fake.emitChunk({
        direction: "rx",
        tsMs: 1000,
        bytes: [...Buffer.from("queued\n")],
      });
      fake.emitChunksDropped(512);
      await waitForChunkFrame();

      const added = s.lines.slice(before);
      expect(added).toHaveLength(2);
      // Order is the point: the queued chunk arrived before the gap, so the
      // notice must land after it, not on top of it.
      expect(added[0].direction).toBe("rx");
      expect(added[0].text).toBe("queued");
      expect(added[1].direction).toBe("sys");
      expect(added[1].text).toContain("512");
    });

    it("shows a translated notice and never the raw sentinel", async () => {
      const s = await openedStore();
      const before = s.lines.length;

      fake.emitChunksDropped(12288);

      const added = s.lines.slice(before);
      expect(added).toHaveLength(1);
      expect(added[0].direction).toBe("sys");
      expect(added[0].text).toContain("12288");
      expect(added[0].text).not.toContain(SOH);
      expect(added[0].text).not.toContain("chunks-dropped");
    });

    it("does not write the notice back into the archive", async () => {
      const s = await openedStore();
      fake.sysLineWrites.length = 0;

      fake.emitChunksDropped(4096);

      // Rust already wrote the sentinel into the archive at the gap; a write-back
      // would record the same loss twice, in the wrong place.
      expect(fake.sysLineWrites).toEqual([]);
      expect(s.lines.length).toBeGreaterThan(0);
    });

    it("stops listening once the session is torn down", async () => {
      const s = await openedStore();
      await s.closePort();
      const before = s.lines.length;

      fake.emitChunksDropped(4096);

      expect(s.lines.length).toBe(before);
    });
  });

  // Closing the port is the last moment the live view can catch up with what the
  // device actually sent — nothing arrives after it, and nothing else ever cuts
  // an unterminated line.
  describe("closing the port", () => {
    async function openedStore() {
      const s = useSerialDebugStore();
      s.port = "/dev/ttyUSB0";
      s.baudRate = 115200;
      await s.openPort();
      return s;
    }

    it("flushes the chunks still queued in the last coalescing frame", async () => {
      const s = await openedStore();
      const before = s.lines.length;

      // Deliberately no waitForChunkFrame(): the close lands *inside* the 16 ms
      // coalescing window. Rust archived these bytes the moment they arrived, so
      // discarding the queue here would make the live view disagree with the
      // export and the auto-save file.
      fake.emitChunk({
        direction: "rx",
        tsMs: 1000,
        bytes: [...Buffer.from("last-gasp\n")],
      });
      await s.closePort();

      const added = s.lines.slice(before).filter((l) => l.direction === "rx");
      expect(added.map((l) => l.text)).toEqual(["last-gasp"]);
    });

    it("retains and auto-saves the final batch emitted while the backend closes", async () => {
      const s = await openedStore();
      s.sessionAutoSavePath = "/logs/closing.txt";
      fake.close = async () => {
        // The backend drains its bridge before acknowledging close. These
        // bytes therefore arrive during close(), not before the close starts.
        fake.emitChunkBatch([
          {
            direction: "rx",
            tsMs: 1000,
            bytes: [...Buffer.from("last-complete\nlast-tail")],
          },
        ]);
      };

      await s.closePort();

      expect(
        s.lines
          .filter((line) => line.direction === "rx")
          .map((line) => line.text),
      ).toEqual(["last-complete", "last-tail"]);
      expect(
        s.drainPendingAutoSaveLines(Infinity).map((line) => line.text),
      ).toEqual(["last-complete", "last-tail"]);
    });

    it("shows the tail the device never terminated with a newline", async () => {
      const s = await openedStore();
      fake.emitChunk({
        direction: "rx",
        tsMs: 1000,
        bytes: [...Buffer.from("boot ok\nlogin: ")],
      });
      await waitForChunkFrame();
      const before = s.lines.length;
      expect(s.lines[before - 1].text).toBe("boot ok");

      await s.closePort();

      // A prompt, a progress bar, a bootloader banner: no trailing newline, so
      // only the close path can turn it into a line at all.
      const added = s.lines.slice(before).filter((l) => l.direction === "rx");
      expect(added.map((l) => l.text)).toEqual(["login: "]);
    });

    it("adds no empty line when nothing was left unterminated", async () => {
      const s = await openedStore();
      fake.emitChunk({
        direction: "rx",
        tsMs: 1000,
        bytes: [...Buffer.from("complete\n")],
      });
      await waitForChunkFrame();
      const before = s.lines.length;

      await s.closePort();

      expect(s.lines.slice(before).filter((l) => l.direction === "rx")).toEqual(
        [],
      );
    });
  });

  it("reuses one TextDecoder across queued chunks flushed in the same frame", async () => {
    const OriginalTextDecoder = globalThis.TextDecoder;
    let decoderConstructCount = 0;
    class CountingTextDecoder extends OriginalTextDecoder {
      constructor(label?: string, options?: TextDecoderOptions) {
        super(label, options);
        decoderConstructCount += 1;
      }
    }
    vi.stubGlobal("TextDecoder", CountingTextDecoder);

    try {
      const s = useSerialDebugStore();
      s.port = "/dev/ttyUSB0";
      s.baudRate = 115200;
      await s.openPort();

      fake.emitChunk({
        direction: "rx",
        tsMs: 1000,
        bytes: [...Buffer.from("hel")],
      });
      fake.emitChunk({
        direction: "rx",
        tsMs: 1001,
        bytes: [...Buffer.from("lo\nworld\n")],
      });

      await waitForChunkFrame();

      expect(s.lines.slice(-2).map((line) => line.text)).toEqual([
        "hello",
        "world",
      ]);
      expect(decoderConstructCount).toBe(1);
    } finally {
      vi.unstubAllGlobals();
      globalThis.TextDecoder = OriginalTextDecoder;
    }
  });

  it("pushes queued frame chunks into the reactive lines array once", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();

    const pushSpy = vi.spyOn(s.lines, "push");

    fake.emitChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("one\n")],
    });
    fake.emitChunk({
      direction: "rx",
      tsMs: 1001,
      bytes: [...Buffer.from("two\n")],
    });

    await waitForChunkFrame();

    expect(s.lines.slice(-2).map((line) => line.text)).toEqual(["one", "two"]);
    expect(pushSpy).toHaveBeenCalledTimes(1);
  });

  it("when port-manager denies, openPort does not call transport.open", async () => {
    const { usePortManagerStore } = await import("@/stores/port-manager");
    const pm = usePortManagerStore();
    // Pre-occupy by a different owner that refuses to release.
    await pm.acquire({
      id: "flash",
      port: "/dev/ttyUSB0",
      onReleaseRequest: async () => false,
      onReleased: () => {},
    });
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    await s.openPort();
    expect(fake.opened).toBe(false);
    expect(s.open).toBe(false);
  });

  it("clear() empties lines and pending buffer", async () => {
    const s = useSerialDebugStore();
    s.sessionAutoSavePath = "/logs/COM1/serial-debug.txt";
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("ab\ncd")],
    });
    expect(s.drainPendingAutoSaveLines()).toHaveLength(1);
    expect(s.lines.length).toBe(1);
    await s.clear();
    expect(s.lines.length).toBe(0);
    expect(s.drainPendingAutoSaveLines()).toEqual([]);
    // new input after clear should start a fresh line
    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("xy\n")],
    });
    expect(s.lines[0].text).toBe("xy");
  });

  it("clear() drops queued transport chunks before they flush", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();

    fake.emitChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("stale\n")],
    });
    s.clear();

    await waitForChunkFrame();

    expect(s.lines).toEqual([]);
  });

  it("auto-resumes when the released port becomes free again", async () => {
    const { usePortManagerStore } = await import("@/stores/port-manager");
    const pm = usePortManagerStore();
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.autoRelease = true;
    await s.openPort();
    expect(s.open).toBe(true);

    // Simulate flash preempting the port — port-manager will call serial-debug's
    // onReleaseRequest (returns true because autoRelease) then onReleased('requested').
    await pm.acquire({
      id: "flash",
      port: "/dev/ttyUSB0",
      onReleaseRequest: async () => false,
      onReleased: () => {},
    });
    // After acquire, the flash now owns; allow microtasks for the release side-effects.
    await new Promise((r) => setTimeout(r, 0));
    expect(s.open).toBe(false);
    expect(s.pendingResume).toBe(true);

    // Flash finishes: releases the port. Our watcher should trigger a re-open.
    pm.release("/dev/ttyUSB0", "flash");
    // Wait a couple ticks for the watcher + async openPort chain.
    await new Promise((r) => setTimeout(r, 50));
    expect(s.open).toBe(true);
    expect(s.pendingResume).toBe(false);
  });

  it("when autoRelease is off and flash requests the port, the conflict prompt includes the auto-release hint", async () => {
    const { usePortManagerStore } = await import("@/stores/port-manager");
    const pm = usePortManagerStore();
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    s.autoRelease = false;
    await s.openPort();
    expect(s.open).toBe(true);

    vi.mocked(showConfirmDialog).mockClear();
    vi.mocked(showConfirmDialog).mockResolvedValueOnce(false);

    // Simulate flash requesting the port — triggers serial-debug's onReleaseRequest.
    await pm.acquire({
      id: "flash",
      port: "/dev/ttyUSB0",
      onReleaseRequest: async () => false,
      onReleased: () => {},
    });

    expect(showConfirmDialog).toHaveBeenCalledTimes(1);
    const opts = vi.mocked(showConfirmDialog).mock.calls[0][0];
    // The hint is appended for flash (body + "\n\n" separator + hint text).
    expect(opts.message).toContain("\n\n");
    // The body (requester) is still present.
    expect(opts.message).toContain("flash");
  });

  it("does not append the auto-release hint for a non-flash requester", async () => {
    const { usePortManagerStore } = await import("@/stores/port-manager");
    const pm = usePortManagerStore();
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    s.autoRelease = false;
    await s.openPort();
    expect(s.open).toBe(true);

    vi.mocked(showConfirmDialog).mockClear();
    vi.mocked(showConfirmDialog).mockResolvedValueOnce(false);

    await pm.acquire({
      id: "some-other-feature",
      port: "/dev/ttyUSB0",
      onReleaseRequest: async () => false,
      onReleased: () => {},
    });

    expect(showConfirmDialog).toHaveBeenCalledTimes(1);
    const opts = vi.mocked(showConfirmDialog).mock.calls[0][0];
    expect(opts.message).not.toContain("\n\n");
  });
});

describe("useSerialDebugStore watch chip management", () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it("addChip returns ok, adds chip, and sets it as active tab", async () => {
    const s = useSerialDebugStore();
    const result = await s.addChip("ERROR", false);
    expect(result).toBe("ok");
    expect(s.watchChips.length).toBe(1);
    expect(s.watchChips[0]).toMatchObject({
      keyword: "ERROR",
      useRegex: false,
    });
    expect(s.activeChipId).toBe(s.watchChips[0].id);
  });

  it("retains filter results and the reading pin on a failed tail refresh, then retries", async () => {
    const s = useSerialDebugStore();
    await s.addChip("ERROR", false);
    const id = s.activeChipId!;
    const before = s.filterPagesById[id];
    s.activeFilterPinned = true;
    const reader = vi
      .spyOn(fake, "readFilterMatches")
      .mockRejectedValueOnce(new Error("filter unavailable"));
    await s.loadActiveFilterTail();
    expect(s.filterReadError).toBe("filter unavailable");
    expect(s.filterPagesById[id]).toBe(before);
    expect(s.activeFilterPinned).toBe(true);
    expect(s.activeFilterLoading).toBe(false);
    await s.loadActiveFilterTail();
    expect(s.filterReadError).toBeNull();
    expect(s.activeFilterPinned).toBe(false);
    reader.mockRestore();
  });

  it("ignores a filter read failure after switching its source tab", async () => {
    const s = useSerialDebugStore();
    await s.addChip("ERROR", false);
    let fail!: (error: Error) => void;
    const reader = vi.spyOn(fake, "readFilterMatches").mockImplementationOnce(
      () =>
        new Promise((_resolve, reject) => {
          fail = reject;
        }),
    );
    const pending = s.loadActiveFilterTail();
    await s.setActiveChip(null);
    fail(new Error("old tab failed"));
    await pending;
    expect(s.filterReadError).toBeNull();
    expect(s.activeFilterLoading).toBe(false);
    reader.mockRestore();
  });

  it("addChip returns duplicate when same keyword added twice", async () => {
    const s = useSerialDebugStore();
    expect(await s.addChip("WIFI", false)).toBe("ok");
    expect(await s.addChip("WIFI", false)).toBe("duplicate");
    expect(s.watchChips.length).toBe(1);
  });

  it("addChip returns invalid-regex for bad pattern", async () => {
    const s = useSerialDebugStore();
    expect(await s.addChip("[invalid", true)).toBe("invalid-regex");
    expect(s.watchChips.length).toBe(0);
  });

  it("removeChip removes by id and falls back activeChipId", async () => {
    const s = useSerialDebugStore();
    await s.addChip("FIRST", false);
    await s.addChip("SECOND", false);
    const firstId = s.watchChips[0].id;
    const secondId = s.watchChips[1].id;
    await s.setActiveChip(secondId);
    await s.removeChip(secondId);
    expect(s.watchChips.length).toBe(1);
    expect(s.watchChips[0].keyword).toBe("FIRST");
    expect(s.activeChipId).toBe(firstId);
  });

  it("setActiveChip switches active tab (null = All)", async () => {
    const s = useSerialDebugStore();
    await s.addChip("LOG", false);
    const id = s.watchChips[0].id;
    expect(s.activeChipId).toBe(id);
    await s.setActiveChip(null);
    expect(s.activeChipId).toBeNull();
    await s.setActiveChip(id);
    expect(s.activeChipId).toBe(id);
  });

  it("clear does not affect watchChips", async () => {
    const s = useSerialDebugStore();
    await s.addChip("LOG", false);
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("LOG line\n")],
    });
    await s.clear();
    expect(s.watchChips.length).toBe(1);
    expect(s.lines.length).toBe(0);
  });

  it("clear resets filter stats and loaded filter pages for a new session", async () => {
    const s = useSerialDebugStore();
    await s.addChip("LOG", false);
    const id = s.watchChips[0].id;
    s.filterStatsById = {
      [id]: {
        filterId: id,
        status: "complete",
        scannedUntilLineNo: 42,
        totalLinesSnapshot: 42,
        totalMatches: 7,
        error: "old",
      },
    };
    s.filterPagesById = {
      [id]: {
        filterId: id,
        totalMatches: 7,
        start: 3,
        items: [
          {
            id: 1,
            direction: "rx",
            tsMs: 1000,
            text: "LOG line",
          },
        ],
      },
    };
    s.activeFilterLoading = true;
    s.activeFilterFullyLoaded = false;

    await s.clear();

    expect(s.filterStatsById[id]).toEqual({
      filterId: id,
      status: "complete",
      scannedUntilLineNo: 0,
      totalLinesSnapshot: 0,
      totalMatches: 0,
      error: null,
    });
    expect(s.filterPagesById).toEqual({});
    expect(s.activeFilterLoading).toBe(false);
    expect(s.activeFilterFullyLoaded).toBe(true);
  });

  it("invalidates filter reads started while clear is waiting for the backend", async () => {
    const s = useSerialDebugStore();
    await s.addChip("LOG", false);
    const id = s.watchChips[0].id;
    s.filterStatsById = {
      [id]: {
        filterId: id,
        status: "complete",
        scannedUntilLineNo: 20,
        totalLinesSnapshot: 20,
        totalMatches: 2,
        error: null,
      },
    };
    let finishClear!: () => void;
    const clearGate = new Promise<void>((resolve) => {
      finishClear = resolve;
    });
    fake.clearSession = () => clearGate;
    let finishPage!: (page: SerialDebugFilterPage) => void;
    const pageGate = new Promise<SerialDebugFilterPage>((resolve) => {
      finishPage = resolve;
    });
    const readFilter = vi.fn(async () => pageGate);
    fake.readFilterMatches = readFilter;

    const clearing = s.clear();
    await Promise.resolve();
    const loading = s.loadActiveFilterTail();
    expect(readFilter).toHaveBeenCalledTimes(1);
    finishClear();
    await clearing;
    finishPage({
      filterId: id,
      totalMatches: 2,
      start: 0,
      items: [{ lineNo: 1, tsMs: 1, direction: "rx", text: "old session" }],
    });
    await loading;

    expect(s.filterPagesById).toEqual({});
    expect(s.activeFilterLoading).toBe(false);
  });

  it("keeps new auto-save lines when archive numbering restarts during clear", async () => {
    const s = useSerialDebugStore();
    s.sessionAutoSavePath = "/tmp/serial-debug-autosave.txt";
    s.dropBackfilledAutoSaveLines(10);
    let finishClear!: () => void;
    const clearGate = new Promise<void>((resolve) => {
      finishClear = resolve;
    });
    fake.clearSession = () => clearGate;

    const clearing = s.clear();
    await Promise.resolve();
    s.appendChunk({
      direction: "rx",
      tsMs: 2,
      bytes: [...Buffer.from("new session\n")],
      archivedBefore: 0,
    });
    finishClear();
    await clearing;

    expect(s.drainPendingAutoSaveLines(Number.POSITIVE_INFINITY)).toEqual([
      expect.objectContaining({ text: "new session" }),
    ]);
  });

  it("preserves displayed logs when the backend refuses to clear", async () => {
    const s = useSerialDebugStore();
    s.lines = [{ id: 1, direction: "rx", tsMs: 1, text: "keep me" }];
    fake.clearSession = async () => {
      throw new Error("clear refused");
    };

    await s.clear();

    expect(s.lines.some((line) => line.text === "keep me")).toBe(true);
    expect(s.lines.some((line) => line.direction === "sys")).toBe(true);
    expect(
      s.activeDisplayLines().filter((line) => line.text === "keep me"),
    ).toHaveLength(1);
  });

  it("gives every archive line in a filter page its own display id", async () => {
    const s = useSerialDebugStore();
    await s.addChip("ERR", false);
    const id = s.watchChips[0].id;
    fake.setFilterPage(id, {
      filterId: id,
      totalMatches: 3,
      start: 0,
      items: [
        { lineNo: 3, tsMs: 1000, direction: "rx", text: "ERR alpha" },
        { lineNo: 9, tsMs: 1001, direction: "rx", text: "ERR beta" },
        { lineNo: 17, tsMs: 1002, direction: "rx", text: "ERR gamma" },
      ],
    });

    await s.setActiveChip(id);

    const items = s.activeDisplayLines();
    expect(items.map((line) => line.text)).toEqual([
      "ERR alpha",
      "ERR beta",
      "ERR gamma",
    ]);
    // Distinct ids per line: the log renderers cache parsed lines by id, so
    // colliding ids make one line render in place of the others (issue: a
    // single filtered line shown repeatedly).
    expect(new Set(items.map((line) => line.id)).size).toBe(3);
    expect(items.every((line) => Number.isInteger(line.id))).toBe(true);
  });

  it("bounds accepted auto-save lines and reports overflow only once", () => {
    const s = useSerialDebugStore();
    const overflow = vi.fn();
    s.sessionAutoSavePath = "/logs/COM1/session.txt";
    s.autoSave = true;
    s.registerAutoSaveOverflowHandler(overflow);
    for (let i = 0; i < 32769; i += 1) {
      s.appendChunk({ direction: "rx", tsMs: i, bytes: [0x78, 0x0a] });
    }
    expect(s.pendingAutoSaveStats()).toEqual({
      queueCount: 32768,
      estimatedChars: 32768 * 49,
    });
    expect(overflow).toHaveBeenCalledTimes(1);
    expect(s.autoSave).toBe(false);
    expect(s.lines.length).toBeLessThanOrEqual(5000);
    expect(s.lines[s.lines.length - 1].text).toBe("x");
  });

  it("accounts character limits and releases capacity across queue removal paths", () => {
    const s = useSerialDebugStore();
    const overflow = vi.fn();
    s.registerAutoSaveOverflowHandler(overflow);
    s.sessionAutoSavePath = "/logs/COM1/first.txt";
    const line = "x".repeat(4000);
    for (let i = 0; i < 2100; i += 1) {
      s.appendChunk({
        direction: "rx",
        tsMs: i,
        bytes: [...Buffer.from(`${line}\n`)],
        archivedBefore: 0,
      });
    }
    expect(s.pendingAutoSaveStats().estimatedChars).toBeLessThanOrEqual(
      8 * 1024 * 1024,
    );
    expect(overflow).toHaveBeenCalledTimes(1);

    s.commitPendingAutoSaveLines(100);
    expect(s.pendingAutoSaveStats()).toEqual({
      queueCount: 1972,
      estimatedChars: 1972 * (4000 + 48),
    });
    expect(s.drainPendingAutoSaveLines(100)).toHaveLength(1);
    expect(s.pendingAutoSaveStats()).toEqual({
      queueCount: 1971,
      estimatedChars: 1971 * (4000 + 48),
    });
    s.dropBackfilledAutoSaveLines(1);
    expect(s.pendingAutoSaveStats()).toEqual({
      queueCount: 0,
      estimatedChars: 0,
    });

    s.sessionAutoSavePath = null;
    s.sessionAutoSavePath = "/logs/COM1/second.txt";
    s.autoSave = true;
    s.appendChunk({ direction: "rx", tsMs: 3000, bytes: [0x79, 0x0a] });
    expect(s.pendingAutoSaveStats()).toEqual({
      queueCount: 1,
      estimatedChars: 49,
    });
  });

  // A filter tab renders straight out of the archive, so it is the one live-view
  // path that can meet the raw cap sentinel.
  it("translates the archive cap sentinel in a filter page", async () => {
    const s = useSerialDebugStore();
    await s.addChip("tyutool", false);
    const id = s.watchChips[0].id;
    const SOH = String.fromCharCode(1);
    fake.setFilterPage(id, {
      filterId: id,
      totalMatches: 1,
      start: 0,
      items: [
        {
          lineNo: 4,
          tsMs: 1000,
          direction: "sys",
          text: `${SOH}tyutool:archive-capped:512${SOH}`,
        },
      ],
    });

    await s.setActiveChip(id);

    const [line] = s.activeDisplayLines();
    expect(line.text).toContain("512 MiB");
    expect(line.text).not.toContain(SOH);
    expect(line.text).not.toContain("archive-capped");
  });

  it("translates the dropped-chunk sentinel in a filter page", async () => {
    const s = useSerialDebugStore();
    await s.addChip("tyutool", false);
    const id = s.watchChips[0].id;
    const SOH = String.fromCharCode(1);
    fake.setFilterPage(id, {
      filterId: id,
      totalMatches: 1,
      start: 0,
      items: [
        {
          lineNo: 4,
          tsMs: 1000,
          direction: "sys",
          text: `${SOH}tyutool:chunks-dropped:8192${SOH}`,
        },
      ],
    });

    await s.setActiveChip(id);

    const [line] = s.activeDisplayLines();
    expect(line.text).toContain("8192");
    expect(line.text).not.toContain(SOH);
    expect(line.text).not.toContain("chunks-dropped");
  });

  it("keeps display ids stable when the same filter page is reloaded", async () => {
    const s = useSerialDebugStore();
    await s.addChip("ERR", false);
    const id = s.watchChips[0].id;
    fake.setFilterPage(id, {
      filterId: id,
      totalMatches: 2,
      start: 0,
      items: [
        { lineNo: 3, tsMs: 1000, direction: "rx", text: "ERR alpha" },
        { lineNo: 9, tsMs: 1001, direction: "rx", text: "ERR beta" },
      ],
    });

    await s.setActiveChip(id);
    const firstIds = s.activeDisplayLines().map((line) => line.id);
    await s.setActiveChip(id);
    expect(s.activeDisplayLines().map((line) => line.id)).toEqual(firstIds);
  });

  it("does not reuse display ids for archive line numbers of a new session", async () => {
    const s = useSerialDebugStore();
    await s.addChip("ERR", false);
    const id = s.watchChips[0].id;
    fake.setFilterPage(id, {
      filterId: id,
      totalMatches: 1,
      start: 0,
      items: [{ lineNo: 1, tsMs: 1000, direction: "rx", text: "ERR old" }],
    });
    await s.setActiveChip(id);
    const oldId = s.activeDisplayLines()[0].id;

    await s.clear();
    fake.setFilterPage(id, {
      filterId: id,
      totalMatches: 1,
      start: 0,
      items: [{ lineNo: 1, tsMs: 2000, direction: "rx", text: "ERR new" }],
    });
    await s.setActiveChip(id);

    expect(s.activeDisplayLines()[0].text).toBe("ERR new");
    expect(s.activeDisplayLines()[0].id).not.toBe(oldId);
  });

  it("throttles active filter tail reloads for repeated live updates", async () => {
    vi.useFakeTimers();
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.addChip("LOG", false);
    const filterId = s.watchChips[0].id;
    fake.readFilterMatchesCalls.length = 0;

    await s.openPort();
    fake.readFilterMatchesCalls.length = 0;

    fake.emitFilterUpdated({
      def: s.watchChips[0],
      stats: {
        filterId,
        status: "complete",
        scannedUntilLineNo: 10,
        totalLinesSnapshot: 10,
        totalMatches: 1,
        error: null,
      },
    });
    fake.emitFilterUpdated({
      def: s.watchChips[0],
      stats: {
        filterId,
        status: "complete",
        scannedUntilLineNo: 11,
        totalLinesSnapshot: 11,
        totalMatches: 2,
        error: null,
      },
    });

    expect(fake.readFilterMatchesCalls).toEqual([]);
    await vi.advanceTimersByTimeAsync(120);

    expect(fake.readFilterMatchesCalls).toHaveLength(1);
    expect(fake.readFilterMatchesCalls[0]?.filterId).toBe(filterId);
  });

  // The live refresh re-anchors the active filter window on the newest matches.
  // Someone who paged backwards is reading older ones, so it has to hold off
  // until they come back — otherwise the content is swapped out under them.
  it("does not re-anchor a paged-back filter window on a live update", async () => {
    vi.useFakeTimers();
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.addChip("LOG", false);
    const filterId = s.watchChips[0].id;
    fake.setFilterPage(filterId, {
      filterId,
      totalMatches: 900,
      start: 400,
      items: [{ lineNo: 401, tsMs: 1000, direction: "rx", text: "LOG mid" }],
    });
    await s.openPort();
    await s.setActiveChip(filterId);
    expect(s.activeFilterPinned).toBe(false);

    await s.loadOlderActiveFilterMatches();
    expect(s.activeFilterPinned).toBe(true);
    fake.readFilterMatchesCalls.length = 0;

    fake.emitFilterUpdated({
      def: s.watchChips[0],
      stats: {
        filterId,
        status: "complete",
        scannedUntilLineNo: 20,
        totalLinesSnapshot: 20,
        totalMatches: 901,
        error: null,
      },
    });
    await vi.advanceTimersByTimeAsync(120);

    expect(fake.readFilterMatchesCalls).toEqual([]);
    // The chip's own count still tracks the session — only the window is held.
    expect(s.filterStatsById[filterId].totalMatches).toBe(901);

    // Back at the tail: the window is re-anchored and the pin comes off, so the
    // next live update is free to refresh again.
    await s.loadActiveFilterTail();

    expect(s.activeFilterPinned).toBe(false);
    expect(fake.readFilterMatchesCalls).toHaveLength(1);
    vi.useRealTimers();
  });

  it("chips cycle colors from CHIP_COLORS when added", async () => {
    const { CHIP_COLORS } = await import("@/features/serial-debug/constants");
    const s = useSerialDebugStore();
    for (let i = 0; i < CHIP_COLORS.length + 1; i++) {
      await s.addChip(`kw${i}`, false);
    }
    expect(s.watchChips[0].color).toBe(CHIP_COLORS[0]);
    expect(s.watchChips[CHIP_COLORS.length].color).toBe(CHIP_COLORS[0]);
  });

  it("ignores a filter page from the session that was cleared while loading", async () => {
    const s = useSerialDebugStore();
    await s.addChip("old", false);
    const id = s.activeChipId!;
    let resolvePage!: (page: SerialDebugFilterPage) => void;
    fake.readFilterMatches = () =>
      new Promise((resolve) => {
        resolvePage = resolve;
      });

    const loading = s.loadActiveFilterTail();
    await s.clear();
    resolvePage({
      filterId: id,
      start: 0,
      totalMatches: 1,
      items: [{ lineNo: 1, tsMs: 1000, direction: "rx", text: "old-session" }],
    });
    await loading;

    expect(s.activeDisplayLines()).toEqual([]);
    expect(s.filterPagesById[id]).toBeUndefined();
  });

  it("keeps the active filter load when an inactive chip is removed", async () => {
    const s = useSerialDebugStore();
    await s.addChip("active", false);
    const activeId = s.activeChipId!;
    await s.addChip("inactive", false);
    const inactiveId = s.activeChipId!;
    await s.setActiveChip(activeId);

    let resolvePage!: (page: SerialDebugFilterPage) => void;
    fake.readFilterMatches = () =>
      new Promise((resolve) => {
        resolvePage = resolve;
      });
    const loading = s.loadActiveFilterTail();
    await s.removeChip(inactiveId);
    resolvePage({
      filterId: activeId,
      start: 0,
      totalMatches: 1,
      items: [{ lineNo: 1, tsMs: 1000, direction: "rx", text: "active" }],
    });
    await loading;

    expect(s.filterPagesById[activeId]?.items[0].text).toBe("active");
    expect(s.activeFilterLoading).toBe(false);
  });

  it("leaves filter loading settled when removing the active chip fails", async () => {
    const s = useSerialDebugStore();
    await s.addChip("active", false);
    const id = s.activeChipId!;
    fake.removeFilter = async () => {
      throw new Error("remove failed");
    };

    await expect(s.removeChip(id)).rejects.toThrow("remove failed");
    expect(s.activeChipId).toBeNull();
    expect(s.activeFilterLoading).toBe(false);
  });
});

describe("useSerialDebugStore.deviceReset", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    // Every deviceReset path ends in appendSysLine, which writes through the
    // transport. Without the fake it reaches the real ws transport and opens a
    // real socket to the dev-serve port — harmless while happy-dom had no
    // WebSocket (the constructor threw and the write was swallowed), a 5s hang
    // once happy-dom 18 shipped one.
    __setSerialDebugTransportForTest(fakeTransport());
  });

  afterEach(() => {
    __setSerialDebugTransportForTest(null);
    vi.restoreAllMocks();
  });

  it("prefers in-session reset when the control port matches the current debug port", async () => {
    const inSessionSpy = vi
      .spyOn(wsTransport, "serialDebugDeviceReset")
      .mockResolvedValue(undefined);
    const externalSpy = vi
      .spyOn(wsTransport, "deviceReset")
      .mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";
    s.open = true;

    await s.deviceReset("t5ai", "/dev/ttyACM1");

    expect(inSessionSpy).toHaveBeenCalledWith("T5AI");
    expect(externalSpy).not.toHaveBeenCalled();
  });

  it("keeps using the standalone reset path when the control port differs from the current debug port", async () => {
    const spy = vi
      .spyOn(wsTransport, "deviceReset")
      .mockResolvedValue(undefined);
    const inSessionSpy = vi
      .spyOn(wsTransport, "serialDebugDeviceReset")
      .mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";

    await s.deviceReset("t5ai", "/dev/ttyACM0");

    expect(spy).toHaveBeenCalledWith("/dev/ttyACM0", "T5AI");
    expect(inSessionSpy).not.toHaveBeenCalled();
  });

  it("logs a clear error when the control port matches the log port but the session is not open", async () => {
    const inSessionSpy = vi
      .spyOn(wsTransport, "serialDebugDeviceReset")
      .mockResolvedValue(undefined);
    const externalSpy = vi
      .spyOn(wsTransport, "deviceReset")
      .mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";
    s.open = false;
    const before = s.lines.length;

    await s.deviceReset("t5ai", "/dev/ttyACM1");

    expect(inSessionSpy).not.toHaveBeenCalled();
    expect(externalSpy).not.toHaveBeenCalled();
    expect(s.lines.length).toBe(before + 1);
    expect(s.lines[s.lines.length - 1].direction).toBe("sys");
  });

  it("falls back to port.value and uses the in-session path when the current debug session is open", async () => {
    const spy = vi
      .spyOn(wsTransport, "serialDebugDeviceReset")
      .mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";
    s.open = true;

    await s.deviceReset("T5AI");

    expect(spy).toHaveBeenCalledWith("T5AI");
  });

  it("treats an empty resetPort like the current log port and still uses the in-session path", async () => {
    const spy = vi
      .spyOn(wsTransport, "serialDebugDeviceReset")
      .mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";
    s.open = true;

    await s.deviceReset("T5AI", "");

    expect(spy).toHaveBeenCalledWith("T5AI");
  });

  it("does nothing when no effective port is available", async () => {
    const spy = vi
      .spyOn(wsTransport, "deviceReset")
      .mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "";
    await s.deviceReset("T5AI", "");
    expect(spy).not.toHaveBeenCalled();
  });

  it("logs a success line after a reset", async () => {
    vi.spyOn(wsTransport, "deviceReset").mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";
    const before = s.lines.length;
    await s.deviceReset("T5AI");
    expect(s.lines.length).toBe(before + 1);
    expect(s.lines[s.lines.length - 1].direction).toBe("sys");
  });

  it("logs an outdated-serve hint for an unknown-variant device_reset error", async () => {
    vi.spyOn(wsTransport, "deviceReset").mockRejectedValue(
      new Error("unknown variant `device_reset`, expected one of ..."),
    );
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";
    const before = s.lines.length;
    await s.deviceReset("T5AI");
    expect(s.lines.length).toBe(before + 1);
    expect(s.lines[s.lines.length - 1].direction).toBe("sys");
  });

  it("logs a generic failure line for other reset errors", async () => {
    vi.spyOn(wsTransport, "deviceReset").mockRejectedValue(
      new Error("port busy"),
    );
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";
    const before = s.lines.length;
    await s.deviceReset("T5AI");
    expect(s.lines.length).toBe(before + 1);
  });
});

describe("useSerialDebugStore open/close/send lifecycle", () => {
  // A transport whose open/send/close behavior is configurable so we can hit
  // the error/disconnect branches the happy-path fake never reaches.
  function configurableTransport() {
    const chunkListeners = new Set<(c: DebugChunk) => void>();
    const chunkBatchListeners = new Set<(chunks: DebugChunk[]) => void>();
    const discListeners = new Set<(p: { reason: string }) => void>();
    const filterListeners = new Set<
      (p: SerialDebugFilterUpdatePayload) => void
    >();
    const state = {
      openError: null as Error | null,
      sendError: null as Error | null,
      closeError: null as Error | null,
      opened: false,
      closeCalls: 0,
      sent: [] as Uint8Array[],
    };
    const transport: SerialDebugTransport = {
      async open() {
        if (state.openError) throw state.openError;
        state.opened = true;
      },
      async close() {
        state.closeCalls += 1;
        if (state.closeError) throw state.closeError;
        state.opened = false;
      },
      async send(b) {
        if (state.sendError) throw state.sendError;
        state.sent.push(b);
      },
      async clearSession() {},
      async appendSysLine() {
        return null;
      },
      async addFilter(keyword, useRegex, color) {
        return {
          def: { id: "filter-1", keyword, useRegex, color },
          stats: {
            filterId: "filter-1",
            status: "complete",
            scannedUntilLineNo: 0,
            totalLinesSnapshot: 0,
            totalMatches: 0,
            error: null,
          },
        };
      },
      async removeFilter() {},
      async readFilterMatches(filterId) {
        return { filterId, totalMatches: 0, start: 0, items: [] };
      },
      async readSessionPage() {
        return { totalLines: 0, start: 0, items: [] };
      },
      async setArchiveLimit() {},
      onChunk(cb) {
        chunkListeners.add(cb);
        return () => chunkListeners.delete(cb);
      },
      onChunkBatch(cb) {
        chunkBatchListeners.add(cb);
        return () => chunkBatchListeners.delete(cb);
      },
      onDisconnect(cb) {
        discListeners.add(cb);
        return () => discListeners.delete(cb);
      },
      onFilterUpdated(cb) {
        filterListeners.add(cb);
        return () => filterListeners.delete(cb);
      },
      onArchiveCapped() {
        return () => {};
      },
    };
    return {
      transport,
      state,
      emitDisconnect: (reason: string) =>
        discListeners.forEach((l) => l({ reason })),
    };
  }

  let cfg: ReturnType<typeof configurableTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    cfg = configurableTransport();
    __setSerialDebugTransportForTest(cfg.transport);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it("openPort with a blank port logs an invalid-config error and does not open", async () => {
    const s = useSerialDebugStore();
    s.port = "   ";
    const before = s.lines.length;
    await s.openPort();
    expect(s.open).toBe(false);
    expect(s.lines.length).toBe(before + 1);
    expect(cfg.state.opened).toBe(false);
  });

  it("openPort with a non-positive baud rate is rejected as invalid config", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.customBaudRate = 0; // currentBaud() === 0 → invalid
    await s.openPort();
    expect(s.open).toBe(false);
    expect(cfg.state.opened).toBe(false);
  });

  it("openPort is a no-op while already opening", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    (s as unknown as { opening: boolean }).opening = true;
    await s.openPort();
    expect(cfg.state.opened).toBe(false);
  });

  it("openPort surfaces a transport.open() failure and releases the port", async () => {
    const { usePortManagerStore } = await import("@/stores/port-manager");
    const pm = usePortManagerStore();
    cfg.state.openError = new Error("device unavailable");
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    expect(s.open).toBe(false);
    // The port was acquired then released after the open failure.
    expect(pm.currentOwner("/dev/ttyUSB0")).toBeNull();
    const last = s.lines[s.lines.length - 1];
    expect(last.direction).toBe("sys");
  });

  it("closePort closes the backend session and releases the port", async () => {
    const { usePortManagerStore } = await import("@/stores/port-manager");
    const pm = usePortManagerStore();
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    expect(s.open).toBe(true);
    expect(pm.currentOwner("/dev/ttyUSB0")).toBe("serial-debug");

    await s.closePort();
    expect(s.open).toBe(false);
    expect(cfg.state.closeCalls).toBeGreaterThanOrEqual(1);
    expect(pm.currentOwner("/dev/ttyUSB0")).toBeNull();
  });

  it("closePort is a no-op when nothing is open", async () => {
    const s = useSerialDebugStore();
    await s.closePort();
    expect(cfg.state.closeCalls).toBe(0);
  });

  it("a transport.close() error during teardown is swallowed (does not throw)", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    cfg.state.closeError = new Error("close kaboom");
    await expect(s.closePort()).resolves.toBeUndefined();
    expect(s.open).toBe(false);
  });

  it("transport disconnect callback appends a sys line and notifies port-manager", async () => {
    const s = useSerialDebugStore();
    s.port = "/dev/ttyUSB0";
    s.baudRate = 115200;
    await s.openPort();
    const before = s.lines.length;
    cfg.emitDisconnect("unplugged");
    // transport.onDisconnect logs a sys line and notifies the port-manager,
    // whose onReleased('unplugged') callback logs a second sys line.
    expect(s.lines.length).toBeGreaterThan(before);
    expect(s.lines[s.lines.length - 1].direction).toBe("sys");
  });

  it("send is a no-op when the port is not open", async () => {
    const s = useSerialDebugStore();
    s.sendInput = "AT";
    await s.send();
    expect(cfg.state.sent.length).toBe(0);
    expect(s.sendInput).toBe("AT");
    expect(s.sending).toBe(false);
  });

  it("send is a no-op when the input is empty", async () => {
    const s = useSerialDebugStore();
    (s as unknown as { open: boolean }).open = true;
    s.sendInput = "";
    await s.send();
    expect(cfg.state.sent.length).toBe(0);
  });

  it("send in hex mode with no valid bytes does not transmit", async () => {
    const s = useSerialDebugStore();
    (s as unknown as { open: boolean }).open = true;
    s.sendMode = "hex";
    s.sendInput = "zz"; // all ignored → zero bytes
    await s.send();
    expect(cfg.state.sent.length).toBe(0);
    expect(s.sendInput).toBe("zz");
    expect(s.sendError).toEqual({ kind: "invalidHex" });
    expect(s.sending).toBe(false);
    s.sendInput = "AA";
    expect(s.sendError).toBeNull();
  });

  it("send logs a sys error line when transport.send rejects", async () => {
    const s = useSerialDebugStore();
    (s as unknown as { open: boolean }).open = true;
    s.sendMode = "ascii";
    s.sendAppendCrlf = false;
    s.sendInput = "AT";
    cfg.state.sendError = new Error("write failed");
    const before = s.lines.length;
    await s.send();
    expect(s.lines.length).toBe(before + 1);
    expect(s.lines[s.lines.length - 1].direction).toBe("sys");
    // history is only updated on a successful send
    expect(s.sendHistory).toEqual([]);
    expect(s.sendInput).toBe("AT");
    expect(s.sendError).toEqual({ kind: "failed", message: "write failed" });
    expect(s.sending).toBe(false);
    cfg.state.sendError = null;
    await s.send();
    expect(cfg.state.sent).toHaveLength(1);
    expect(s.sendInput).toBe("");
    expect(s.sendError).toBeNull();
  });
});

describe("useSerialDebugStore reboot target resolution", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("prefers the remembered reboot control port and chip over flash page defaults", () => {
    const s = useSerialDebugStore();
    const flash = useFlashStore();
    flash.selectedSerialPort = "/dev/ttyACM0";
    flash.selectedChipId = "esp32";

    s.rememberRebootTarget("/dev/ttyUSB0", "t5ai");

    expect(s.resolveRebootTarget(["/dev/ttyUSB0", "/dev/ttyACM0"])).toEqual({
      controlPort: "/dev/ttyUSB0",
      chipId: "t5ai",
      needsSelection: false,
    });
  });

  it("falls back to the firmware tool selections when no remembered reboot target exists", () => {
    const s = useSerialDebugStore();
    const flash = useFlashStore();
    flash.selectedSerialPort = "/dev/ttyACM0";
    flash.selectedChipId = "esp32c3";

    expect(s.resolveRebootTarget(["/dev/ttyACM0"])).toEqual({
      controlPort: "/dev/ttyACM0",
      chipId: "esp32c3",
      needsSelection: false,
    });
  });

  it("requires explicit selection when the flash page is on the auth-only pseudo chip", () => {
    const s = useSerialDebugStore();
    const flash = useFlashStore();
    flash.selectedSerialPort = "/dev/ttyACM0";
    flash.selectedChipId = AUTH_ONLY_CHIP_ID;

    expect(s.resolveRebootTarget(["/dev/ttyACM0"])).toEqual({
      controlPort: "/dev/ttyACM0",
      chipId: null,
      needsSelection: true,
    });
  });

  it("marks the selection invalid when the remembered control port is missing from the current port list", () => {
    const s = useSerialDebugStore();
    s.rememberRebootTarget("/dev/ttyUSB0", "t5ai");

    expect(s.resolveRebootTarget(["/dev/ttyACM0"])).toEqual({
      controlPort: null,
      chipId: null,
      needsSelection: true,
    });
  });

  it("immediately overwrites the remembered reboot target when the user reselects", () => {
    const s = useSerialDebugStore();
    s.rememberRebootTarget("/dev/ttyUSB0", "t5ai");

    s.rememberRebootTarget("/dev/ttyACM0", "esp32");

    expect(s.rebootControlPort).toBe("/dev/ttyACM0");
    expect(s.rebootChipId).toBe("esp32");
  });
});

describe("useSerialDebugStore baud-rate follows flash chip", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("follows the flash chip default log baud when no user override is set", async () => {
    const { chipManifest } =
      await import("@/features/firmware-flash/chip-manifests");
    const flash = useFlashStore();
    const s = useSerialDebugStore();
    expect(s.baudRate).toBe(
      chipManifest(flash.selectedChipId).defaultLogBaudRate,
    );
    flash.selectedChipId = "esp32";
    await nextTick();
    expect(s.baudRate).toBe(chipManifest("esp32").defaultLogBaudRate);
  });

  it("keeps the user's explicit baud after a chip change (override wins)", async () => {
    const flash = useFlashStore();
    const s = useSerialDebugStore();
    s.baudRate = 9600; // setter records a user override
    flash.selectedChipId = "esp32";
    await nextTick();
    expect(s.baudRate).toBe(9600);
  });
});

describe("useSerialDebugStore visible log window", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  function appendLines(
    s: ReturnType<typeof useSerialDebugStore>,
    count: number,
  ): void {
    const text =
      Array.from({ length: count }, (_, i) => `line${i}`).join("\n") + "\n";
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from(text)],
    });
  }

  it("defaults the visible window to 5000 lines", () => {
    const s = useSerialDebugStore();
    expect(DEFAULT_VISIBLE_LOG_WINDOW_LINES).toBe(5000);
    expect(s.logWindowLines).toBe(DEFAULT_VISIBLE_LOG_WINDOW_LINES);
  });

  it("trims already-buffered lines as soon as the limit is lowered", async () => {
    const s = useSerialDebugStore();
    appendLines(s, 20);
    expect(s.lines.length).toBe(20);

    s.logWindowLines = 5;
    await nextTick();

    expect(s.lines.length).toBe(5);
    expect(s.lines[0].text).toBe("line15");
    expect(s.lines[4].text).toBe("line19");
  });

  it("caps newly appended lines at the configured limit", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 3;
    await nextTick();

    appendLines(s, 10);

    expect(s.lines.length).toBe(3);
    expect(s.lines[0].text).toBe("line7");
    expect(s.lines[2].text).toBe("line9");
  });
});

describe("useSerialDebugStore session archive limit", () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it("defaults to 256 MiB and pushes nothing until the value changes", () => {
    const s = useSerialDebugStore();
    expect(DEFAULT_ARCHIVE_LIMIT_MIB).toBe(256);
    expect(s.archiveLimitMib).toBe(DEFAULT_ARCHIVE_LIMIT_MIB);
    expect(fake.archiveLimitCalls).toEqual([]);
  });

  it("pushes the limit to Rust in bytes whenever it changes", async () => {
    const s = useSerialDebugStore();
    s.archiveLimitMib = 512;
    await nextTick();
    expect(fake.archiveLimitCalls).toEqual([512 * 1024 * 1024]);

    s.archiveLimitMib = 64;
    await nextTick();
    expect(fake.archiveLimitCalls).toEqual([
      512 * 1024 * 1024,
      64 * 1024 * 1024,
    ]);
  });

  it("swallows a transport failure — the Rust default cap still applies", async () => {
    const s = useSerialDebugStore();
    fake.setArchiveLimit = async () => {
      throw new Error("archive not ready");
    };
    s.archiveLimitMib = 128;
    await expect(nextTick()).resolves.toBeUndefined();
    expect(s.archiveLimitMib).toBe(128);
  });
});

describe("useSerialDebugStore.setAutoSaveEnabled", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("flips the setting when a directory is already chosen", async () => {
    const s = useSerialDebugStore();
    s.autoSaveDir = "/tmp/logs";

    await s.setAutoSaveEnabled(true);
    expect(s.autoSave).toBe(true);
    expect(s.autoSaveDir).toBe("/tmp/logs");

    await s.setAutoSaveEnabled(false);
    expect(s.autoSave).toBe(false);
    expect(s.autoSaveDir).toBe("/tmp/logs");
  });

  it("does nothing when the value is already what was asked for", async () => {
    const s = useSerialDebugStore();
    s.autoSaveDir = "/tmp/logs";
    s.autoSave = true;

    await s.setAutoSaveEnabled(true);

    expect(s.autoSave).toBe(true);
  });

  // No directory means the auto-save watcher would never start, so enabling
  // asks for one. Outside Tauri the picker is unavailable and the flag is left
  // on but inert — the same state the settings switch has always produced.
  it("asks for a directory when enabling without one", async () => {
    const s = useSerialDebugStore();

    await s.setAutoSaveEnabled(true);

    expect(s.autoSave).toBe(true);
    expect(s.autoSaveDir).toBe("");
  });
});

describe("useSerialDebugStore full-session history window", () => {
  const ENTRY_LINES = HISTORY_PAGE_SIZE * HISTORY_ENTRY_PAGES;
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it("pages the archive tail into the window and shows it instead of the live buffer", async () => {
    const s = useSerialDebugStore();
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("live-a\nlive-b\n")],
    });
    fake.setSessionArchive(2000);

    expect(await s.enterHistoryMode(s.lines.length)).toBe(true);

    expect(s.historyMode).toBe(true);
    expect(s.historyLines.length).toBe(ENTRY_LINES);
    expect(s.historyStartLineNo).toBe(2000 - ENTRY_LINES + 1);
    expect(s.historyEndLineNo).toBe(2000);
    expect(s.historyTotalLines).toBe(2000);
    expect(s.historyAtArchiveEnd).toBe(true);
    expect(s.historyAtSessionStart).toBe(false);
    expect(s.activeDisplayLines()).toBe(s.historyLines);
    expect(s.activeDisplayLines()[0].text).toBe(
      `arch-${2000 - ENTRY_LINES + 1}`,
    );
    // Never one big request: `readSessionPage` holds the Rust archive lock for
    // the whole range, so a long read stalls the serial writer.
    expect(
      fake.readSessionPageCalls.every((c) => c.limit <= HISTORY_PAGE_SIZE),
    ).toBe(true);
    // Positioned so the viewport starts roughly where the live buffer did.
    expect(s.historyEntryOffsetLines).toBe(ENTRY_LINES - s.lines.length);
  });

  it("keeps filling the live buffer while history mode is on, without displaying it", async () => {
    const s = useSerialDebugStore();
    fake.setSessionArchive(1000);
    await s.enterHistoryMode(0);

    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("still-live\n")],
    });

    expect(s.lines[s.lines.length - 1].text).toBe("still-live");
    expect(s.activeDisplayLines()).toBe(s.historyLines);
    expect(s.activeDisplayLines().some((l) => l.text === "still-live")).toBe(
      false,
    );
  });

  it("returns the number of lines it prepended and keeps the window within budget", async () => {
    const s = useSerialDebugStore();
    const budget = ENTRY_LINES + 2 * HISTORY_PAGE_SIZE;
    s.logWindowLines = budget;
    await nextTick();
    fake.setSessionArchive(5000);
    await s.enterHistoryMode(50);
    expect(s.historyStartLineNo).toBe(5000 - ENTRY_LINES + 1);

    // Steps that still fit inside the budget leave the window's tail alone.
    for (let i = 1; i <= 2; i += 1) {
      expect(await s.loadOlderHistory()).toEqual({
        prepended: HISTORY_PAGE_SIZE,
        reanchored: false,
      });
      expect(s.historyLines.length).toBe(ENTRY_LINES + i * HISTORY_PAGE_SIZE);
      expect(s.historyAtArchiveEnd).toBe(true);
    }

    // The step that overflows trims the *tail*, which sits below the viewport,
    // so nothing the user is looking at moves.
    expect((await s.loadOlderHistory()).prepended).toBe(HISTORY_PAGE_SIZE);
    expect(s.historyLines.length).toBe(budget);
    expect(s.historyAtArchiveEnd).toBe(false);
    expect(s.historyStartLineNo).toBe(
      5000 - ENTRY_LINES + 1 - 3 * HISTORY_PAGE_SIZE,
    );
    expect(s.historyLines[0].text).toBe(`arch-${s.historyStartLineNo}`);

    for (let i = 0; i < 10; i += 1) await s.loadOlderHistory();
    expect(s.historyLines.length).toBe(budget);
  });

  it("stops at the session start, detected from lineNo 1 rather than page.start", async () => {
    const s = useSerialDebugStore();
    fake.setSessionArchive(ENTRY_LINES + 250);
    await s.enterHistoryMode(10);
    expect(s.historyAtSessionStart).toBe(false);

    expect((await s.loadOlderHistory()).prepended).toBe(250);
    expect(s.historyStartLineNo).toBe(1);
    expect(s.historyAtSessionStart).toBe(true);

    fake.readSessionPageCalls.length = 0;
    expect(await s.loadOlderHistory()).toEqual({
      prepended: 0,
      reanchored: false,
    });
    expect(fake.readSessionPageCalls).toEqual([]);
  });

  it("re-anchors on the archive tail instead of splicing a non-contiguous page", async () => {
    const s = useSerialDebugStore();
    fake.setSessionArchive(2000);
    await s.enterHistoryMode(50);
    const firstLineNo = s.historyStartLineNo;

    // A head-dropping archive policy would answer the "give me the 400 lines
    // before firstLineNo" request with renumbered lines. Splicing them on would
    // silently show the wrong content, so the self-check must refuse.
    const badStart = firstLineNo - 1 - HISTORY_PAGE_SIZE;
    fake.setSessionPageResponder((start, limit) => ({
      totalLines: 2000,
      start,
      items: Array.from({ length: Math.min(limit, 2000 - start) }, (_, i) => ({
        lineNo: start + i + 1 + (start === badStart ? 25 : 0),
        tsMs: 0,
        direction: "rx" as const,
        text: `arch-${start + i + 1}`,
      })),
    }));

    expect(await s.loadOlderHistory()).toEqual({
      prepended: 0,
      reanchored: true,
    });
    expect(s.historyMode).toBe(true);
    expect(s.historyStartLineNo).toBe(firstLineNo);
    expect(s.historyLines.length).toBe(ENTRY_LINES);
  });

  it("pages forward again and drops from the head to stay within budget", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 1500;
    await nextTick();
    fake.setSessionArchive(5000);
    await s.enterHistoryMode(50);
    expect(await s.jumpToSessionStart()).toBe(true);
    expect(s.historyStartLineNo).toBe(1);
    expect(s.historyAtSessionStart).toBe(true);
    expect(s.historyAtArchiveEnd).toBe(false);

    const dropped = await s.loadNewerHistory();

    expect(dropped).toBe(ENTRY_LINES + HISTORY_PAGE_SIZE - 1500);
    expect(s.historyLines.length).toBe(1500);
    expect(s.historyStartLineNo).toBe(1 + dropped);
    expect(s.historyAtSessionStart).toBe(false);
    expect(s.historyAtArchiveEnd).toBe(false);
  });

  it("reports being at the archive end once a short page comes back", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 20000;
    await nextTick();
    fake.setSessionArchive(ENTRY_LINES + 100);
    await s.enterHistoryMode(10);
    await s.jumpToSessionStart();
    expect(s.historyAtArchiveEnd).toBe(false);

    expect(await s.loadNewerHistory()).toBe(0);

    expect(s.historyAtArchiveEnd).toBe(true);
    expect(s.historyEndLineNo).toBe(ENTRY_LINES + 100);
  });

  it("leaves history mode on clear — the new session renumbers from 1", async () => {
    const s = useSerialDebugStore();
    fake.setSessionArchive(2000);
    await s.enterHistoryMode(10);
    expect(s.historyMode).toBe(true);

    await s.clear();

    expect(s.historyMode).toBe(false);
    expect(s.historyLines).toEqual([]);
    expect(s.historyStartLineNo).toBe(0);
    expect(s.activeDisplayLines()).toBe(s.lines);
  });

  it("does not let a read that resolves after a clear resurrect the window", async () => {
    const s = useSerialDebugStore();
    fake.setSessionArchive(2000);
    await s.enterHistoryMode(10);
    let release: (() => void) | null = null;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const passthrough = fake.readSessionPage.bind(fake);
    fake.readSessionPage = async (start: number, limit: number) => {
      await gate;
      return passthrough(start, limit);
    };

    const pending = s.loadOlderHistory();
    await s.clear();
    release!();
    expect(await pending).toEqual({ prepended: 0, reanchored: false });

    expect(s.historyMode).toBe(false);
    expect(s.historyLines).toEqual([]);
  });

  it("keeps the reading window and exposes a retryable error when the archive read fails", async () => {
    const s = useSerialDebugStore();
    fake.setSessionArchive(2000);
    await s.enterHistoryMode(10);
    const before = s.historyLines;
    fake.setSessionPageResponder(() => {
      throw new Error("archive gone");
    });

    expect((await s.loadOlderHistory()).prepended).toBe(0);

    expect(s.historyMode).toBe(true);
    expect(s.historyLines).toBe(before);
    expect(s.historyError).toBe("archive gone");
  });

  it("does not enter history mode when the archive is empty", async () => {
    const s = useSerialDebugStore();
    fake.setSessionArchive(0);
    expect(await s.enterHistoryMode(10)).toBe(false);
    expect(s.historyMode).toBe(false);
    expect(s.activeDisplayLines()).toBe(s.lines);
  });

  it("bounds how many filter matches the older-matches button accumulates", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 1000;
    await nextTick();
    fake.readFilterMatches = async (
      filterId: string,
      start: number,
      limit: number,
    ) => ({
      filterId,
      totalMatches: 10_000,
      start,
      items: Array.from({ length: limit }, (_, i) => ({
        lineNo: start + i + 1,
        tsMs: 0,
        direction: "rx" as const,
        text: `m-${start + i + 1}`,
      })),
    });
    s.watchChips = [{ id: "f1", keyword: "m", useRegex: false, color: "#000" }];
    s.filterStatsById = {
      f1: {
        filterId: "f1",
        status: "complete",
        scannedUntilLineNo: 0,
        totalLinesSnapshot: 0,
        totalMatches: 10_000,
        error: null,
      },
    };
    await s.setActiveChip("f1");
    expect(s.filterPagesById.f1.items.length).toBe(FILTER_PAGE_SIZE);

    for (let i = 0; i < 5; i += 1) {
      expect(await s.loadOlderActiveFilterMatches()).toBe(FILTER_PAGE_SIZE);
    }

    // Bounded, and trimmed from the tail so the scroll position is unaffected.
    expect(s.filterPagesById.f1.items.length).toBe(1000);
    expect(s.filterPagesById.f1.start).toBe(10_000 - 6 * FILTER_PAGE_SIZE);
    expect(s.filterPagesById.f1.items[0].text).toBe(
      `m-${10_000 - 6 * FILTER_PAGE_SIZE + 1}`,
    );
  });
});
