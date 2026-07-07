// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DebugChunk, DebugConfig } from "./types";

// Web mode → serialDebugTransport() builds a WebTransport that delegates to wsTransport.
vi.mock("@/runtime", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/runtime")>();
  return { ...actual, isTauriRuntime: vi.fn(() => false) };
});

// Capture the callbacks WebTransport hands to wsTransport.serialDebugOpen so the
// test can drive chunk/disconnect events synchronously.
const wsState: {
  cfg: DebugConfig | null;
  onChunk: ((c: DebugChunk) => void) | null;
  onChunkBatch: ((chunks: DebugChunk[]) => void) | null;
  onDisconnect: ((reason: string) => void) | null;
  onFilterUpdated: ((payload: unknown) => void) | null;
  closed: number;
  sent: Uint8Array[];
} = {
  cfg: null,
  onChunk: null,
  onChunkBatch: null,
  onDisconnect: null,
  onFilterUpdated: null,
  closed: 0,
  sent: [],
};

vi.mock("@/transport/ws-transport", () => ({
  wsTransport: {
    serialDebugOpen: vi.fn(
      async (
        cfg: DebugConfig,
        onChunk: (c: DebugChunk) => void,
        onChunkBatch: (chunks: DebugChunk[]) => void,
        onDisconnect: (reason: string) => void,
        onFilterUpdated: (payload: unknown) => void,
      ) => {
        wsState.cfg = cfg;
        wsState.onChunk = onChunk;
        wsState.onChunkBatch = onChunkBatch;
        wsState.onDisconnect = onDisconnect;
        wsState.onFilterUpdated = onFilterUpdated;
      },
    ),
    serialDebugClose: vi.fn(async () => {
      wsState.closed += 1;
    }),
    serialDebugSend: vi.fn(async (bytes: Uint8Array) => {
      wsState.sent.push(bytes);
    }),
    serialDebugSessionClear: vi.fn(async () => {}),
    serialDebugAppendSysLine: vi.fn(async () => {}),
    serialDebugFilterAdd: vi.fn(async () => ({
      def: { id: "filter-1", keyword: "ERR", useRegex: false, color: "#f00" },
      stats: {
        filterId: "filter-1",
        status: "complete",
        scannedUntilLineNo: 0,
        totalLinesSnapshot: 0,
        totalMatches: 0,
        error: null,
      },
    })),
    serialDebugFilterRemove: vi.fn(async () => {}),
    serialDebugFilterReadMatches: vi.fn(async (filterId: string) => ({
      filterId,
      totalMatches: 0,
      start: 0,
      items: [],
    })),
    serialDebugSessionReadPage: vi.fn(async () => ({
      totalLines: 0,
      start: 0,
      items: [],
    })),
  },
}));

import {
  __setSerialDebugTransportForTest,
  serialDebugTransport,
  type SerialDebugTransport,
} from "./transport";
import { wsTransport } from "@/transport/ws-transport";

const cfg: DebugConfig = {
  port: "/dev/ttyUSB0",
  baudRate: 115200,
  dataBits: "eight",
  parity: "none",
  stopBits: "one",
};

