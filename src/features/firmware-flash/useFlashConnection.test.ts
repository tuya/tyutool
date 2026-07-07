// @vitest-environment happy-dom
import { beforeEach, describe, it, expect, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { computed, ref } from "vue";
import type { ComputedRef } from "vue";

// ── Hoisted mock fns (must exist when hoisted vi.mock factories run) ──
const { invoke, listPorts, deviceReset } = vi.hoisted(() => ({
  invoke: vi.fn(async () => undefined as unknown),
  listPorts: vi.fn(async () => [] as unknown[]),
  deviceReset: vi.fn(async () => undefined),
}));

// isTauriRuntime is toggled per-test via vi.mocked(...).mockReturnValue(...)
vi.mock("@/runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime")>()),
  isTauriRuntime: vi.fn(() => true),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@/transport/ws-transport", () => ({
  wsTransport: { listPorts, deviceReset },
}));

import { isTauriRuntime } from "@/runtime";
import { usePortManagerStore } from "@/stores/port-manager";
import { useFlashConnection } from "./useFlashConnection";
import type { FlashConnectionDeps } from "./useFlashConnection";

function makeDeps(overrides: Partial<FlashConnectionDeps> = {}) {
  const selectedSerialPort = ref("");
  const selectedChipId = ref("t5ai");
  const connected = ref(false);
  const autoConnected = ref(false);
  const busyRef = ref(false);
  const busy = computed(() => busyRef.value) as ComputedRef<boolean>;
  const appendLog = vi.fn();
  const onCancelRunningOperation = vi.fn();
  const deps: FlashConnectionDeps = {
    selectedSerialPort,
    selectedChipId,
    connected,
    autoConnected,
    busy,
    appendLog,
    onCancelRunningOperation,
    ...overrides,
  };
  return {
    deps,
    selectedSerialPort,
    selectedChipId,
    connected,
    autoConnected,
    busyRef,
    appendLog,
    onCancelRunningOperation,
  };
}

