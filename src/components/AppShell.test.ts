// @vitest-environment happy-dom
import { createPinia, setActivePinia } from "pinia";
import { createApp, defineComponent, h, nextTick, reactive } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const route = reactive({
  path: "/flash",
  meta: { layout: "default" },
  name: "flash",
});

const indicatorState = {
  flash: {
    enabled: true,
    active: true,
    ports: ["COM3"],
    count: 1,
    displayMode: "single-port" as const,
  },
  "serial-debug": {
    enabled: true,
    active: false,
    ports: [],
    count: 0,
    displayMode: "single-port" as const,
  },
  toolbox: {
    enabled: true,
    active: true,
    ports: ["COM7", "COM9"],
    count: 2,
    displayMode: "count" as const,
  },
};

vi.mock("vue-router", () => ({
  useRoute: () => route,
  RouterLink: defineComponent({
    inheritAttrs: false,
    setup(_props, { attrs, slots }) {
      return () => h("a", attrs, slots.default?.());
    },
  }),
  RouterView: defineComponent({
    setup() {
      return () => null;
    },
  }),
}));

vi.mock("vue-i18n", () => ({
  useI18n: () => ({
    t: (key: string) =>
      (
        ({
          "app.mainNav": "Main navigation",
          "app.nav.flash": "Firmware tools",
          "app.nav.serialDebug": "Serial debug",
          "app.nav.toolbox": "Toolbox",
          "app.nav.settings": "Settings",
          "app.quickThemeOnLight": "Theme",
          "app.quickThemeOnDark": "Theme",
          "app.quickThemeOnSystem": "Theme",
          "app.quickLangOnZh": "Language",
          "app.quickLangOnEn": "Language",
        }) as Record<string, string>
      )[key] ?? key,
  }),
}));

vi.mock("@/features/serial-port-indicators/useFeaturePortIndicators", () => ({
  useFeaturePortIndicators: () => ({
    activePorts: ["COM3", "COM7", "COM9"],
    paletteMode: "light" as const,
    indicatorForFeature: (feature: "flash" | "serial-debug" | "toolbox") =>
      indicatorState[feature],
  }),
}));

import AppShell from "./AppShell.vue";

describe("AppShell serial port indicators", () => {
  let app: ReturnType<typeof createApp> | null = null;
  let host: HTMLDivElement | null = null;

  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    host = document.createElement("div");
    document.body.appendChild(host);
  });

  afterEach(() => {
    app?.unmount();
    host?.remove();
    app = null;
    host = null;
  });

  it("renders indicators for active flash and toolbox nav items only", async () => {
    app = createApp(AppShell);
    app.component(
      "FontAwesomeIcon",
      defineComponent({
        setup() {
          return () => h("i");
        },
      }),
    );
    app.mount(host!);
    await nextTick();

    expect(
      document.querySelector('[data-port-indicator-feature="flash"]'),
    ).not.toBeNull();
    expect(
      document.querySelector('[data-port-indicator-feature="toolbox"]'),
    ).not.toBeNull();
    expect(
      document.querySelector('[data-port-indicator-feature="serial-debug"]'),
    ).toBeNull();
    expect(
      document
        .querySelector('[data-port-indicator-feature="toolbox"]')
        ?.textContent?.includes("2"),
    ).toBe(true);
  });
});
