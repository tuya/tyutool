// @vitest-environment happy-dom
import { createApp, defineComponent, h } from "vue";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import { i18n } from "@/i18n";
import BatchAuthConfig from "./BatchAuthConfig.vue";

describe("BatchAuthConfig command prefix controls", () => {
  let app: ReturnType<typeof createApp> | undefined;
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
    app = undefined;
  });

  it("disables prefix configuration while a batch is running", () => {
    const store = useBatchFlashAuthStore();
    store.authConfig.commandPrefixMode = "custom";
    store.addPorts(["COM3"]);
    store.slots[0].status = "authorizing";

    app = createApp(BatchAuthConfig);
    app.use(pinia);
    app.use(i18n);
    app.component(
      "FontAwesomeIcon",
      defineComponent({
        setup() {
          return () => h("i");
        },
      }),
    );
    app.mount(host);

    expect(
      host.querySelector<HTMLButtonElement>("#batch-auth-command-prefix-mode")
        ?.disabled,
    ).toBe(true);
    expect(
      host.querySelector<HTMLInputElement>("#batch-auth-custom-command-prefix")
        ?.disabled,
    ).toBe(true);
  });
});
