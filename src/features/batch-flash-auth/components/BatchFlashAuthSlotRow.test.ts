// @vitest-environment happy-dom
import { createApp, h, nextTick, reactive, type Component } from "vue";
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

import Row from "./BatchFlashAuthSlotRow.vue";
it("exposes no-code retry with its configuration reason and blocks quarantine retry", async () => {
  const retry = vi.fn();
  const slot = reactive({
    port: "COM31",
    status: "no_code",
    progress: 0,
    currentPhase: "",
    writeUncertain: false,
  });
  mount(Row, {
    portSlot: slot,
    retryDisabledReason: "Select firmware",
    onRetry: retry,
  });
  const button = [...host.querySelectorAll("button")].find(
    (b) => b.textContent?.trim() === en.batchFlashAuth.slot.retry,
  )!;
  expect(button).toBeDefined();
  expect(button.disabled).toBe(true);
  expect(button.getAttribute("title")).toBe("Select firmware");
  expect(button.getAttribute("aria-label")).toContain("COM31");
  button.click();
  expect(retry).not.toHaveBeenCalled();
  slot.writeUncertain = true;
  await flush();
  expect(
    [...host.querySelectorAll("button")].some(
      (b) => b.textContent?.trim() === en.batchFlashAuth.slot.retry,
    ),
  ).toBe(false);
});
it("supports the keyboard context menu and restores the row on Escape", async () => {
  mount(Row, {
    portSlot: { port: "COM31", status: "idle", progress: 0, currentPhase: "" },
  });
  const row = host.querySelector<HTMLElement>('[data-port="COM31"]')!;
  expect(row).toBeTruthy();
  row.focus();
  row.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "F10",
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    }),
  );
  await flush();
  const menu = document.querySelector<HTMLElement>('[role="menu"]')!;
  expect(menu.contains(document.activeElement)).toBe(true);
  document.activeElement!.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    }),
  );
  await flush();
  expect(document.querySelector('[role="menu"]')).toBeNull();
  expect(document.activeElement).toBe(row);
});
