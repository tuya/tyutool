// @vitest-environment happy-dom
import { createPinia, setActivePinia } from "pinia";
import { createApp, defineComponent, h, nextTick } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { t } = vi.hoisted(() => ({
  t: (key: string, params?: { msg?: string }): string => {
    const messages: Record<string, string> = {
      "serialDebug.send.inputLabel": "Data to send",
      "serialDebug.send.sendBtn": "Send",
      "serialDebug.send.invalidHex": "Enter at least one complete Hex byte.",
      "serialDebug.send.failed": `Send failed: ${params?.msg}. Your input was kept. Try sending again.`,
    };
    return messages[key] ?? key;
  },
}));
vi.mock("vue-i18n", () => ({
  useI18n: () => ({ t }),
  createI18n: () => ({
    global: { t, te: () => true, locale: { value: "en" } },
  }),
}));
vi.mock("@/runtime", () => ({
  isTauriRuntime: () => false,
  getRuntime: () => "web",
}));

import { serialDebugTransport } from "@/features/serial-debug/transport";
import { useSerialDebugStore } from "@/stores/serial-debug";
import SerialDebugSendBar from "./SerialDebugSendBar.vue";

describe("SerialDebugSendBar recovery", () => {
  let app: ReturnType<typeof createApp>;
  let host: HTMLDivElement;
  let s: ReturnType<typeof useSerialDebugStore>;
  let write: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    setActivePinia(createPinia());
    const transport = serialDebugTransport();
    write = vi.spyOn(transport, "send").mockResolvedValue(undefined);
    vi.spyOn(transport, "appendSysLine").mockResolvedValue(null);
    s = useSerialDebugStore();
    s.open = true;
    host = document.createElement("div");
    document.body.appendChild(host);
    app = createApp(SerialDebugSendBar);
    app.component(
      "FontAwesomeIcon",
      defineComponent(() => () => h("i")),
    );
    app.mount(host);
  });

  afterEach(() => {
    app.unmount();
    host.remove();
    vi.restoreAllMocks();
  });

  function input(): HTMLInputElement {
    return host.querySelector('input[type="text"]')!;
  }

  function sendButton(): HTMLButtonElement {
    return Array.from(host.querySelectorAll("button")).find(
      (button) => button.textContent?.trim() === "Send",
    )!;
  }

  async function enterDraft(text: string): Promise<void> {
    input().value = text;
    input().dispatchEvent(new Event("input", { bubbles: true }));
    await nextTick();
  }

  async function flush(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await nextTick();
  }

  it("labels the field and announces the selected send mode", async () => {
    expect(input().getAttribute("aria-label")).toBe("Data to send");
    const [ascii, hex] = host.querySelectorAll(".mode-btn");
    expect(ascii.getAttribute("aria-pressed")).toBe("true");
    expect(hex.getAttribute("aria-pressed")).toBe("false");
    (hex as HTMLButtonElement).click();
    await nextTick();
    expect(ascii.getAttribute("aria-pressed")).toBe("false");
    expect(hex.getAttribute("aria-pressed")).toBe("true");
  });

  it("retains the draft when Enter cannot send on a closed port", async () => {
    await enterDraft("AT");
    s.open = false;
    await nextTick();
    input().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await flush();
    expect(write).not.toHaveBeenCalled();
    expect(input().value).toBe("AT");
  });

  it("retains invalid Hex and connects its inline error to the field", async () => {
    s.sendMode = "hex";
    await enterDraft("zz");
    sendButton().click();
    await flush();
    expect(write).not.toHaveBeenCalled();
    expect(input().value).toBe("zz");
    expect(input().getAttribute("aria-invalid")).toBe("true");
    const error = host.querySelector('[role="alert"]');
    expect(error?.textContent).toContain(
      "Enter at least one complete Hex byte.",
    );
    expect(input().getAttribute("aria-describedby")).toBe(error?.id);
    await enterDraft("AA,bb");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    sendButton().click();
    await flush();
    expect(Array.from(write.mock.calls[0][0] as Uint8Array)).toEqual([
      0xaa, 0xbb,
    ]);
    expect(input().value).toBe("");
  });

  it("shows a transport failure inline, keeps input, and allows retry", async () => {
    write.mockRejectedValueOnce(new Error("write failed"));
    await enterDraft("AT");
    sendButton().click();
    await flush();
    expect(input().value).toBe("AT");
    expect(host.querySelector('[role="alert"]')?.textContent).toContain(
      "Send failed: write failed. Your input was kept. Try sending again.",
    );
    expect(sendButton().disabled).toBe(false);
    sendButton().click();
    await flush();
    expect(write).toHaveBeenCalledTimes(2);
    expect(input().value).toBe("");
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });

  it("disables duplicate submits while allowing edits to a pending draft", async () => {
    let finish!: () => void;
    write.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    await enterDraft("AT");
    sendButton().click();
    await nextTick();
    expect(input().value).toBe("AT");
    expect(sendButton().disabled).toBe(true);
    expect(input().disabled).toBe(false);
    input().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    sendButton().click();
    expect(write).toHaveBeenCalledTimes(1);
    await enterDraft("NEXT");
    finish();
    await flush();
    expect(input().value).toBe("NEXT");
    expect(sendButton().disabled).toBe(false);
  });

  it("does not submit or cancel Enter during IME composition", async () => {
    await enterDraft("AT");
    input().dispatchEvent(new CompositionEvent("compositionstart"));
    const composing = new KeyboardEvent("keydown", {
      key: "Enter",
      cancelable: true,
    });
    input().dispatchEvent(composing);
    input().dispatchEvent(new CompositionEvent("compositionend"));
    const flagged = new KeyboardEvent("keydown", {
      key: "Enter",
      isComposing: true,
      cancelable: true,
    });
    input().dispatchEvent(flagged);
    const legacy = new KeyboardEvent("keydown", {
      key: "Enter",
      keyCode: 229,
      cancelable: true,
    });
    input().dispatchEvent(legacy);
    await flush();
    expect(write).not.toHaveBeenCalled();
    expect(
      [composing, flagged, legacy].every((event) => !event.defaultPrevented),
    ).toBe(true);
    expect(input().value).toBe("AT");
    input().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await flush();
    expect(write).toHaveBeenCalledTimes(1);
    expect(input().value).toBe("");
  });
});
