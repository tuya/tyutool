// @vitest-environment happy-dom
import { createApp, h, nextTick, type Component } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { createI18n } from "vue-i18n";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import en from "@/locales/en.json";
vi.mock("@/runtime", () => ({
  isTauriRuntime: () => false,
  getRuntime: () => "web",
}));
let app: ReturnType<typeof createApp> | undefined;
let host: HTMLElement;
beforeEach(() => {
  setActivePinia(createPinia());
  host = document.createElement("div");
  document.body.append(host);
});
afterEach(() => {
  app?.unmount();
  document.body.replaceChildren();
  vi.restoreAllMocks();
});
function mount(component: Component, props: Record<string, unknown> = {}) {
  app = createApp({ render: () => h(component, props) });
  app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
  app.component("FontAwesomeIcon", { render: () => h("i") });
  app.mount(host);
}
async function flush() {
  await nextTick();
  await new Promise((r) => setTimeout(r, 0));
  await nextTick();
}

import ChipBar from "./SerialDebugChipBar.vue";
import { useSerialDebugStore } from "@/stores/serial-debug";
describe("filter controls", () => {
  it("ignores IME Enter, blocks duplicate pending submissions and retains a failed draft", async () => {
    const s = useSerialDebugStore();
    let reject!: (e: Error) => void;
    const add = vi
      .spyOn(s, "addChip")
      .mockImplementation(() => new Promise((_r, j) => (reject = j)));
    mount(ChipBar);
    const trigger = host.querySelector<HTMLButtonElement>(".tab-add")!;
    trigger.focus();
    trigger.click();
    await flush();
    const input = document.querySelector<HTMLInputElement>(".pop-input")!;
    input.value = "ERR";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        isComposing: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await flush();
    expect(add).not.toHaveBeenCalled();
    const submit = document.querySelector<HTMLButtonElement>(".pop-add-btn")!;
    submit.click();
    submit.click();
    await flush();
    expect(add).toHaveBeenCalledOnce();
    expect(submit.disabled).toBe(true);
    reject(new Error("Filter rejected"));
    await flush();
    expect(input.value).toBe("ERR");
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      "Filter rejected",
    );
    expect(submit.disabled).toBe(false);
    document.querySelector<HTMLButtonElement>(".pop-toggle")!.focus();
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await flush();
    expect(document.querySelector(".add-popover")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });
  it("exposes separate activation and removal buttons with filter context", async () => {
    const s = useSerialDebugStore();
    s.watchChips = [
      { id: "one", keyword: "ERROR", useRegex: false, color: "red" },
    ];
    vi.spyOn(s, "setActiveChip").mockResolvedValue(undefined);
    mount(ChipBar);
    const chip = host.querySelector(".tab-chip")!;
    const buttons = chip.querySelectorAll("button");
    expect(buttons).toHaveLength(2);
    expect(buttons[0].getAttribute("aria-pressed")).toBe("false");
    expect(buttons[1].getAttribute("aria-label")).toContain("ERROR");
    buttons[0].click();
    expect(s.setActiveChip).toHaveBeenCalledWith("one");
  });

  it("shows a failed removal instead of clearing the error when reopening feedback", async () => {
    const s = useSerialDebugStore();
    s.watchChips = [
      { id: "one", keyword: "ERROR", useRegex: false, color: "red" },
    ];
    vi.spyOn(s, "removeChip").mockRejectedValue(new Error("Remove failed"));
    mount(ChipBar);
    host.querySelector<HTMLButtonElement>(".tab-close")!.click();
    await flush();
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      "Remove failed",
    );
  });
});
