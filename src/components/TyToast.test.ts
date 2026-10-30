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
  vi.useFakeTimers();
  setActivePinia(createPinia());
  host = document.createElement("div");
  document.body.append(host);
});
afterEach(() => {
  app?.unmount();
  document.body.replaceChildren();
  vi.useRealTimers();
  vi.restoreAllMocks();
});
function mount(component: Component, props: Record<string, unknown> = {}) {
  app = createApp({ render: () => h(component, props) });
  app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
  app.component("FontAwesomeIcon", { render: () => h("i") });
  app.mount(host);
}

import Toast from "./TyToast.vue";
import { toastState } from "@/composables/toastState";
const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
it("pauses expiry while focused or hovered and opens the update task directly", async () => {
  toastState.visible = false;
  mount(Toast);
  toastState.visible = true;
  await nextTick();
  const toast = document.querySelector<HTMLElement>(".ty-toast")!;
  const action = toast.querySelector("button")!;
  action.focus();
  await vi.advanceTimersByTimeAsync(6500);
  expect(document.querySelector(".ty-toast")).not.toBeNull();
  action.blur();
  toast.dispatchEvent(new MouseEvent("mouseenter"));
  await vi.advanceTimersByTimeAsync(6500);
  expect(document.querySelector(".ty-toast")).not.toBeNull();
  action.click();
  await nextTick();
  expect(push).toHaveBeenCalledWith({
    path: "/settings",
    query: { update: "1" },
  });
});
it("expires after interaction leaves", async () => {
  toastState.visible = false;
  mount(Toast);
  toastState.visible = true;
  await nextTick();
  const action = document.querySelector<HTMLButtonElement>(".ty-toast-action")!;
  action.focus();
  await vi.advanceTimersByTimeAsync(3000);
  action.blur();
  await vi.advanceTimersByTimeAsync(6001);
  expect(toastState.visible).toBe(false);
});
