// @vitest-environment happy-dom
import { createApp, defineComponent, h, nextTick } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";

vi.mock("./BatchFlashAuthPortFilterModal.vue", () => ({
  default: defineComponent({ setup: () => () => null }),
}));
import BatchFlashAuthToolbar from "./BatchFlashAuthToolbar.vue";

// Parent task owns the production locale catalogues.
const messages = {
  en: {
    currentMode: "Current mode: {mode}",
    modeFlashThenAuth: "Flash then authorize",
    modeAuthOnly: "Authorize only",
    modeFlashOnly: "Flash only",
    excelValidationPending: "Wait for auth table validation",
  },
  "zh-CN": {
    currentMode: "当前模式：{mode}",
    modeFlashThenAuth: "先烧录后授权",
    modeAuthOnly: "仅授权",
    modeFlashOnly: "仅烧录",
    excelValidationPending: "请等待授权表校验完成",
  },
};

describe("BatchFlashAuthToolbar operation summary", () => {
  let app: ReturnType<typeof createApp>;
  let host: HTMLDivElement;
  let pinia: ReturnType<typeof createPinia>;
  const initialLocale = i18n.global.locale.value;

  beforeEach(() => {
    pinia = createPinia();
    setActivePinia(pinia);
    for (const [locale, toolbar] of Object.entries(messages)) {
      i18n.global.mergeLocaleMessage(locale, { batchFlashAuth: { toolbar } });
    }
    host = document.createElement("div");
    document.body.appendChild(host);
  });
  afterEach(() => {
    app?.unmount();
    host.remove();
    i18n.global.locale.value = initialLocale;
  });

  it.each(["en", "zh-CN"] as const)(
    "shows configured mode and visible firmware prerequisite in %s",
    async (locale) => {
      i18n.global.locale.value = locale;
      const store = useBatchFlashAuthStore();
      store.addPorts(["COM3"]);
      store.authConfig.excelPath = "/auth.xlsx";
      await nextTick();
      store.excelStats = {
        total: 1,
        used: 0,
        inProgress: 0,
        remaining: 1,
        invalid: 0,
      };
      const start = vi.spyOn(store, "startBatch");
      app = createApp(BatchFlashAuthToolbar).use(pinia).use(i18n);
      app.component(
        "FontAwesomeIcon",
        defineComponent({ setup: () => () => h("i") }),
      );
      app.mount(host);
      const button = Array.from(host.querySelectorAll("button")).find(
        (el) =>
          el.textContent?.trim() ===
          i18n.global.t("batchFlashAuth.toolbar.start"),
      )!;
      expect(host.textContent).toContain(messages[locale].modeFlashThenAuth);
      const reason = host.querySelector("#batch-start-prerequisite");
      expect(reason?.textContent).toBe(
        i18n.global.t("batchFlashAuth.toolbar.selectFirmwareFirst"),
      );
      expect(button.getAttribute("aria-describedby")).toContain(
        "batch-start-prerequisite",
      );
      expect(button.disabled).toBe(true);
      button.click();
      expect(start).not.toHaveBeenCalled();

      store.firmwarePath = "/fw.bin";
      await nextTick();
      expect(button.disabled).toBe(false);
      expect(host.querySelector("#batch-start-prerequisite")).toBeNull();
      store.excelStats = null;
      await nextTick();
      expect(host.querySelector("#batch-start-prerequisite")?.textContent).toBe(
        messages[locale].excelValidationPending,
      );
      store.authorizeEnabled = false;
      await nextTick();
      expect(host.textContent).toContain(messages[locale].modeFlashOnly);
      expect(button.disabled).toBe(false);
      store.authorizeEnabled = true;
      store.flashFirmware = false;
      await nextTick();
      expect(host.textContent).toContain(messages[locale].modeAuthOnly);
    },
  );
});
