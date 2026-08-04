import { ref } from "vue";
import type { ComputedRef, Ref } from "vue";
import { i18n } from "@/i18n";
import { isTauriRuntime } from "@/runtime";
import { rLog } from "@/utils/log";
import { wsTransport } from "@/transport/ws-transport";
import { usePortManagerStore } from "@/stores/port-manager";
import { rustPluginIdForChip } from "@/features/firmware-flash/chip-manifests";
import { SERIAL_PORT_OPTIONS } from "@/features/firmware-flash/constants";
import {
  formatSerialPortLabel,
  tuyaDualSerialHoverTooltip,
  type SerialPortDropdownOption,
  type TauriSerialPortRow,
} from "@/utils/serial-port-label";

const t = i18n.global.t;
const locale = i18n.global.locale;

export interface FlashConnectionDeps {
  selectedSerialPort: Ref<string>;
  selectedChipId: Ref<string>;
  connected: Ref<boolean>;
  autoConnected: Ref<boolean>;
  busy: ComputedRef<boolean>;
  appendLog: (line: string) => void;
  /** Cancel + reset a running operation (progress state lives in the store). */
  onCancelRunningOperation: () => void;
}

/** Serial-port scan + connect/disconnect/device-reset lifecycle. Owns
 *  serialPortOptions; cross-cutting state is injected via deps. */
