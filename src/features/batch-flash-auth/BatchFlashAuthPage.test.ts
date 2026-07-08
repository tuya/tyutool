// @vitest-environment happy-dom
import { createApp, defineComponent, h, nextTick, reactive } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const pushSpy = vi.fn();
const mockStore = reactive({
  isBusy: false,
  loadPersistedData: vi.fn(async () => undefined),
  ensureListener: vi.fn(async () => undefined),
  autoAssign: vi.fn(async () => undefined),
  cleanup: vi.fn(),
});

vi.mock("vue-i18n", () => ({
  useI18n: () => ({
    t: (key: string) =>
      (
        ({
          "toolbox.batchFlashAuth.name": "Batch flash auth",
          "batchFlashAuth.title": "Batch flash auth",
          "batchFlashAuth.subtitle": "Batch subtitle",
          "batchFlashAuth.configurationSection": "Configuration",
          "batchFlashAuth.configurationHint": "Hint",
          "batchFlashAuth.collapse": "Collapse",
          "batchFlashAuth.expand": "Expand",
        }) as Record<string, string>
      )[key] ?? key,
  }),
}));

vi.mock("vue-router", () => ({
  useRouter: () => ({
    push: pushSpy,
  }),
}));

vi.mock("@/stores/batch-flash-auth", () => ({
  useBatchFlashAuthStore: () => mockStore,
}));

vi.mock("@/features/serial-port-indicators/useFeaturePortIndicators", () => ({
  useFeaturePortIndicators: () => ({
    activePorts: ["COM7", "COM9", "COM11"],
    paletteMode: "light" as const,
    indicatorForFeature: () => ({
      enabled: true,
      active: true,
      ports: ["COM7", "COM9", "COM11"],
      count: 3,
      displayMode: "count" as const,
    }),
  }),
}));

vi.mock("./components/BatchFlashAuthDashboard.vue", () => ({
  default: defineComponent({
    setup() {
      return () => h("div", "dashboard");
    },
  }),
}));

vi.mock("./components/BatchFlashAuthConfig.vue", () => ({
  default: defineComponent({
    setup() {
      return () => h("div", "flash-config");
    },
  }),
}));

vi.mock("./components/BatchAuthConfig.vue", () => ({
  default: defineComponent({
    setup() {
      return () => h("div", "auth-config");
    },
  }),
}));

vi.mock("./components/BatchFlashAuthToolbar.vue", () => ({
  default: defineComponent({
    setup() {
      return () => h("div", "toolbar");
    },
  }),
}));

vi.mock("./components/BatchFlashAuthSlotList.vue", () => ({
  default: defineComponent({
    setup() {
      return () => h("div", "slot-list");
    },
  }),
}));

vi.mock("@/features/toolbox/components/ToolboxBreadcrumb.vue", () => ({
  default: defineComponent({
    setup() {
      return () => h("div", "breadcrumb");
    },
  }),
}));

vi.mock("./components/DisclaimerModal.vue", () => ({
  default: defineComponent({
    setup() {
      return () => h("div", "disclaimer");
    },
  }),
}));

import BatchFlashAuthPage from "./BatchFlashAuthPage.vue";

describe("BatchFlashAuthPage serial port indicators", () => {
  let app: ReturnType<typeof createApp> | null = null;
  let host: HTMLDivElement | null = null;

  beforeEach(() => {
    pushSpy.mockReset();
    mockStore.isBusy = false;
    mockStore.loadPersistedData.mockClear();
    mockStore.ensureListener.mockClear();
    mockStore.autoAssign.mockClear();
    mockStore.cleanup.mockClear();
    localStorage.clear();
    host = document.createElement("div");
    document.body.appendChild(host);
  });

  afterEach(() => {
    app?.unmount();
    host?.remove();
    app = null;
    host = null;
  });

  it("shows the aggregated toolbox indicator in the page header", async () => {
    app = createApp(BatchFlashAuthPage);
    app.component(
      "FontAwesomeIcon",
      defineComponent({
        setup() {
          return () => h("i");
        },
      }),
    );
    app.mount(host!);
    await Promise.resolve();
    await nextTick();

    const indicator = document.querySelector(
      '[data-port-indicator-surface="batch-flash-auth-header"]',
    );
    expect(indicator).not.toBeNull();
    expect(indicator?.textContent?.includes("3")).toBe(true);
  });
});
