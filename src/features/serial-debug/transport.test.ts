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
  onDisconnect: ((reason: string) => void) | null;
  closed: number;
  sent: Uint8Array[];
} = { cfg: null, onChunk: null, onDisconnect: null, closed: 0, sent: [] };

vi.mock("@/transport/ws-transport", () => ({
  wsTransport: {
    serialDebugOpen: vi.fn(
      async (
        cfg: DebugConfig,
        onChunk: (c: DebugChunk) => void,
        onDisconnect: (reason: string) => void,
      ) => {
        wsState.cfg = cfg;
        wsState.onChunk = onChunk;
        wsState.onDisconnect = onDisconnect;
      },
    ),
    serialDebugClose: vi.fn(async () => {
      wsState.closed += 1;
    }),
    serialDebugSend: vi.fn(async (bytes: Uint8Array) => {
      wsState.sent.push(bytes);
    }),
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
    wsState.onDisconnect = null;
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
    wsState.onDisconnect = null;
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
