import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Control wsUrl() deterministically (node env has no window).
const { getWsUrl } = vi.hoisted(() => ({
  getWsUrl: vi.fn(() => "ws://test-host:9527"),
}));
vi.mock("@/platform", () => ({
  platform: { getWsUrl, pickFile: vi.fn() },
}));

import { WsTransport, wsTransport } from "./ws-transport";
import type { FlashJobPayload } from "@/features/firmware-flash/flash-ipc-types";

// ---------------------------------------------------------------------------
// Controllable mock WebSocket.
//
// Mirrors the readyState constants the production code reads (WebSocket.OPEN,
// WebSocket.CONNECTING). Records every frame sent (parsed JSON), and exposes
// helpers to drive the connection lifecycle + push server messages.
// ---------------------------------------------------------------------------
type Listener = (ev: { data: string }) => void;

class MockWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  static instances: MockWebSocket[] = [];

  readyState = MockWebSocket.CONNECTING;
  url: string;

  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;

  sent: unknown[] = [];
  closed = false;
  private messageListeners = new Set<Listener>();

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
  }

  addEventListener(type: string, fn: Listener) {
    if (type === "message") this.messageListeners.add(fn);
  }
  removeEventListener(type: string, fn: Listener) {
    if (type === "message") this.messageListeners.delete(fn);
  }

  send(data: string) {
    this.sent.push(JSON.parse(data));
  }
  close() {
    this.closed = true;
    this.readyState = MockWebSocket.CLOSED;
  }

  // --- test driving helpers ---
  open() {
    this.readyState = MockWebSocket.OPEN;
    this.onopen?.();
  }
  error() {
    this.onerror?.();
  }
  fireClose() {
    this.onclose?.();
  }
  /** Deliver a server frame to all registered message listeners. */
  recv(obj: unknown) {
    const ev = { data: JSON.stringify(obj) };
    for (const fn of [...this.messageListeners]) fn(ev);
  }
  /** Deliver a raw (possibly non-JSON) frame. */
  recvRaw(data: string) {
    for (const fn of [...this.messageListeners]) fn({ data });
  }
  get lastSent() {
    return this.sent[this.sent.length - 1];
  }
  get listenerCount() {
    return this.messageListeners.size;
  }
}

function latest(): MockWebSocket {
  return MockWebSocket.instances[MockWebSocket.instances.length - 1];
}

/** Wait a macrotask so connect()'s onopen-resolved promise chain flushes. */
function flush() {
  return new Promise((r) => setTimeout(r, 0));
}

