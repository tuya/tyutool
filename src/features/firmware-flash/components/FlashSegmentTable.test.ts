// @vitest-environment happy-dom
import { createApp, h, nextTick, type Component } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { createI18n } from "vue-i18n";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import en from "@/locales/en.json";
const originalScrollWidthDescriptor = Object.getOwnPropertyDescriptor(
  HTMLInputElement.prototype,
  "scrollWidth",
);
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
  vi.unstubAllGlobals();
  if (originalScrollWidthDescriptor) {
    Object.defineProperty(
      HTMLInputElement.prototype,
      "scrollWidth",
      originalScrollWidthDescriptor,
    );
  } else {
    Reflect.deleteProperty(HTMLInputElement.prototype, "scrollWidth");
  }
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

it("keeps firmware paths read-only, shows their tail, and copies by click or keyboard", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", {
    ...navigator,
    clipboard: { writeText },
  });
  Object.defineProperty(HTMLInputElement.prototype, "scrollWidth", {
    configurable: true,
    get() {
      return this.classList.contains("segment-path-input") ? 240 : 0;
    },
  });
  mount(Table);
  await flush();

  const paths = [
    ...host.querySelectorAll<HTMLInputElement>(".segment-path-input"),
  ];
  expect(paths).toHaveLength(4);
  expect(paths.every((input) => input.readOnly)).toBe(true);
  expect(paths.every((input) => input.dir === "ltr")).toBe(true);
  expect(paths[0].scrollLeft).toBe(240);
  expect(paths[0].title).toBe("/long/file.bin");

  paths[0].click();
  await flush();
  expect(writeText).toHaveBeenLastCalledWith("/long/file.bin");
  paths[2].dispatchEvent(
    new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
  );
  await flush();
  expect(writeText).toHaveBeenLastCalledWith("/long/file.bin");
  expect(host.querySelector('[role="status"]')?.textContent).toContain(
    en.common.copied,
  );

  paths[0].scrollLeft = 0;
  paths[0].dispatchEvent(new FocusEvent("focus"));
  expect(paths[0].scrollLeft).toBe(240);
  paths[0].scrollLeft = 0;
  window.dispatchEvent(new Event("resize"));
  await flush();
  expect(paths[0].scrollLeft).toBe(240);

  writeText.mockRejectedValueOnce(new Error("clipboard unavailable"));
  paths[0].click();
  await flush();
  const failure = host.querySelector<HTMLElement>('[role="alert"]');
  expect(failure?.textContent).toContain(en.common.copyFailed);
  expect(failure?.className).not.toContain("sr-only");
});

it("uses matching subtle stripe backgrounds for each segment and its address error", async () => {
  context.flashSegments[1].startAddr = "BAD!";
  mount(Table);
  await flush();

  const tableRows = host.querySelectorAll("tbody tr");
  expect(tableRows[1].className).toContain("bg-[color-mix");
  expect(tableRows[2].className).toContain("bg-[color-mix");
  expect(host.querySelectorAll(".segment-card-striped")[0].className).toContain(
    "segment-card-striped",
  );
  expect(
    host
      .querySelectorAll(".segment-card-striped")[0]
      .querySelector('[role="alert"]'),
  ).not.toBeNull();
});
