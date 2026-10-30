// @vitest-environment happy-dom
import { createApp, h, nextTick, reactive, ref } from "vue";
import { createI18n } from "vue-i18n";
import { afterEach, describe, expect, it, vi } from "vitest";
import SerialDebugSettingsModal from "./SerialDebugSettingsModal.vue";
import en from "@/locales/en.json";

vi.mock("@/stores/serial-debug", () => ({
  useSerialDebugStore: () =>
    reactive({
      open: false,
      dataBits: "eight",
      parity: "none",
      stopBits: "one",
      autoRelease: true,
      hexView: false,
      ansiEnabled: true,
    }),
}));

describe("SerialDebugSettingsModal dropdown focus", () => {
  let app: ReturnType<typeof createApp>;
  afterEach(() => {
    app.unmount();
    document.body.replaceChildren();
  });

  it("keeps teleported options usable, consumes dropdown Escape first, then restores the opener", async () => {
    const host = document.createElement("div");
    const opener = document.createElement("button");
    document.body.append(opener, host);
    opener.focus();
    const open = ref(true);
    const close = vi.fn(() => {
      open.value = false;
    });
    app = createApp({
      setup: () => () =>
        open.value ? h(SerialDebugSettingsModal, { onClose: close }) : null,
    });
    app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
    app.component("FontAwesomeIcon", () => h("span"));
    app.mount(host);
    await nextTick();
    await nextTick();
    expect(document.activeElement).toBe(host.querySelector("button"));
    expect(opener.hasAttribute("inert")).toBe(true);
    const trigger =
      host.querySelector<HTMLButtonElement>(".ty-select-trigger")!;
    trigger.focus();
    trigger.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowDown",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    await nextTick();
    const list = document.querySelector<HTMLElement>('[role="listbox"]')!;
    expect(list.closest("[inert]")).toBeNull();
    list
      .querySelector('[role="option"]')!
      .dispatchEvent(
        new MouseEvent("mousedown", { bubbles: true, cancelable: true }),
      );
    await nextTick();
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(trigger);
    trigger.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowDown",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    trigger.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    expect(close).not.toHaveBeenCalled();
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    trigger.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    await nextTick();
    expect(close).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(opener);
    expect(opener.hasAttribute("inert")).toBe(false);
  });
});
