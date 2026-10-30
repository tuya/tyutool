// @vitest-environment happy-dom
import { createApp, h, nextTick, reactive, ref } from "vue";
import { createI18n } from "vue-i18n";
import { afterEach, describe, expect, it, vi } from "vitest";
import BatchFlashAuthPortFilterModal from "./BatchFlashAuthPortFilterModal.vue";
import en from "@/locales/en.json";

vi.mock("@/stores/batch-flash-auth", () => ({
  useBatchFlashAuthStore: () => ({
    filterConfig: reactive({ blockedPorts: [] }),
    addBlockedPort: vi.fn(),
    removeBlockedPort: vi.fn(),
  }),
}));

describe("port filter modal accessibility", () => {
  let app: ReturnType<typeof createApp>;
  afterEach(() => {
    app.unmount();
    document.body.replaceChildren();
  });

  it("labels the dialog and input, focuses the field, cycles Tab and restores on Escape", async () => {
    const host = document.createElement("div");
    const opener = document.createElement("button");
    document.body.append(opener, host);
    opener.focus();
    const open = ref(false);
    app = createApp({
      setup: () => () =>
        h(BatchFlashAuthPortFilterModal, {
          open: open.value,
          onClose: () => {
            open.value = false;
          },
        }),
    });
    app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
    app.mount(host);
    open.value = true;
    await nextTick();
    await nextTick();
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]');
    expect(dialog?.getAttribute("aria-modal")).toBe("true");
    expect(
      document.getElementById(dialog!.getAttribute("aria-labelledby")!)
        ?.textContent,
    ).toContain(en.batchFlashAuth.filter.title);
    const input = dialog!.querySelector("input")!;
    expect(input.getAttribute("aria-label")).toBe(en.batchFlashAuth.slot.port);
    expect(document.activeElement).toBe(input);
    const buttons = dialog!.querySelectorAll("button");
    buttons[buttons.length - 1].focus();
    buttons[buttons.length - 1].dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).toBe(buttons[0]);
    input.dispatchEvent(
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
