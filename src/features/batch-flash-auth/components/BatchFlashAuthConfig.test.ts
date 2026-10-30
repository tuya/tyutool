// @vitest-environment happy-dom
import { createApp, defineComponent, h, nextTick } from "vue";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import { i18n } from "@/i18n";
import BatchFlashAuthConfig from "./BatchFlashAuthConfig.vue";

describe("BatchFlashAuthConfig field labels", () => {
  let app: ReturnType<typeof createApp>;
  let host: HTMLDivElement;
  let pinia: ReturnType<typeof createPinia>;

  beforeEach(() => {
    pinia = createPinia();
    setActivePinia(pinia);
    host = document.createElement("div");
    document.body.appendChild(host);
  });
  afterEach(() => {
    app?.unmount();
    host.remove();
  });

  it("associates chip, both bauds and firmware paths with their visible labels", async () => {
    const store = useBatchFlashAuthStore();
    app = createApp(BatchFlashAuthConfig).use(pinia).use(i18n);
    app.component(
      "FontAwesomeIcon",
      defineComponent({ setup: () => () => h("i") }),
    );
    app.mount(host);
    for (const id of [
      "batch-chip",
      "batch-flash-baud",
      "batch-auth-baud",
      "batch-firmware-path",
    ]) {
      const label = host.querySelector<HTMLLabelElement>(`label[for="${id}"]`);
      expect(label?.control, id).toBe(host.querySelector(`#${id}`));
      expect(label?.control, id).not.toBeNull();
    }
    store.firmwareSource = "default";
    await nextTick();
    const label = host.querySelector<HTMLLabelElement>(
      'label[for="batch-firmware-version"]',
    );
    expect(label?.control).toBe(host.querySelector("#batch-firmware-version"));
    expect(label?.control).not.toBeNull();
  });
});
