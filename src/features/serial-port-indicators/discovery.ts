import { isTauriRuntime } from "@/runtime";
import { wsTransport } from "@/transport/ws-transport";
import type { TauriSerialPortRow } from "@/utils/serial-port-label";

// Explicit scans and page initialization reuse a recent successful device-event scan.
export const DEVICE_EVENT_REFRESH_WINDOW_MS = 1000;

type Unlisten = () => void;
type DiscoveryDeps = {
  enumerate: () => Promise<TauriSerialPortRow[]>;
  listen: (changed: () => void) => Promise<Unlisten>;
};

export function createSerialPortDiscovery(
  deps: DiscoveryDeps,
  refreshWindowMs = DEVICE_EVENT_REFRESH_WINDOW_MS,
) {
  const subscribers = new Set<() => void>();
  let listener: Promise<Unlisten> | undefined;
  let generation = 0;
  let eventGeneration = -1;
  let cached: TauriSerialPortRow[] | undefined;
  let completedAt = -Infinity;
  let pending: Promise<TauriSerialPortRow[]> | undefined;

  function invalidate() {
    generation++;
    eventGeneration = -1;
    cached = undefined;
  }

  function changed() {
    invalidate();
    eventGeneration = generation;
    for (const callback of subscribers) callback();
  }

  function listPorts(): Promise<TauriSerialPortRow[]> {
    if (pending) return pending;
    if (
      cached &&
      eventGeneration === generation &&
      performance.now() - completedAt < refreshWindowMs
    ) {
      return Promise.resolve(cached);
    }
    // A later explicit scan does not extend the device-event reuse window.
    if (cached) eventGeneration = -1;
    cached = undefined;
    pending = (async () => {
      // An event during enumeration invalidates that snapshot. Finish with a
      // scan started after the latest event, even if a caller was already busy.
      for (;;) {
        const startedGeneration = generation;
        let rows: TauriSerialPortRow[];
        try {
          rows = await deps.enumerate();
        } catch (error) {
          if (startedGeneration !== generation) continue;
          throw error;
        }
        if (startedGeneration !== generation) continue;
        cached = rows;
        completedAt = performance.now();
        return rows;
      }
    })().finally(() => {
      pending = undefined;
    });
    return pending;
  }

  function subscribe(callback: () => void): Unlisten {
    subscribers.add(callback);
    if (!listener) {
      listener = deps.listen(changed).catch((error: unknown) => {
        // Explicit enumeration remains available if notification setup fails.
        console.warn("Serial device notifications unavailable", error);
        return () => {};
      });
    }
    return () => {
      subscribers.delete(callback);
      if (!subscribers.size && listener) {
        void listener.then((stop) => stop());
        listener = undefined;
      }
    };
  }

  return { listPorts, subscribe, invalidate };
}

export const serialPortDiscovery = createSerialPortDiscovery({
  async enumerate() {
    if (!isTauriRuntime()) return wsTransport.listPorts();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<TauriSerialPortRow[]>("list_serial_ports_cmd");
  },
  async listen(changed) {
    if (!isTauriRuntime()) return wsTransport.onSerialPortsChanged(changed);
    const { listen } = await import("@tauri-apps/api/event");
    return listen("serial-ports-changed", changed);
  },
});
