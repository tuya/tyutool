// @vitest-environment happy-dom
import { createApp, h, nextTick, type Component } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { createI18n } from "vue-i18n";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
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

import Popup from "./SerialDebugRxSelectionHexPopup.vue";
import { useSerialDebugStore } from "@/stores/serial-debug";
it("shows readonly named bytes and closes with focus restored", async () => {
  const s = useSerialDebugStore();
  const opener = document.createElement("button");
  document.body.append(opener);
  opener.focus();
  mount(Popup);
  s.hexPopup = {
    open: true,
    bytes: new Uint8Array([65, 66, 67]),
    initialMode: "hex",
  };
  await flush();
  const dialog = host.querySelector<HTMLElement>('[role="dialog"]')!;
  expect(dialog.contains(document.activeElement)).toBe(true);
  const input = dialog.querySelector("textarea")!;
  expect(input.readOnly).toBe(true);
  expect(input.getAttribute("aria-label")).toBeTruthy();
  [...dialog.querySelectorAll("button")]
    .find((b) => b.textContent?.trim() === "ASCII")!
    .click();
  await flush();
  expect(input.value).toBe("ABC");
  expect(
    dialog.querySelector('[aria-pressed="true"]')?.textContent?.trim(),
  ).toBe("ASCII");
  document.activeElement!.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    }),
  );
  await flush();
  expect(s.hexPopup.open).toBe(false);
  expect(document.activeElement).toBe(opener);
});
