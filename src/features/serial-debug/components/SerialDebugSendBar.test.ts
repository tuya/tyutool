// @vitest-environment happy-dom
import { createPinia, setActivePinia } from "pinia";
import { createApp, defineComponent, h, nextTick } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { t } = vi.hoisted(() => ({
  t: (key: string, params?: { msg?: string; n?: number }): string => {
    const messages: Record<string, string> = {
      "serialDebug.send.inputLabel": "Data to send",
      "serialDebug.send.sendBtn": "Send",
      "serialDebug.send.sending": "Sending…",
      "serialDebug.send.openPortHint": "Open a port to send.",
      "serialDebug.send.emptyHint": "Enter data to send.",
      "serialDebug.send.byteCount": `${params?.n} bytes`,
      "serialDebug.send.crlfPreview": "CRLF adds 2 bytes",
      "serialDebug.send.noCrlfPreview": "No line ending",
      "serialDebug.send.hexPreviewIgnored": `${params?.n} trailing Hex character ignored`,
      "serialDebug.send.invalidHex": "Enter at least one complete Hex byte.",
      "serialDebug.send.failed": `Send failed: ${params?.msg}. Your input was kept. Try sending again.`,
    };
    return messages[key] ?? key;
  },
}));
vi.mock("vue-i18n", () => ({
  useI18n: () => ({ t, locale: { value: "en" } }),
  createI18n: () => ({
    global: { t, te: () => true, locale: { value: "en" } },
  }),
}));
vi.mock("@/runtime", () => ({
  isTauriRuntime: () => false,
  getRuntime: () => "web",
}));

