// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from "vue";
import { createI18n } from "vue-i18n";
import UpdateDialog from "./UpdateDialog.vue";
import en from "@/locales/en.json";
import updateDialogSource from "./UpdateDialog.vue?raw";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./update-sources", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./update-sources")>()),
  fetchLatestJson: vi.fn().mockRejectedValue(new Error("Offline test")),
}));

describe("UpdateDialog keyboard focus", () => {
  let app: ReturnType<typeof createApp>;
  afterEach(() => {
    app.unmount();
    document.body.replaceChildren();
  });

  it("enters, traps focus, ignores handled Escape and restores on close", async () => {
    const host = document.createElement("div");
    const opener = document.createElement("button");
    document.body.append(opener, host);
    opener.focus();
    const open = ref(false);
    app = createApp({
      setup: () => () =>
        h(UpdateDialog, {
          open: open.value,
          onClose: () => {
            open.value = false;
          },
        }),
    });
    app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
    app.component("FontAwesomeIcon", () => h("span"));
    app.mount(host);
    open.value = true;
    await nextTick();
    await nextTick();
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    const button = dialog.querySelector("button")!;
    expect(document.activeElement).toBe(button);
    const tab = new KeyboardEvent("keydown", {
      key: "Tab",
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    button.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(true);
    const buttons = [...dialog.querySelectorAll("button:not([disabled])")];
    expect(document.activeElement).toBe(buttons[buttons.length - 1]);
    const event = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    event.preventDefault();
    button.dispatchEvent(event);
    expect(open.value).toBe(true);
    button.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    await nextTick();
    expect(open.value).toBe(false);
    expect(document.activeElement).toBe(opener);
  });
});

describe("UpdateDialog source actions", () => {
  it("renders a per-source update action in each source card", () => {
    expect(updateDialogSource).toContain(
      '@click="triggerSourceAction(source)"',
    );
    expect(updateDialogSource).toContain(
      't("settings.update.updateFromSource")',
    );
    expect(updateDialogSource).toContain(
      "v-if=\"sourceActionKind(source) !== 'none'\"",
    );
  });
});
