// @vitest-environment happy-dom
import { createPinia, setActivePinia } from "pinia";
import {
  createApp,
  defineComponent,
  h,
  nextTick,
  type ComponentPublicInstance,
} from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { t } = vi.hoisted(() => ({
  t: (key: string): string =>
    (
      ({
        "serialDebug.pageTitle": "Serial debug",
        "serialDebug.conn.port": "Port",
        "serialDebug.conn.baud": "Baud",
        "serialDebug.conn.settings": "Settings",
        "serialDebug.conn.deviceReset": "Reboot device",
        "serialDebug.conn.deviceResetHint": "Reboot device",
        "serialDebug.conn.statusDisconnected": "Disconnected",
        "serialDebug.conn.connecting": "Connecting",
        "serialDebug.conn.statusConnected": "Connected",
        "serialDebug.conn.open": "Open",
        "serialDebug.conn.close": "Close",
        "serialDebug.conn.customBaud": "Custom",
        "serialDebug.conn.rebootControlPort": "Control port",
        "serialDebug.conn.rebootControlPortUnset": "Not selected",
        "serialDebug.conn.changeRebootTarget": "Change",
        "serialDebug.conn.rebootTargetDialogTitle":
          "Choose reboot control port and chip",
        "serialDebug.conn.rebootTargetChip": "Chip",
        "serialDebug.conn.rebootTargetConfirm": "Confirm",
        "common.closeDialog": "Close",
        "flash.noPortsPlaceholder": "No ports",
        "flash.chips.t5ai": "T5AI",
        "flash.chips.esp32": "ESP32",
      }) as Record<string, string>
    )[key] ?? key,
}));

vi.mock("vue-i18n", () => ({
  useI18n: () => ({ t }),
  createI18n: () => ({
    global: {
      t,
      te: () => true,
      locale: { value: "en" },
    },
  }),
}));

vi.mock("@/runtime", () => ({
  isTauriRuntime: () => false,
  getRuntime: () => "web",
}));

import { wsTransport } from "@/transport/ws-transport";
import { useFlashStore } from "@/stores/flash";
import { useSerialDebugStore } from "@/stores/serial-debug";
import SerialDebugConnectionBar from "./SerialDebugConnectionBar.vue";

async function flush(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
}

describe("SerialDebugConnectionBar reboot target UI", () => {
  let app: ReturnType<typeof createApp> | null = null;
  let host: HTMLDivElement | null = null;

  beforeEach(() => {
    setActivePinia(createPinia());
    const flash = useFlashStore();
    flash.selectedChipId = "t5ai";
    flash.selectedSerialPort = "";
    host = document.createElement("div");
    document.body.appendChild(host);
    vi.spyOn(wsTransport, "listPorts").mockResolvedValue([
      { path: "COM3", name: "Flash", portRole: "flash_auth" },
      { path: "COM4", name: "Logs", portRole: "log" },
    ]);
  });

  afterEach(() => {
    app?.unmount();
    host?.remove();
    app = null;
    host = null;
    vi.restoreAllMocks();
  });

  function mountComponent(): ComponentPublicInstance {
    app = createApp(
      defineComponent({
        setup() {
          return () => h(SerialDebugConnectionBar);
        },
      }),
    );
    app.component(
      "FontAwesomeIcon",
      defineComponent({
        setup() {
          return () => h("i");
        },
      }),
    );
    return app.mount(host!);
  }

  it("shows the currently resolved reboot control port next to the reboot action", async () => {
    const s = useSerialDebugStore();
    s.rememberRebootTarget("COM3", "t5ai");

    mountComponent();
    await flush();

    expect(host?.textContent).toContain("Control port");
    expect(host?.textContent).toContain("COM3");
  });

  it("reopens the reboot target dialog when the user clicks Change", async () => {
    const s = useSerialDebugStore();
    s.rememberRebootTarget("COM3", "t5ai");

    mountComponent();
    await flush();

    const changeButton = Array.from(host!.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("Change"),
    );
    changeButton?.click();
    await flush();

    expect(document.body.textContent).toContain(
      "Choose reboot control port and chip",
    );
  });

  it("opens the reboot target dialog on the first click when no reboot target is configured", async () => {
    const s = useSerialDebugStore();
    const flash = useFlashStore();
    flash.selectedSerialPort = "";
    flash.selectedChipId = "t5ai";
    const resetSpy = vi.spyOn(s, "deviceReset").mockResolvedValue(undefined);

    mountComponent();
    await flush();

    const rebootButton = host!.querySelector(
      'button[aria-label="Reboot device"]',
    ) as HTMLButtonElement | null;
    rebootButton?.click();
    await flush();

    expect(resetSpy).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain(
      "Choose reboot control port and chip",
    );
  });
});
