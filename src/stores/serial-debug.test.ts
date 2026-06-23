// @vitest-environment happy-dom
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { wsTransport } from "@/transport/ws-transport";
import {
  __setSerialDebugTransportForTest,
  type SerialDebugTransport,
} from "@/features/serial-debug/transport";
import type { DebugChunk } from "@/features/serial-debug/types";
import { useSerialDebugStore } from "./serial-debug";
import { useFlashStore } from "./flash";
import { nextTick } from "vue";

function fakeTransport(): SerialDebugTransport & {
  emitChunk: (c: DebugChunk) => void;
  emitDisconnect: (reason: string) => void;
  sent: Uint8Array[];
  opened: boolean;
} {
  const chunkListeners = new Set<(c: DebugChunk) => void>();
  const discListeners = new Set<(p: { reason: string }) => void>();
  const sent: Uint8Array[] = [];
  let opened = false;
  return {
    sent,
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
    onChunk(cb) {
      chunkListeners.add(cb);
      return () => chunkListeners.delete(cb);
    },
    onDisconnect(cb) {
      discListeners.add(cb);
      return () => discListeners.delete(cb);
    },
    emitChunk(c) {
      chunkListeners.forEach((l) => l(c));
    },
    emitDisconnect(reason) {
      discListeners.forEach((l) => l({ reason }));
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

  it("drops oldest entries past MAX_LOG_LINES", () => {
    const s = useSerialDebugStore();
    // Simulate 20005 lines arriving in one batch (each terminated by \n).
    const oneLine = "x\n";
    const bytes = [...Buffer.from(oneLine.repeat(20005))];
    s.appendChunk({ direction: "rx", tsMs: 1000, bytes });
    expect(s.lines.length).toBe(20000);
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

  it("clear() empties lines and pending buffer", () => {
    const s = useSerialDebugStore();
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("ab\ncd")],
    });
    expect(s.lines.length).toBe(1);
    s.clear();
    expect(s.lines.length).toBe(0);
    // new input after clear should start a fresh line
    s.appendChunk({
      direction: "rx",
      tsMs: 2000,
      bytes: [...Buffer.from("xy\n")],
    });
    expect(s.lines[0].text).toBe("xy");
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

  it("addChip returns ok, adds chip, and sets it as active tab", () => {
    const s = useSerialDebugStore();
    const result = s.addChip("ERROR", false);
    expect(result).toBe("ok");
    expect(s.watchChips.length).toBe(1);
    expect(s.watchChips[0]).toMatchObject({
      keyword: "ERROR",
      useRegex: false,
    });
    expect(s.activeChipId).toBe(s.watchChips[0].id);
  });

  it("addChip returns duplicate when same keyword added twice", () => {
    const s = useSerialDebugStore();
    expect(s.addChip("WIFI", false)).toBe("ok");
    expect(s.addChip("WIFI", false)).toBe("duplicate");
    expect(s.watchChips.length).toBe(1);
  });

  it("addChip returns invalid-regex for bad pattern", () => {
    const s = useSerialDebugStore();
    expect(s.addChip("[invalid", true)).toBe("invalid-regex");
    expect(s.watchChips.length).toBe(0);
  });

  it("removeChip removes by id and falls back activeChipId", () => {
    const s = useSerialDebugStore();
    s.addChip("FIRST", false);
    s.addChip("SECOND", false);
    const firstId = s.watchChips[0].id;
    const secondId = s.watchChips[1].id;
    s.setActiveChip(secondId);
    s.removeChip(secondId);
    expect(s.watchChips.length).toBe(1);
    expect(s.watchChips[0].keyword).toBe("FIRST");
    expect(s.activeChipId).toBe(firstId);
  });

  it("setActiveChip switches active tab (null = All)", () => {
    const s = useSerialDebugStore();
    s.addChip("LOG", false);
    const id = s.watchChips[0].id;
    expect(s.activeChipId).toBe(id);
    s.setActiveChip(null);
    expect(s.activeChipId).toBeNull();
    s.setActiveChip(id);
    expect(s.activeChipId).toBe(id);
  });

  it("matchChipKeyword matches plain text substring (case-sensitive)", () => {
    const s = useSerialDebugStore();
    s.addChip("ERROR", false);
    const chip = s.watchChips[0];
    expect(
      s.matchChipKeyword(
        { id: 1, tsMs: 0, direction: "rx", text: "ERROR: timeout" },
        chip,
      ),
    ).toBe(true);
    expect(
      s.matchChipKeyword(
        { id: 2, tsMs: 0, direction: "rx", text: "error: timeout" },
        chip,
      ),
    ).toBe(false);
    expect(
      s.matchChipKeyword(
        { id: 3, tsMs: 0, direction: "rx", text: "no match" },
        chip,
      ),
    ).toBe(false);
  });

  it("matchChipKeyword matches regex pattern", () => {
    const s = useSerialDebugStore();
    s.addChip("err(or)?", true);
    const chip = s.watchChips[0];
    expect(
      s.matchChipKeyword(
        { id: 1, tsMs: 0, direction: "rx", text: "err: foo" },
        chip,
      ),
    ).toBe(true);
    expect(
      s.matchChipKeyword(
        { id: 2, tsMs: 0, direction: "rx", text: "error: bar" },
        chip,
      ),
    ).toBe(true);
    expect(
      s.matchChipKeyword({ id: 3, tsMs: 0, direction: "rx", text: "ok" }, chip),
    ).toBe(false);
  });

  it("clear does not affect watchChips", () => {
    const s = useSerialDebugStore();
    s.addChip("LOG", false);
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("LOG line\n")],
    });
    s.clear();
    expect(s.watchChips.length).toBe(1);
    expect(s.lines.length).toBe(0);
  });

  it("chips cycle colors from CHIP_COLORS when added", async () => {
    const { CHIP_COLORS } = await import("@/features/serial-debug/constants");
    const s = useSerialDebugStore();
    for (let i = 0; i < CHIP_COLORS.length + 1; i++) {
      s.addChip(`kw${i}`, false);
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
    const discListeners = new Set<(p: { reason: string }) => void>();
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
      onChunk(cb) {
        chunkListeners.add(cb);
        return () => chunkListeners.delete(cb);
      },
      onDisconnect(cb) {
        discListeners.add(cb);
        return () => discListeners.delete(cb);
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
