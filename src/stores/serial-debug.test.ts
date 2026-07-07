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
} from "@/features/serial-debug/types";
import { useSerialDebugStore } from "./serial-debug";
import { useFlashStore } from "./flash";
import { nextTick } from "vue";
import { MAX_PENDING_LINE_BYTES } from "@/features/serial-debug/constants";

async function waitForChunkFrame(): Promise<void> {
  await new Promise((r) => setTimeout(r, 20));
  await nextTick();
}

function fakeTransport(): SerialDebugTransport & {
  emitChunk: (c: DebugChunk) => void;
  emitChunkBatch: (chunks: DebugChunk[]) => void;
  emitDisconnect: (reason: string) => void;
  emitFilterUpdated: (payload: SerialDebugFilterUpdatePayload) => void;
  readFilterMatchesCalls: Array<{
    filterId: string;
    start?: number;
    limit?: number;
  }>;
  sent: Uint8Array[];
  opened: boolean;
} {
  const chunkListeners = new Set<(c: DebugChunk) => void>();
  const chunkBatchListeners = new Set<(chunks: DebugChunk[]) => void>();
  const discListeners = new Set<(p: { reason: string }) => void>();
  const filterListeners = new Set<
    (p: SerialDebugFilterUpdatePayload) => void
  >();
  const sent: Uint8Array[] = [];
  const readFilterMatchesCalls: Array<{
    filterId: string;
    start?: number;
    limit?: number;
  }> = [];
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
    async appendSysLine() {},
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
    async readSessionPage() {
      return {
        totalLines: 0,
        start: 0,
        items: [],
      };
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
    const bytes = [...Buffer.from(oneLine.repeat(3505))];
    s.appendChunk({ direction: "rx", tsMs: 1000, bytes });
    expect(s.lines.length).toBe(3000);
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
    expect(s.sendHistory).toEqual(["A", "B"]);
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

  it("chips cycle colors from CHIP_COLORS when added", async () => {
    const { CHIP_COLORS } = await import("@/features/serial-debug/constants");
    const s = useSerialDebugStore();
    for (let i = 0; i < CHIP_COLORS.length + 1; i++) {
      await s.addChip(`kw${i}`, false);
    }
    expect(s.watchChips[0].color).toBe(CHIP_COLORS[0]);
    expect(s.watchChips[CHIP_COLORS.length].color).toBe(CHIP_COLORS[0]);
  });
});

describe("useSerialDebugStore.deviceReset", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("uses provided resetPort when given", async () => {
    const spy = vi
      .spyOn(wsTransport, "deviceReset")
      .mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";

    await s.deviceReset("T5AI", "/dev/ttyACM0");

    expect(spy).toHaveBeenCalledWith("/dev/ttyACM0", "T5AI");
  });

  it("falls back to port.value when resetPort is not provided", async () => {
    const spy = vi
      .spyOn(wsTransport, "deviceReset")
      .mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";

    await s.deviceReset("T5AI");

    expect(spy).toHaveBeenCalledWith("/dev/ttyACM1", "T5AI");
  });

  it("falls back to port.value when resetPort is empty string", async () => {
    const spy = vi
      .spyOn(wsTransport, "deviceReset")
      .mockResolvedValue(undefined);
    const s = useSerialDebugStore();
    s.port = "/dev/ttyACM1";

    await s.deviceReset("T5AI", "");

    expect(spy).toHaveBeenCalledWith("/dev/ttyACM1", "T5AI");
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
      async appendSysLine() {},
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
