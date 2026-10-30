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
  t: (key: string, params?: Record<string, unknown>): string =>
    key === "serialDebug.conn.portsFailed"
      ? String(params?.msg)
      : ((
          {
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
          } as Record<string, string>
        )[key] ?? key),
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
  it("restores the actual custom baud and blocks invalid input without opening", async () => {
    const s = useSerialDebugStore();
    s.port = "COM3";
    s.customBaudRate = 123456;
    const open = vi.spyOn(s, "openPort").mockResolvedValue(undefined);
    mountComponent();
    await flush();
    const input = host!.querySelector<HTMLInputElement>('input[type="number"]');
    expect(input?.value).toBe("123456");
    for (const value of ["", "0", "-1", "1.5"]) {
      input!.value = value;
      input!.dispatchEvent(new Event("input", { bubbles: true }));
      await flush();
      const button = [...host!.querySelectorAll("button")].find(
        (b) => b.textContent?.trim() === "Open",
      )!;
      expect(button.disabled).toBe(true);
      expect(input!.getAttribute("aria-invalid")).toBe("true");
      button.click();
    }
    expect(open).not.toHaveBeenCalled();
  });
  it("shows enumeration errors and keeps a remembered missing port unavailable", async () => {
    const s = useSerialDebugStore();
    s.port = "COM99";
    vi.mocked(wsTransport.listPorts).mockRejectedValue(
      new Error("Enumeration refused"),
    );
    mountComponent();
    await flush();
    expect(host!.textContent).toContain("Enumeration refused");
    expect(
      [...host!.querySelectorAll("button")].find(
        (b) => b.textContent?.trim() === "Open",
      )!.disabled,
    ).toBe(true);
  });
  it("closes the reboot dialog with Escape and restores focus to Change", async () => {
    mountComponent();
    await flush();
    const change = [...host!.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Change"),
    )!;
    change.focus();
    change.click();
    await flush();
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog.contains(document.activeElement)).toBe(true);
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await flush();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(change);
  });
});
