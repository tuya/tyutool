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

import Table from "./FlashSegmentTable.vue";
import { reactive } from "vue";
vi.mock("../context", () => ({ useFirmwareFlashContext: () => context }));
import type { FlashSegment } from "../types";
let context: {
  busy: boolean;
  flashSegments: FlashSegment[];
  onPickFile: ReturnType<typeof vi.fn>;
  addSegment: ReturnType<typeof vi.fn>;
  removeSegment: (index: number) => void;
};
beforeEach(() => {
  context = reactive({
    busy: false,
    flashSegments: [
      {
        id: "one",
        startAddr: "0x00",
        endAddr: "0x20",
        firmwarePath: "/long/file.bin",
        firmwareFile: new File([new Uint8Array(32)], "file.bin"),
      },
      {
        id: "two",
        startAddr: "0x00",
        endAddr: "0x10",
        firmwarePath: "",
        firmwareFile: null,
      },
    ],
    onPickFile: vi.fn(),
    addSegment: vi.fn(),
    removeSegment: (i: number) => context.flashSegments.splice(i, 1),
  });
});
it("associates invalid addresses with the correct fields in both layouts", async () => {
  context.flashSegments[0].startAddr = "BAD!";
  mount(Table);
  await flush();
  for (const prefix of ["flash-", "flash-m-"]) {
    const start = host.querySelector<HTMLInputElement>(`#${prefix}one-start`)!;
    const end = host.querySelector<HTMLInputElement>(`#${prefix}one-end`)!;
    expect(start.getAttribute("aria-invalid")).toBe("true");
    expect(end.getAttribute("aria-invalid")).toBe("false");
    expect(
      document.getElementById(start.getAttribute("aria-describedby")!)
        ?.textContent,
    ).toContain(en.flash.err.addrInvalid);
  }
  context.flashSegments[0].startAddr = "0x20";
  context.flashSegments[0].endAddr = "oops";
  await flush();
  expect(
    host.querySelector("#flash-one-end")!.getAttribute("aria-invalid"),
  ).toBe("true");
  expect(
    host.querySelector("#flash-one-start")!.getAttribute("aria-invalid"),
  ).toBe("false");
});
it("returns focus to a remaining control when deleting a segment", async () => {
  mount(Table);
  const buttons = [
    ...host.querySelectorAll<HTMLButtonElement>("button[aria-label]"),
  ].filter((b) =>
    b.getAttribute("aria-label")?.includes(en.flash.removeSegment),
  );
  const button = buttons[0];
  button.focus();
  button.click();
  await flush();
  expect(document.activeElement).not.toBe(document.body);
  expect(document.activeElement?.isConnected).toBe(true);
  expect(context.flashSegments).toHaveLength(1);
});