export function useFlashConnection(deps: FlashConnectionDeps) {
  const serialPortOptions = ref<SerialPortDropdownOption[]>(
    SERIAL_PORT_OPTIONS.map((p) => ({ ...p })),
  );

  let lastScanFingerprint: string | undefined = undefined;

  async function refreshDevice(): Promise<void> {
    if (isTauriRuntime()) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const ports = await invoke<TauriSerialPortRow[]>(
          "list_serial_ports_cmd",
        );
        serialPortOptions.value = ports.map((p) => {
          const tip = tuyaDualSerialHoverTooltip(
            p.usbVid,
            p.usbPid,
            p.usbInterface,
            t,
          );
          const row: SerialPortDropdownOption = {
            value: p.path,
            label: formatSerialPortLabel(p, t),
          };
          if (tip) {
            row.optionTooltip = tip;
          }
          return row;
        });

        // Fingerprint includes role/interface so label updates when metadata changes
        const fingerprint = ports
          .map((p) => `${p.path}:${p.portRole ?? ""}:${p.usbInterface ?? ""}`)
          .join(",");

        if (ports.length > 0) {
          const exists = ports.some(
            (p) => p.path === deps.selectedSerialPort.value,
          );
          if (!exists) {
            deps.selectedSerialPort.value = ports[0].path;
          }
          // Only log when the port list actually changed
          if (fingerprint !== lastScanFingerprint) {
            deps.appendLog(
              t("flash.log.portsFound", {
                list: serialPortOptions.value
                  .map((x: SerialPortDropdownOption) => x.label)
                  .join(locale.value === "zh-CN" ? "、" : ", "),
              }),
            );
          }
        } else {
          deps.selectedSerialPort.value = "";
          deps.connected.value = false;
          if (fingerprint !== lastScanFingerprint) {
            deps.appendLog(t("flash.log.noPortsFound"));
          }
        }
        lastScanFingerprint = fingerprint;
      } catch {
        deps.appendLog(t("flash.log.portsListFailed"));
        serialPortOptions.value = [];
        deps.selectedSerialPort.value = "";
        deps.connected.value = false;
        lastScanFingerprint = undefined;
      }
    } else {
      // Web mode: ask the local serve process for ports
      try {
        const ports = await wsTransport.listPorts();
        serialPortOptions.value = ports.map((p) => {
          const tip = tuyaDualSerialHoverTooltip(
            p.usbVid,
            p.usbPid,
            p.usbInterface,
            t,
          );
          const row: SerialPortDropdownOption = {
            value: p.path,
            label: formatSerialPortLabel(p, t),
          };
          if (tip) {
            row.optionTooltip = tip;
          }
          return row;
        });
        const fingerprint = ports
          .map((p) => `${p.path}:${p.portRole ?? ""}:${p.usbInterface ?? ""}`)
          .join(",");
        if (ports.length > 0) {
          const exists = ports.some(
            (p) => p.path === deps.selectedSerialPort.value,
          );
          if (!exists) deps.selectedSerialPort.value = ports[0].path;
          if (fingerprint !== lastScanFingerprint) {
            deps.appendLog(
              t("flash.log.portsFound", {
                list: serialPortOptions.value
                  .map((x: SerialPortDropdownOption) => x.label)
                  .join(locale.value === "zh-CN" ? "、" : ", "),
              }),
            );
          }
        } else {
          deps.selectedSerialPort.value = "";
          deps.connected.value = false;
          if (fingerprint !== lastScanFingerprint) {
            deps.appendLog(t("flash.log.noPortsFound"));
            deps.appendLog(t("flash.log.noPortsFoundWebServeHint"));
          }
        }
        lastScanFingerprint = fingerprint;
      } catch {
        deps.appendLog(t("flash.log.wsConnectFailed"));
        serialPortOptions.value = [];
        deps.selectedSerialPort.value = "";
        deps.connected.value = false;
        lastScanFingerprint = undefined;
      }
    }
  }

  async function connect(): Promise<void> {
    const port = deps.selectedSerialPort.value;
    if (!port) {
      deps.appendLog(t("flash.log.noPortsFound"));
      return;
    }
    const pm = usePortManagerStore();
    const currentOwner = pm.currentOwner(port);
    const preemptingInAppOwner =
      currentOwner !== null && currentOwner !== "flash";

    if (isTauriRuntime() && !preemptingInAppOwner) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<{
          available: boolean;
          errorMessage?: string | null;
          processInfo?: string | null;
          killHint?: string | null;
        }>("check_port_available_cmd", { port });

        if (!result.available) {
          deps.appendLog(t("flash.log.portBusy", { port }));
          if (result.errorMessage)
            deps.appendLog(
              t("flash.log.portBusyDetail", { msg: result.errorMessage }),
            );
          if (result.processInfo)
            deps.appendLog(
              t("flash.log.portBusyProcess", { info: result.processInfo }),
            );
          if (result.killHint)
            deps.appendLog(
              t("flash.log.portBusyKillHint", { cmd: result.killHint }),
            );
          return;
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        deps.appendLog(t("flash.log.portCheckFailed", { msg }));
        // Continue — let the real operation fail if there's really a problem
      }
    }
    const outcome = await pm.acquire({
      id: "flash",
      port,
      onReleaseRequest: async () => false, // flash never yields mid-operation
      onReleased: () => {
        deps.connected.value = false;
        deps.autoConnected.value = false;
      },
    });
    if (outcome === "denied") {
      deps.appendLog(t("flash.log.portBusy", { port }));
      return;
    }
    deps.connected.value = true;
    deps.autoConnected.value = false;
    deps.appendLog(t("flash.log.connected"));
    rLog.info(`[Flash] Connected to port: ${port}`);
  }

  function disconnect(): void {
    if (deps.busy.value) {
      // Cancel the running operation before disconnecting
      deps.onCancelRunningOperation();
    }
    rLog.info(
      `[Flash] Disconnected from port: ${deps.selectedSerialPort.value}`,
    );
    deps.connected.value = false;
    deps.autoConnected.value = false;
    deps.appendLog(t("flash.log.disconnected"));
    usePortManagerStore().release(deps.selectedSerialPort.value, "flash");
  }

  async function deviceReset(): Promise<void> {
    if (!deps.selectedSerialPort.value.trim() || deps.busy.value) {
      return;
    }
    const chipId = rustPluginIdForChip(deps.selectedChipId.value);
    try {
      if (isTauriRuntime()) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("device_reset_cmd", {
          args: {
            port: deps.selectedSerialPort.value,
            chipId,
          },
        });
      } else {
        await wsTransport.deviceReset(deps.selectedSerialPort.value, chipId);
      }
      deps.appendLog(
        t("flash.log.deviceResetOk", { port: deps.selectedSerialPort.value }),
      );
      rLog.info(
        `[Flash] Device reset (DTR/RTS) on ${deps.selectedSerialPort.value}`,
      );
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      if (raw.includes("unknown variant") && raw.includes("device_reset")) {
        deps.appendLog(t("flash.log.deviceResetServeOutdated"));
      } else {
        deps.appendLog(t("flash.log.deviceResetFailed", { msg: raw }));
      }
      rLog.warn(`[Flash] Device reset failed: ${raw}`);
    }
  }

  return { serialPortOptions, refreshDevice, connect, disconnect, deviceReset };
}
