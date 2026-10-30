// @vitest-environment happy-dom
import { createApp, h, nextTick } from "vue";
import { createI18n } from "vue-i18n";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import TyConfirmDialog from "./TyConfirmDialog.vue";
import {
  confirmDialogState,
  resolveConfirmDialog,
  showConfirmDialog,
} from "@/composables/confirmDialog";
import en from "@/locales/en.json";

describe("TyConfirmDialog keyboard focus", () => {
  let app: ReturnType<typeof createApp>;
  let opener: HTMLButtonElement;

  async function settle() {
    await nextTick();
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    await nextTick();
  }

  beforeEach(() => {
    const host = document.createElement("div");
    opener = document.createElement("button");
    document.body.append(opener, host);
    opener.focus();
    app = createApp(TyConfirmDialog);
    app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
    app.component("FontAwesomeIcon", () => h("span"));
    app.mount(host);
  });

  afterEach(() => {
    resolveConfirmDialog(false);
    app.unmount();
    document.body.replaceChildren();
  });

  it.each(["danger", "warning"] as const)(
    "focuses safe Cancel for %s",
    async (kind) => {
      void showConfirmDialog({ title: "Confirm", message: "Proceed?", kind });
      await settle();
      expect(document.activeElement).toBe(
        document.querySelector(".ty-dialog-btn-cancel"),
      );
    },
  );

  it.each([true, false])(
    "focuses the informational action with showCancel=%s",
    async (showCancel) => {
      void showConfirmDialog({
        title: "Notice",
        message: "Done",
        kind: "info",
        showCancel,
      });
      await settle();
      expect(document.activeElement).toBe(
        document.querySelector(".ty-dialog-btn-ok"),
      );
    },
  );

  it.each(["danger", "warning"] as const)(
    "focuses OK for %s when Cancel is absent",
    async (kind) => {
      void showConfirmDialog({
        title: "Notice",
        message: "Done",
        kind,
        showCancel: false,
      });
      await settle();
      expect(document.activeElement).toBe(
        document.querySelector(".ty-dialog-btn-ok"),
      );
    },
  );

  it("cycles Tab at both ends and restores the opener on Escape", async () => {
    const result = showConfirmDialog({ title: "Confirm", message: "Proceed?" });
    await settle();
    const buttons = document.querySelectorAll<HTMLButtonElement>(
      '[role="alertdialog"] button',
    );
    const first = buttons[0];
    const last = buttons[buttons.length - 1];
    last.focus();
    const tab = new KeyboardEvent("keydown", {
      key: "Tab",
      bubbles: true,
      cancelable: true,
    });
    last.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(first);
    first.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).toBe(last);
    last.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await settle();
    expect(await result).toBe(false);
    expect(document.activeElement).toBe(opener);
  });

  it("does not dismiss on an Escape already handled by a child", async () => {
    void showConfirmDialog({ title: "Notice", message: "Done" });
    await settle();
    const button =
      document.querySelector<HTMLButtonElement>(".ty-dialog-btn-ok")!;
    button.addEventListener("keydown", (event) => event.preventDefault(), {
      once: true,
    });
    button.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await settle();
    expect(confirmDialogState.visible).toBe(true);
  });

  it("restores the opener on a button close", async () => {
    void showConfirmDialog({ title: "Notice", message: "Done" });
    await settle();
    document.querySelector<HTMLButtonElement>(".ty-dialog-close")!.click();
    await settle();
    expect(document.activeElement).toBe(opener);
  });

  it("restores focus when the component unmounts while open", async () => {
    void showConfirmDialog({ title: "Notice", message: "Done" });
    await settle();
    app.unmount();
    await settle();
    expect(document.activeElement).toBe(opener);
  });
});