import { parseSendHistory, recordSendHistory } from "../send-history";
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
    return host.querySelector<HTMLButtonElement>(".btn-primary")!;
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

  it("previews the exact lenient Hex bytes and warns about an odd trailing digit", async () => {
    s.sendMode = "hex";
    await enterDraft("41, 42; F!");
    const preview = host.querySelector(".send-preview")!;
    expect(preview.textContent).toContain("2 bytes");
    expect(preview.textContent).toContain("1 trailing Hex character ignored");
    sendButton().click();
    await flush();
    expect(write).toHaveBeenCalledWith(Uint8Array.of(0x41, 0x42));
    await enterDraft("!");
    expect(preview.textContent).toContain(
      "Enter at least one complete Hex byte.",
    );
    expect(preview.textContent).not.toContain("trailing Hex");
  });

  it("includes UTF-8 and the optional CRLF in the ASCII byte count", async () => {
    s.sendAppendCrlf = true;
    await enterDraft("中");
    const preview = host.querySelector(".send-preview")!;
    expect(preview.textContent).toContain("5 bytes");
    expect(preview.textContent).toContain("CRLF adds 2 bytes");
    s.sendAppendCrlf = false;
    await nextTick();
    expect(preview.textContent).toContain("3 bytes");
    expect(preview.textContent).toContain("No line ending");
    sendButton().click();
    await flush();
    expect(write).toHaveBeenCalledWith(new TextEncoder().encode("中"));
  });

  it("explains empty, disconnected and pending states while keeping the draft editable", async () => {
    await nextTick();
    expect(host.textContent).toContain("Enter data to send.");
    s.open = false;
    await enterDraft("AT");
    expect(host.textContent).toContain("Open a port to send.");
    expect(sendButton().disabled).toBe(true);
    s.open = true;
    let finish!: () => void;
    write.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    await nextTick();
    sendButton().click();
    await flush();
    expect(sendButton().textContent).toContain("Sending…");
    expect(sendButton().disabled).toBe(true);
    await enterDraft("NEXT");
    finish();
    await flush();
    expect(s.sendInput).toBe("NEXT");
    expect(sendButton().textContent).toContain("Send");
  });

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

  it("previews without replacing a draft, then restores send settings and the original draft", async () => {
    s.sendHistory = recordSendHistory([], "55 AA", "hex", false, 1000);
    await enterDraft("UNSENT");
    s.sendMode = "ascii";
    s.sendAppendCrlf = true;
    host.querySelector<HTMLButtonElement>(".history-wrap > button")!.click();
    await nextTick();
    expect(document.querySelector(".history-preview pre")?.textContent).toBe(
      "55 AA",
    );
    expect(s.sendInput).toBe("UNSENT");
    document.querySelector<HTMLButtonElement>(".history-fill")!.click();
    await nextTick();
    expect(s.sendInput).toBe("55 AA");
    expect(s.sendMode).toBe("hex");
    expect(s.sendAppendCrlf).toBe(false);
    expect(write).not.toHaveBeenCalled();
    host.querySelector<HTMLButtonElement>(".restore-draft")!.click();
    await nextTick();
    expect(s.sendInput).toBe("UNSENT");
    expect(s.sendMode).toBe("ascii");
    expect(s.sendAppendCrlf).toBe(true);
  });

  it("keeps current settings when filling a migrated entry and can prepare input offline", async () => {
    s.sendHistory = parseSendHistory(["OLD"]);
    s.open = false;
    s.sendMode = "hex";
    await nextTick();
    host.querySelector<HTMLButtonElement>(".history-wrap > button")!.click();
    await nextTick();
    document.querySelector<HTMLButtonElement>(".history-fill")!.click();
    await nextTick();
    expect(s.sendMode).toBe("hex");
    expect(input().disabled).toBe(false);
    expect(sendButton().disabled).toBe(true);
  });

  it("does not save a command name while confirming IME text", async () => {
    s.sendHistory = recordSendHistory([], "AT", "ascii", true, 1000);
    host.querySelector<HTMLButtonElement>(".history-wrap > button")!.click();
    await nextTick();
    const name = document.querySelector<HTMLInputElement>(
      ".history-name input",
    )!;
    name.value = "查询版本";
    name.dispatchEvent(new Event("input", { bubbles: true }));
    await nextTick();
    const compositionEnter = new KeyboardEvent("keydown", {
      key: "Enter",
      isComposing: true,
      bubbles: true,
      cancelable: true,
    });
    name.dispatchEvent(compositionEnter);
    await nextTick();
    expect(s.sendHistory[0].name).toBe("");
    expect(compositionEnter.defaultPrevented).toBe(false);
    name.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    expect(s.sendHistory[0].name).toBe("查询版本");
  });

  it("restores the unsent draft after browsing history and leaves Down alone outside history", async () => {
    s.sendHistory = parseSendHistory(["OLD", "OLDER"]);
    await enterDraft("UNSENT");
    const key = async (key: string) => {
      input().dispatchEvent(
        new KeyboardEvent("keydown", { key, cancelable: true }),
      );
      await nextTick();
    };
    await key("ArrowDown");
    expect(input().value).toBe("UNSENT");
    await key("ArrowUp");
    expect(input().value).toBe("OLD");
    await key("ArrowUp");
    expect(input().value).toBe("OLDER");
    await key("ArrowDown");
    await key("ArrowDown");
    expect(input().value).toBe("UNSENT");
    await enterDraft("NEW");
    await key("ArrowDown");
    expect(input().value).toBe("NEW");
  });

  it("closes history on Escape and restores the trigger", async () => {
    s.sendHistory = parseSendHistory(["OLD"]);
    await nextTick();
    const trigger = host.querySelector<HTMLButtonElement>(
      ".history-wrap > button",
    )!;
    trigger.click();
    await nextTick();
    const item = document.querySelector<HTMLButtonElement>(".history-item")!;
    item.focus();
    item.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    expect(document.querySelector(".history-dropdown")).toBeNull();
    expect(document.activeElement).toBe(trigger);
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
    expect(input().getAttribute("aria-describedby")?.split(" ")).toContain(
      error?.id,
    );
    expect(
      host
        .querySelector(".send-feedback")
        ?.textContent?.split("Enter at least one complete Hex byte."),
    ).toHaveLength(2);
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

  it("restores a newer history draft after an older send completes", async () => {
    let finish!: () => void;
    write.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    s.sendHistory = parseSendHistory(["OLD"]);
    await enterDraft("AT");
    sendButton().click();
    await nextTick();
    await enterDraft("NEXT");
    input().dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowUp", cancelable: true }),
    );
    await nextTick();
    expect(input().value).toBe("OLD");
    finish();
    await flush();
    input().dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", cancelable: true }),
    );
    await nextTick();
    expect(input().value).toBe("NEXT");
  });
});
