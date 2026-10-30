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

import Dashboard from "./BatchFlashAuthDashboard.vue";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import {
  confirmDialogState,
  resolveConfirmDialog,
} from "@/composables/confirmDialog";
it("keeps archive access after the completion notice is dismissed", async () => {
  const s = useBatchFlashAuthStore();
  s.authConfig.excelPath = "/auth.xlsx";
  s.batchEndTime = Date.now();
  s.slots = [{ port: "COM3", status: "done", progress: 100, currentPhase: "" }];
  s.completionBanner = { kind: "all-success", count: 1 };
  mount(Dashboard);
  s.dismissBanner();
  await flush();
  expect(
    [...host.querySelectorAll("button")].some(
      (b) => b.textContent?.trim() === en.batchFlashAuth.archive.button,
    ),
  ).toBe(true);
});
it("names cumulative resets and allows cancellation without losing totals", async () => {
  const s = useBatchFlashAuthStore();
  s.cumulativeStats.flash = { total: 7, success: 5, fail: 2 };
  mount(Dashboard);
  const reset = host.querySelector<HTMLButtonElement>(
    'button[aria-label*="Flash"]',
  )!;
  expect(reset).toBeTruthy();
  reset.click();
  await flush();
  expect(confirmDialogState.visible).toBe(true);
  expect(s.cumulativeStats.flash.total).toBe(7);
  resolveConfirmDialog(false);
  await flush();
  expect(s.cumulativeStats.flash.total).toBe(7);
  reset.click();
  await flush();
  resolveConfirmDialog(true);
  await flush();
  expect(s.cumulativeStats.flash.total).toBe(0);
});