describe("serialDebugTransport factory", () => {
  beforeEach(() => {
    __setSerialDebugTransportForTest(null);
    wsState.cfg = null;
    wsState.onChunk = null;
    wsState.onChunkBatch = null;
    wsState.onDisconnect = null;
    wsState.onFilterUpdated = null;
    wsState.closed = 0;
    wsState.sent = [];
    vi.clearAllMocks();
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it("returns the same singleton across calls", () => {
    const a = serialDebugTransport();
    const b = serialDebugTransport();
    expect(a).toBe(b);
  });

  it("__setSerialDebugTransportForTest replaces the singleton", () => {
    const fake = {} as SerialDebugTransport;
    __setSerialDebugTransportForTest(fake);
    expect(serialDebugTransport()).toBe(fake);
  });
});

describe("WebTransport (web mode delegates to wsTransport)", () => {
  beforeEach(() => {
    __setSerialDebugTransportForTest(null);
    wsState.cfg = null;
    wsState.onChunk = null;
    wsState.onChunkBatch = null;
    wsState.onDisconnect = null;
    wsState.onFilterUpdated = null;
    wsState.closed = 0;
    wsState.sent = [];
    vi.clearAllMocks();
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it("open forwards the config to wsTransport.serialDebugOpen", async () => {
    const tr = serialDebugTransport();
    await tr.open(cfg);
    expect(wsTransport.serialDebugOpen).toHaveBeenCalledTimes(1);
    expect(wsState.cfg).toEqual(cfg);
  });

  it("onChunk subscribers receive chunks emitted by wsTransport", async () => {
    const tr = serialDebugTransport();
    const received: DebugChunk[] = [];
    const unsub = tr.onChunk((c) => received.push(c));
    await tr.open(cfg);

    const chunk: DebugChunk = { direction: "rx", tsMs: 1, bytes: [0x61] };
    wsState.onChunk?.(chunk);
    expect(received).toEqual([chunk]);

    // unsubscribe stops further delivery
    unsub();
    wsState.onChunk?.({ direction: "rx", tsMs: 2, bytes: [0x62] });
    expect(received.length).toBe(1);
  });

  it("onChunkBatch subscribers receive chunk batches emitted by wsTransport", async () => {
    const tr = serialDebugTransport();
    const received: DebugChunk[][] = [];
    const unsub = tr.onChunkBatch((chunks) => received.push(chunks));
    await tr.open(cfg);

    const batch: DebugChunk[] = [
      { direction: "rx", tsMs: 1, bytes: [0x61] },
      { direction: "rx", tsMs: 2, bytes: [0x62, 0x63] },
    ];
    wsState.onChunkBatch?.(batch);
    expect(received).toEqual([batch]);

    unsub();
    wsState.onChunkBatch?.([{ direction: "rx", tsMs: 3, bytes: [0x64] }]);
    expect(received).toHaveLength(1);
  });

  it("chunk batch input falls back to onChunk delivery when no batch listener is registered", async () => {
    const tr = serialDebugTransport();
    const received: DebugChunk[] = [];
    tr.onChunk((chunk) => received.push(chunk));
    await tr.open(cfg);

    wsState.onChunkBatch?.([
      { direction: "rx", tsMs: 1, bytes: [0x61] },
      { direction: "rx", tsMs: 2, bytes: [0x62, 0x63] },
    ]);

    expect(received).toEqual([
      { direction: "rx", tsMs: 1, bytes: [0x61] },
      { direction: "rx", tsMs: 2, bytes: [0x62, 0x63] },
    ]);
  });

  it("onDisconnect subscribers receive the reason wrapped as a payload", async () => {
    const tr = serialDebugTransport();
    const reasons: string[] = [];
    const unsub = tr.onDisconnect((p) => reasons.push(p.reason));
    await tr.open(cfg);

    wsState.onDisconnect?.("unplugged");
    expect(reasons).toEqual(["unplugged"]);

    unsub();
    wsState.onDisconnect?.("again");
    expect(reasons.length).toBe(1);
  });

  it("send forwards bytes to wsTransport.serialDebugSend", async () => {
    const tr = serialDebugTransport();
    const bytes = Uint8Array.from([1, 2, 3]);
    await tr.send(bytes);
    expect(wsTransport.serialDebugSend).toHaveBeenCalledTimes(1);
    expect(wsState.sent[0]).toBe(bytes);
  });

  it("readSessionPage delegates to wsTransport.serialDebugSessionReadPage", async () => {
    const tr = serialDebugTransport();
    await tr.readSessionPage(200, 100);
    expect(wsTransport.serialDebugSessionReadPage).toHaveBeenCalledWith(
      200,
      100,
    );
  });

  it("close calls wsTransport.serialDebugClose only when open", async () => {
    const tr = serialDebugTransport();
    // close before open → no-op (isOpen false guard)
    await tr.close();
    expect(wsTransport.serialDebugClose).not.toHaveBeenCalled();

    await tr.open(cfg);
    await tr.close();
    expect(wsTransport.serialDebugClose).toHaveBeenCalledTimes(1);
    expect(wsState.closed).toBe(1);

    // closing again is a no-op
    await tr.close();
    expect(wsTransport.serialDebugClose).toHaveBeenCalledTimes(1);
  });
});