describe("useFlashConnection", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    vi.mocked(isTauriRuntime).mockReturnValue(true);
    invoke.mockResolvedValue(undefined);
    listPorts.mockResolvedValue([]);
    deviceReset.mockResolvedValue(undefined);
  });

  // ── refreshDevice (Tauri) ──────────────────────────────────────

  it("Tauri refreshDevice lists ports and auto-selects the first when none chosen", async () => {
    invoke.mockResolvedValue([
      { path: "/dev/ttyUSB0" },
      { path: "/dev/ttyUSB1" },
    ]);
    const { deps, selectedSerialPort } = makeDeps();
    const c = useFlashConnection(deps);
    await c.refreshDevice();
    expect(invoke).toHaveBeenCalledWith("list_serial_ports_cmd");
    expect(c.serialPortOptions.value.map((o) => o.value)).toEqual([
      "/dev/ttyUSB0",
      "/dev/ttyUSB1",
    ]);
    expect(selectedSerialPort.value).toBe("/dev/ttyUSB0");
  });

  it("Tauri refreshDevice keeps a still-present selection", async () => {
    invoke.mockResolvedValue([
      { path: "/dev/ttyUSB0" },
      { path: "/dev/ttyUSB1" },
    ]);
    const { deps, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB1";
    const c = useFlashConnection(deps);
    await c.refreshDevice();
    expect(selectedSerialPort.value).toBe("/dev/ttyUSB1");
  });

  it("Tauri refreshDevice with no ports clears selection and disconnects", async () => {
    invoke.mockResolvedValue([]);
    const { deps, selectedSerialPort, connected } = makeDeps();
    connected.value = true;
    selectedSerialPort.value = "/dev/ttyUSB0";
    const c = useFlashConnection(deps);
    await c.refreshDevice();
    expect(selectedSerialPort.value).toBe("");
    expect(connected.value).toBe(false);
  });

  it("Tauri refreshDevice swallows invoke failure, clears options and logs", async () => {
    invoke.mockRejectedValue(new Error("enumeration failed"));
    const { deps, appendLog, selectedSerialPort, connected } = makeDeps();
    connected.value = true;
    const c = useFlashConnection(deps);
    await c.refreshDevice();
    expect(c.serialPortOptions.value).toEqual([]);
    expect(selectedSerialPort.value).toBe("");
    expect(connected.value).toBe(false);
    expect(appendLog).toHaveBeenCalled();
  });

  it("Tauri refreshDevice only logs the port list once when the fingerprint is unchanged", async () => {
    invoke.mockResolvedValue([{ path: "/dev/ttyUSB0" }]);
    const { deps, appendLog } = makeDeps();
    const c = useFlashConnection(deps);
    await c.refreshDevice();
    const callsAfterFirst = appendLog.mock.calls.length;
    await c.refreshDevice();
    expect(appendLog.mock.calls.length).toBe(callsAfterFirst);
  });

  // ── refreshDevice (web) ────────────────────────────────────────

  it("web refreshDevice uses wsTransport.listPorts", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(false);
    listPorts.mockResolvedValue([{ path: "/dev/ttyWEB0" }]);
    const { deps, selectedSerialPort } = makeDeps();
    const c = useFlashConnection(deps);
    await c.refreshDevice();
    expect(listPorts).toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
    expect(selectedSerialPort.value).toBe("/dev/ttyWEB0");
  });

  it("web refreshDevice failure logs the WS hint and clears state", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(false);
    listPorts.mockRejectedValue(new Error("no serve"));
    const { deps, appendLog, selectedSerialPort } = makeDeps();
    const c = useFlashConnection(deps);
    await c.refreshDevice();
    expect(c.serialPortOptions.value).toEqual([]);
    expect(selectedSerialPort.value).toBe("");
    expect(appendLog).toHaveBeenCalledWith(
      expect.stringContaining("ws://127.0.0.1:9527"),
    );
  });

  // ── connect ────────────────────────────────────────────────────

  it("connect with no port selected logs and does not connect", async () => {
    const { deps, connected, appendLog } = makeDeps();
    const c = useFlashConnection(deps);
    await c.connect();
    expect(connected.value).toBe(false);
    expect(appendLog).toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("connect (Tauri) checks port availability, connects, and claims the port", async () => {
    invoke.mockResolvedValue({ available: true });
    const { deps, connected, autoConnected, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    const c = useFlashConnection(deps);
    await c.connect();
    expect(invoke).toHaveBeenCalledWith("check_port_available_cmd", {
      port: "/dev/ttyUSB0",
    });
    expect(connected.value).toBe(true);
    expect(autoConnected.value).toBe(false);
    const pm = usePortManagerStore();
    expect(pm.currentOwner("/dev/ttyUSB0")).toBe("flash");
  });

  it("connect (Tauri) aborts when the port is reported busy", async () => {
    invoke.mockResolvedValue({
      available: false,
      errorMessage: "in use",
      processInfo: "pid 42",
      killHint: "kill 42",
    });
    const { deps, connected, selectedSerialPort, appendLog } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    const c = useFlashConnection(deps);
    await c.connect();
    expect(connected.value).toBe(false);
    expect(appendLog).toHaveBeenCalled();
    const pm = usePortManagerStore();
    expect(pm.currentOwner("/dev/ttyUSB0")).toBeNull();
  });

  it("connect (Tauri) continues even if the availability check throws", async () => {
    invoke.mockRejectedValue(new Error("check exploded"));
    const { deps, connected, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    const c = useFlashConnection(deps);
    await c.connect();
    // Falls through to the optimistic connect path
    expect(connected.value).toBe(true);
  });

  it("connect denied by port-manager rolls back connected", async () => {
    invoke.mockResolvedValue({ available: true });
    const { deps, connected, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    // Pre-claim the port with a different owner that refuses to release
    const pm = usePortManagerStore();
    await pm.acquire({
      id: "serial-debug",
      port: "/dev/ttyUSB0",
      onReleaseRequest: async () => false,
      onReleased: () => {},
    });
    const c = useFlashConnection(deps);
    await c.connect();
    expect(connected.value).toBe(false);
    expect(pm.currentOwner("/dev/ttyUSB0")).toBe("serial-debug");
  });

  it("connect can preempt an in-app serial-debug owner before running the Tauri availability check", async () => {
    invoke.mockResolvedValue({
      available: false,
      errorMessage: "still closing",
    });
    const { deps, connected, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    const onReleased = vi.fn();
    const pm = usePortManagerStore();
    await pm.acquire({
      id: "serial-debug",
      port: "/dev/ttyUSB0",
      onReleaseRequest: async () => true,
      onReleased,
    });

    const c = useFlashConnection(deps);
    await c.connect();

    expect(connected.value).toBe(true);
    expect(onReleased).toHaveBeenCalledWith("requested");
    expect(pm.currentOwner("/dev/ttyUSB0")).toBe("flash");
    expect(invoke).not.toHaveBeenCalledWith("check_port_available_cmd", {
      port: "/dev/ttyUSB0",
    });
  });

  it("connect (web) skips the availability invoke and claims the port", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(false);
    const { deps, connected, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyWEB0";
    const c = useFlashConnection(deps);
    await c.connect();
    expect(invoke).not.toHaveBeenCalled();
    expect(connected.value).toBe(true);
    const pm = usePortManagerStore();
    expect(pm.currentOwner("/dev/ttyWEB0")).toBe("flash");
  });

  // ── disconnect ─────────────────────────────────────────────────

  it("disconnect releases the port and resets connection flags", async () => {
    invoke.mockResolvedValue({ available: true });
    const { deps, connected, autoConnected, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    const c = useFlashConnection(deps);
    await c.connect();
    const pm = usePortManagerStore();
    expect(pm.currentOwner("/dev/ttyUSB0")).toBe("flash");

    c.disconnect();
    expect(connected.value).toBe(false);
    expect(autoConnected.value).toBe(false);
    expect(pm.currentOwner("/dev/ttyUSB0")).toBeNull();
  });

  it("disconnect while busy cancels the running operation first", () => {
    const { deps, busyRef, onCancelRunningOperation, selectedSerialPort } =
      makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    busyRef.value = true;
    const c = useFlashConnection(deps);
    c.disconnect();
    expect(onCancelRunningOperation).toHaveBeenCalledOnce();
  });

  // ── deviceReset ────────────────────────────────────────────────

  it("deviceReset (Tauri) invokes device_reset_cmd with mapped chip id", async () => {
    const { deps, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    const c = useFlashConnection(deps);
    await c.deviceReset();
    expect(invoke).toHaveBeenCalledWith("device_reset_cmd", {
      args: { port: "/dev/ttyUSB0", chipId: expect.any(String) },
    });
  });

  it("deviceReset (web) calls wsTransport.deviceReset", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(false);
    const { deps, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyWEB0";
    const c = useFlashConnection(deps);
    await c.deviceReset();
    expect(deviceReset).toHaveBeenCalledWith(
      "/dev/ttyWEB0",
      expect.any(String),
    );
  });

  it("deviceReset is a no-op when no port is selected", async () => {
    const { deps } = makeDeps();
    const c = useFlashConnection(deps);
    await c.deviceReset();
    expect(invoke).not.toHaveBeenCalled();
    expect(deviceReset).not.toHaveBeenCalled();
  });

  it("deviceReset is a no-op while busy", async () => {
    const { deps, busyRef, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    busyRef.value = true;
    const c = useFlashConnection(deps);
    await c.deviceReset();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("deviceReset failure is logged without throwing", async () => {
    invoke.mockRejectedValue(new Error("reset failed"));
    const { deps, appendLog, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    const c = useFlashConnection(deps);
    await expect(c.deviceReset()).resolves.toBeUndefined();
    expect(appendLog).toHaveBeenCalled();
  });

  it("deviceReset maps the outdated-serve unknown-variant error to a hint", async () => {
    invoke.mockRejectedValue(
      new Error("unknown variant `device_reset`, expected ..."),
    );
    const { deps, appendLog, selectedSerialPort } = makeDeps();
    selectedSerialPort.value = "/dev/ttyUSB0";
    const c = useFlashConnection(deps);
    await c.deviceReset();
    expect(appendLog).toHaveBeenCalled();
  });
});
