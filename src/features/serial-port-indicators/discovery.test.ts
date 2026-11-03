// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createSerialPortDiscovery } from "./discovery";

function setup(windowMs = 1000) {
  let changed!: () => void;
  const enumerate = vi.fn(async () => [{ path: "COM3" }]);
  const stop = vi.fn();
  const listen = vi.fn(async (callback: () => void) => {
    changed = callback;
    return stop;
  });
  const discovery = createSerialPortDiscovery({ enumerate, listen }, windowMs);
  const callback = vi.fn(() => {
    void discovery.listPorts().catch(() => {});
  });
  const unsubscribe = discovery.subscribe(callback);
  return {
    ...discovery,
    enumerate,
    stop,
    listen,
    callback,
    unsubscribe,
    changed: () => changed(),
  };
}
afterEach(() => vi.restoreAllMocks());

it("refreshes on events and reuses the result for a configurable window", async () => {
  let now = 100;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  const d = setup(500);
  await d.listPorts();
  await d.listPorts();
  expect(d.enumerate).toHaveBeenCalledTimes(2);
  d.changed();
  await d.listPorts();
  expect(d.callback).toHaveBeenCalledOnce();
  now += 499;
  await d.listPorts();
  expect(d.enumerate).toHaveBeenCalledTimes(3);
  now += 1;
  await d.listPorts();
  expect(d.enumerate).toHaveBeenCalledTimes(4);
  await d.listPorts();
  expect(d.enumerate).toHaveBeenCalledTimes(5);
});

it("invalidates a fresh event result on another device event", async () => {
  const d = setup();
  d.changed();
  await d.listPorts();
  d.enumerate.mockResolvedValueOnce([{ path: "COM4" }]);
  d.changed();
  expect(await d.listPorts()).toEqual([{ path: "COM4" }]);
  expect(d.enumerate).toHaveBeenCalledTimes(2);
});

it("coalesces consumers and rescans if an event arrives during enumeration", async () => {
  const d = setup();
  let finish!: (rows: { path: string }[]) => void;
  d.enumerate.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const first = d.listPorts();
  const second = d.listPorts();
  expect(first).toBe(second);
  d.changed();
  d.enumerate.mockResolvedValueOnce([{ path: "COM4" }]);
  finish([{ path: "COM3" }]);
  expect(await first).toEqual([{ path: "COM4" }]);
  expect(await second).toEqual([{ path: "COM4" }]);
  expect(d.enumerate).toHaveBeenCalledTimes(2);
});

it("does not cache a failed event scan and allows an immediate retry", async () => {
  const d = setup();
  d.enumerate.mockRejectedValueOnce(new Error("scan failed"));
  d.changed();
  await expect(d.listPorts()).rejects.toThrow("scan failed");
  expect(await d.listPorts()).toEqual([{ path: "COM3" }]);
  expect(d.enumerate).toHaveBeenCalledTimes(2);
});

it("shares the native subscription and releases it when the last consumer leaves", async () => {
  const d = setup();
  const unsubscribeSecond = d.subscribe(() => {});
  expect(d.listen).toHaveBeenCalledOnce();
  d.unsubscribe();
  await Promise.resolve();
  expect(d.stop).not.toHaveBeenCalled();
  unsubscribeSecond();
  await Promise.resolve();
  expect(d.stop).toHaveBeenCalledOnce();
});

it("rescans after a device event even when the older pending request fails", async () => {
  const d = setup();
  let reject!: (error: Error) => void;
  d.enumerate.mockImplementationOnce(
    () =>
      new Promise((_, fail) => {
        reject = fail;
      }),
  );
  const first = d.listPorts();
  d.changed();
  reject(new Error("device changed during scan"));
  expect(await first).toEqual([{ path: "COM3" }]);
  expect(d.enumerate).toHaveBeenCalledTimes(2);
});
