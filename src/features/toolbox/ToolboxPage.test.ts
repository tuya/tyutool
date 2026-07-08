// @vitest-environment happy-dom
import { createApp, defineComponent, h, nextTick } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("vue-i18n", () => ({
  useI18n: () => ({
    t: (key: string) =>
      (
        ({
          "toolbox.section": "Tools",
          "app.nav.toolbox": "Toolbox",
          "toolbox.subtitle": "Toolbox subtitle",
          "toolbox.batchFlashAuth.name": "Batch flash auth",
          "toolbox.batchFlashAuth.desc": "Batch tool",
        }) as Record<string, string>
      )[key] ?? key,
  }),
}));

vi.mock("@/features/serial-port-indicators/useFeaturePortIndicators", () => ({
  useFeaturePortIndicators: () => ({
    activePorts: ["COM7", "COM9"],
    paletteMode: "light" as const,
    indicatorForFeature: () => ({
      enabled: true,
      active: true,
      ports: ["COM7", "COM9"],
      count: 2,
      displayMode: "count" as const,
    }),
  }),
}));

import ToolboxPage from "./ToolboxPage.vue";

describe("ToolboxPage serial port indicators", () => {
  let app: ReturnType<typeof createApp> | null = null;
  let host: HTMLDivElement | null = null;

  beforeEach(() => {
    host = document.createElement("div");
    document.body.appendChild(host);
  });

  afterEach(() => {
    app?.unmount();
    host?.remove();
    app = null;
    host = null;
  });

  it("shows the toolbox indicator on the batch flash auth card", async () => {
    app = createApp(ToolboxPage);
    app.component(
      "RouterLink",
      defineComponent({
        inheritAttrs: false,
        setup(_props, { attrs, slots }) {
          return () => h("a", attrs, slots.default?.());
        },
      }),
    );
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

    const indicator = document.querySelector(
      '[data-tool-id="batch-flash-auth"] [data-port-indicator-surface="toolbox-card"]',
    );
    expect(indicator).not.toBeNull();
    expect(indicator?.textContent?.includes("2")).toBe(true);
  });
});