beforeEach(() => {
  MockWebSocket.instances = [];
  getWsUrl.mockReturnValue("ws://test-host:9527");
  vi.stubGlobal("WebSocket", MockWebSocket as unknown as typeof WebSocket);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("wsUrl resolution", () => {
  it("uses the injected platform.getWsUrl() value", async () => {
    getWsUrl.mockReturnValue("ws://injected:1234");
    const t = new WsTransport();
    const p = t.isAvailable();
    await flush();
    latest().open();
    await p;
    expect(latest().url).toBe("ws://injected:1234");
  });

  it("falls back to 127.0.0.1:9527 when no URL is injected (node env, no window)", async () => {
    getWsUrl.mockReturnValue("");
    const t = new WsTransport();
    const p = t.isAvailable();
    await flush();
    latest().open();
    await p;
    expect(latest().url).toBe("ws://127.0.0.1:9527");
  });
});

describe("connect lifecycle (isAvailable)", () => {
  it("resolves true once the socket opens", async () => {
    const t = new WsTransport();
    const p = t.isAvailable();
    await flush();
    latest().open();
    await expect(p).resolves.toBe(true);
  });

  it("resolves false when the socket errors before opening", async () => {
    const t = new WsTransport();
    const p = t.isAvailable();
    await flush();
    latest().error();
    await expect(p).resolves.toBe(false);
  });

  it("reuses an already-open connection instead of opening a second socket", async () => {
    const t = new WsTransport();
    const p1 = t.isAvailable();
    await flush();
    latest().open();
    await p1;
    expect(MockWebSocket.instances.length).toBe(1);

    // Second call should reuse the open ws (no new instance).
    await expect(t.isAvailable()).resolves.toBe(true);
    expect(MockWebSocket.instances.length).toBe(1);
  });

  it("clears cached connection on close", async () => {
    const t = new WsTransport();
    const p = t.isAvailable();
    await flush();
    const ws = latest();
    ws.open();
    await p;

    ws.fireClose();
    // After close the cached ws is dropped; next connect opens a fresh socket.
    const p2 = t.isAvailable();
    await flush();
    latest().open();
    await p2;
    expect(MockWebSocket.instances.length).toBe(2);
  });
});

describe("deviceReset", () => {
  it("sends a device_reset frame and resolves on ok result", async () => {
    const t = new WsTransport();
    const p = t.deviceReset("/dev/ttyUSB0", "BK7231N");
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    expect(ws.lastSent).toEqual({
      type: "device_reset",
      port: "/dev/ttyUSB0",
      chip_id: "BK7231N",
    });
    ws.recv({ type: "device_reset_result", ok: true });
    await expect(p).resolves.toBeUndefined();
    expect(ws.listenerCount).toBe(0); // handler removed
  });

  it("rejects when result.ok is false", async () => {
    const t = new WsTransport();
    const p = t.deviceReset("/dev/ttyUSB0", "T5AI");
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({ type: "device_reset_result", ok: false, error: "boom" });
    await expect(p).rejects.toThrow("boom");
  });

  it("rejects on an error frame", async () => {
    const t = new WsTransport();
    const p = t.deviceReset("/dev/ttyUSB0", "T5AI");
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({ type: "error", message: "server exploded" });
    await expect(p).rejects.toThrow("server exploded");
  });

  it("ignores non-JSON frames and resolves on the later valid result", async () => {
    const t = new WsTransport();
    const p = t.deviceReset("/dev/ttyUSB0", "T5AI");
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recvRaw("not json");
    ws.recv({ type: "device_reset_result", ok: true });
    await expect(p).resolves.toBeUndefined();
  });

  it("rejects after the 15s timeout when no response arrives", async () => {
    vi.useFakeTimers();
    const t = new WsTransport();
    const p = t.deviceReset("/dev/ttyUSB0", "T5AI");
    // connect() resolves via microtasks; advance them.
    await vi.advanceTimersByTimeAsync(0);
    latest().open();
    await vi.advanceTimersByTimeAsync(0);
    const rejected = expect(p).rejects.toThrow(/deviceReset timeout/);
    await vi.advanceTimersByTimeAsync(15000);
    await rejected;
  });
});

describe("listPorts", () => {
  it("sends list_ports and normalizes string entries to {path}", async () => {
    const t = new WsTransport();
    const p = t.listPorts();
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    expect(ws.lastSent).toEqual({ type: "list_ports" });
    ws.recv({
      type: "ports",
      ports: ["/dev/ttyUSB0", { path: "/dev/ttyUSB1", productName: "X" }],
    });
    await expect(p).resolves.toEqual([
      { path: "/dev/ttyUSB0" },
      { path: "/dev/ttyUSB1", productName: "X" },
    ]);
  });

  it("resolves to [] when ports is omitted", async () => {
    const t = new WsTransport();
    const p = t.listPorts();
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({ type: "ports" });
    await expect(p).resolves.toEqual([]);
  });

  it("closes any existing connection first (forces a reconnect)", async () => {
    const t = new WsTransport();
    // Establish an initial open connection.
    const a = t.isAvailable();
    await flush();
    const first = latest();
    first.open();
    await a;

    // listPorts should close `first` and open a fresh socket.
    const p = t.listPorts();
    await flush();
    expect(first.closed).toBe(true);
    expect(MockWebSocket.instances.length).toBe(2);
    const second = latest();
    second.open();
    await flush();
    second.recv({ type: "ports", ports: [] });
    await expect(p).resolves.toEqual([]);
  });

  it("rejects after the 5s timeout", async () => {
    vi.useFakeTimers();
    const t = new WsTransport();
    const p = t.listPorts();
    await vi.advanceTimersByTimeAsync(0);
    latest().open();
    await vi.advanceTimersByTimeAsync(0);
    const rejected = expect(p).rejects.toThrow("listPorts timeout");
    await vi.advanceTimersByTimeAsync(5000);
    await rejected;
  });
});

function makeJob(mode: FlashJobPayload["mode"]): FlashJobPayload {
  return {
    mode,
    chipId: "BK7231N",
    port: "/dev/ttyUSB0",
    baudRate: 921600,
    segments: null,
    flashStartHex: null,
    flashEndHex: null,
    eraseStartHex: null,
    eraseEndHex: null,
    readStartHex: null,
    readEndHex: null,
    readFilePath: null,
    firmwarePath: null,
    authorizeUuid: null,
    authorizeKey: null,
  };
}

describe("runJob", () => {
  it("streams progress then resolves on done.ok", async () => {
    const t = new WsTransport();
    const progress: unknown[] = [];
    const p = t.runJob(makeJob("erase"), [], (ev) => progress.push(ev));
    await flush();
    const ws = latest();
    ws.open();
    await flush();

    const sent = ws.lastSent as { type: string; job: { mode: string } };
    expect(sent.type).toBe("run_job");
    expect(sent.job.mode).toBe("erase");

    ws.recv({ type: "progress", payload: { kind: "phase", phase: "erase" } });
    ws.recv({ type: "progress", payload: { kind: "percent", value: 50 } });
    ws.recv({
      type: "progress",
      payload: { kind: "done", result: { ok: { elapsed_secs: 1 } } },
    });

    await expect(p).resolves.toBeUndefined();
    expect(progress.length).toBe(3);
    expect(progress[1]).toMatchObject({
      payload: { kind: "percent", value: 50 },
    });
    expect(ws.listenerCount).toBe(0);
  });

  it("buffers a file_content frame and attaches it to the next progress event", async () => {
    const t = new WsTransport();
    const progress: Array<{
      payload: { kind: string };
      fileContent?: { name: string; content: string } | null;
    }> = [];
    const p = t.runJob(makeJob("read"), [], (ev) => progress.push(ev as never));
    await flush();
    const ws = latest();
    ws.open();
    await flush();

    // file_content does not itself produce a progress callback.
    ws.recv({
      type: "progress",
      payload: { kind: "file_content", name: "dump.bin", content: "QUJD" },
    });
    expect(progress.length).toBe(0);

    // Next progress event carries the buffered file content.
    ws.recv({
      type: "progress",
      payload: { kind: "done", result: { ok: { elapsed_secs: 1 } } },
    });
    await p;
    expect(progress.length).toBe(1);
    expect(progress[0].fileContent).toEqual({
      name: "dump.bin",
      content: "QUJD",
    });
  });

  it("defaults file_content name/content when omitted", async () => {
    const t = new WsTransport();
    const progress: Array<{ fileContent?: { name: string } | null }> = [];
    const p = t.runJob(makeJob("read"), [], (ev) => progress.push(ev as never));
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({ type: "progress", payload: { kind: "file_content" } });
    ws.recv({
      type: "progress",
      payload: { kind: "done", result: { ok: { elapsed_secs: 1 } } },
    });
    await p;
    expect(progress[0].fileContent).toEqual({ name: "read.bin", content: "" });
  });

  it("rejects with 'Cancelled' on a done.cancelled result", async () => {
    const t = new WsTransport();
    const p = t.runJob(makeJob("flash"), [], () => {});
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({
      type: "progress",
      payload: { kind: "done", result: { cancelled: { elapsed_secs: 1 } } },
    });
    await expect(p).rejects.toThrow("Cancelled");
  });

  it("rejects with the error message on a done.err result", async () => {
    const t = new WsTransport();
    const p = t.runJob(makeJob("flash"), [], () => {});
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({
      type: "progress",
      payload: {
        kind: "done",
        result: { err: { message: "write failed", elapsed_secs: 1 } },
      },
    });
    await expect(p).rejects.toThrow("write failed");
  });

  it("rejects on a top-level error frame", async () => {
    const t = new WsTransport();
    const p = t.runJob(makeJob("flash"), [], () => {});
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({ type: "error", message: "abort" });
    await expect(p).rejects.toThrow("abort");
    expect(ws.listenerCount).toBe(0);
  });

  it("base64-encodes a single firmware file into file_content", async () => {
    const t = new WsTransport();
    const file = new File([new Uint8Array([65, 66, 67])], "fw.bin"); // "ABC"
    const p = t.runJob(makeJob("flash"), [file], () => {});
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    const sent = ws.lastSent as { file_content?: string };
    expect(sent.file_content).toBe("QUJD"); // base64("ABC")
    ws.recv({
      type: "progress",
      payload: { kind: "done", result: { ok: { elapsed_secs: 1 } } },
    });
    await p;
  });

  it("base64-encodes multiple segment files into file_contents", async () => {
    const t = new WsTransport();
    const job = makeJob("flash");
    job.segments = [
      { firmwarePath: "a", startAddr: "0x0", endAddr: "0x10" },
      { firmwarePath: "b", startAddr: "0x20", endAddr: "0x30" },
    ];
    const fileA = new File([new Uint8Array([65, 66, 67])], "a.bin"); // ABC
    const p = t.runJob(job, [fileA, null], () => {});
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    const sent = ws.lastSent as { file_contents?: string[] };
    expect(sent.file_contents).toEqual(["QUJD", ""]);
    ws.recv({
      type: "progress",
      payload: { kind: "done", result: { ok: { elapsed_secs: 1 } } },
    });
    await p;
  });
});

describe("authorizeConfirm", () => {
  it("sends authorize_confirm frame when socket is open", async () => {
    const t = new WsTransport();
    const a = t.isAvailable();
    await flush();
    const ws = latest();
    ws.open();
    await a;
    t.authorizeConfirm(true);
    expect(ws.lastSent).toEqual({ type: "authorize_confirm", confirmed: true });
  });

  it("sends authorize_confirm false when user declines", async () => {
    const t = new WsTransport();
    const a = t.isAvailable();
    await flush();
    const ws = latest();
    ws.open();
    await a;
    t.authorizeConfirm(false);
    expect(ws.lastSent).toEqual({
      type: "authorize_confirm",
      confirmed: false,
    });
  });

  it("is a no-op when there is no open socket", () => {
    const t = new WsTransport();
    expect(() => t.authorizeConfirm(true)).not.toThrow();
  });
});

describe("cancelJob", () => {
  it("sends a cancel frame when the socket is open", async () => {
    const t = new WsTransport();
    const a = t.isAvailable();
    await flush();
    const ws = latest();
    ws.open();
    await a;
    t.cancelJob();
    expect(ws.lastSent).toEqual({ type: "cancel" });
  });

  it("is a no-op when there is no open socket", () => {
    const t = new WsTransport();
    expect(() => t.cancelJob()).not.toThrow();
  });
});

describe("serialDebugOpen / send / close", () => {
  it("resolves on serial_debug_opened and routes chunks to onChunk", async () => {
    const t = new WsTransport();
    const chunks: unknown[] = [];
    const chunkBatches: unknown[] = [];
    const disconnects: string[] = [];
    const cfg = { port: "/dev/ttyUSB0", baudRate: 115200 } as never;
    const p = t.serialDebugOpen(
      cfg,
      (c) => chunks.push(c),
      (batch) => chunkBatches.push(batch),
      (r) => disconnects.push(r),
      () => {},
    );
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    expect(ws.lastSent).toEqual({ type: "serial_debug_open", cfg });

    ws.recv({ type: "serial_debug_opened" });
    await expect(p).resolves.toBeUndefined();

    const chunk = { direction: "rx", tsMs: 1, bytes: [1, 2, 3] };
    ws.recv({ type: "serial_debug_chunk", chunk });
    expect(chunks).toEqual([chunk]);
    expect(chunkBatches).toEqual([]);

    ws.recv({ type: "serial_debug_disconnected", reason: "unplugged" });
    expect(disconnects).toEqual(["unplugged"]);
  });

  it("routes serial_debug_chunk_batch frames to onChunkBatch in order", async () => {
    const t = new WsTransport();
    const chunks: unknown[] = [];
    const chunkBatches: unknown[] = [];
    const p = t.serialDebugOpen(
      { port: "x" } as never,
      (c) => chunks.push(c),
      (batch) => chunkBatches.push(batch),
      () => {},
      () => {},
    );
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({ type: "serial_debug_opened" });
    await p;

    ws.recv({
      type: "serial_debug_chunk_batch",
      chunks: [
        { direction: "rx", tsMs: 1, bytes: [1] },
        { direction: "rx", tsMs: 2, bytes: [2, 3] },
      ],
    });

    expect(chunks).toEqual([]);
    expect(chunkBatches).toEqual([
      [
        { direction: "rx", tsMs: 1, bytes: [1] },
        { direction: "rx", tsMs: 2, bytes: [2, 3] },
      ],
    ]);
  });

  it("rejects serialDebugOpen on an error frame", async () => {
    const t = new WsTransport();
    const p = t.serialDebugOpen(
      { port: "x" } as never,
      () => {},
      () => {},
      () => {},
      () => {},
    );
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({ type: "error", message: "open failed" });
    await expect(p).rejects.toThrow("open failed");
  });

  it("serialDebugSend sends a serial_debug_send frame with byte array", async () => {
    const t = new WsTransport();
    const p = t.serialDebugSend(new Uint8Array([10, 20, 30]));
    await flush();
    const ws = latest();
    ws.open();
    await p;
    expect(ws.lastSent).toEqual({
      type: "serial_debug_send",
      bytes: [10, 20, 30],
    });
  });

  it("serialDebugClose detaches the chunk handler and sends close frame", async () => {
    const t = new WsTransport();
    const chunks: unknown[] = [];
    const p = t.serialDebugOpen(
      { port: "x" } as never,
      (c) => chunks.push(c),
      () => {},
      () => {},
      () => {},
    );
    await flush();
    const ws = latest();
    ws.open();
    await flush();
    ws.recv({ type: "serial_debug_opened" });
    await p;

    await t.serialDebugClose();
    expect(ws.lastSent).toEqual({ type: "serial_debug_close" });

    // After close, chunk frames are no longer routed.
    ws.recv({ type: "serial_debug_chunk", chunk: { direction: "rx" } });
    expect(chunks).toEqual([]);
  });
});

describe("serialDebugSessionReadPage", () => {
  it("requests a session page and resolves on serial_debug_session_page", async () => {
    const t = new WsTransport();
    const p = t.serialDebugSessionReadPage(100, 50);
    await flush();
    const ws = latest();
    ws.open();
    await flush();

    expect(ws.lastSent).toEqual({
      type: "serial_debug_session_read_page",
      start: 100,
      limit: 50,
    });

    ws.recv({
      type: "serial_debug_session_page",
      page: {
        totalLines: 123,
        start: 100,
        items: [{ id: 1, tsMs: 1, direction: "rx", text: "line" }],
      },
    });

    await expect(p).resolves.toEqual({
      totalLines: 123,
      start: 100,
      items: [{ id: 1, tsMs: 1, direction: "rx", text: "line" }],
    });
  });
});

describe("singleton export", () => {
  it("exports a ready-to-use wsTransport instance", () => {
    expect(wsTransport).toBeInstanceOf(WsTransport);
  });
});
