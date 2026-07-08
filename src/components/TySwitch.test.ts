// @vitest-environment happy-dom
import { createApp, defineComponent, h, nextTick, ref } from "vue";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import TySwitch from "./TySwitch.vue";

describe("TySwitch", () => {
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

  it("toggles through v-model on click and keyboard", async () => {
    const value = ref(false);

    app = createApp(
      defineComponent({
        setup() {
          return () =>
            h(TySwitch, {
              modelValue: value.value,
              "onUpdate:modelValue": (nextValue: boolean) => {
                value.value = nextValue;
              },
              "aria-label": "Test switch",
            });
        },
      }),
    );
    app.mount(host!);
    await nextTick();

    const button = host!.querySelector(
      'button[role="switch"]',
    ) as HTMLButtonElement | null;
    expect(button?.getAttribute("aria-checked")).toBe("false");

    button?.click();
    await nextTick();
    expect(value.value).toBe(true);
    expect(button?.getAttribute("aria-checked")).toBe("true");

    button?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await nextTick();
    expect(value.value).toBe(false);

    button?.dispatchEvent(new KeyboardEvent("keydown", { key: " " }));
    await nextTick();
    expect(value.value).toBe(true);
  });

  it("does not emit updates while disabled", async () => {
    const value = ref(false);

    app = createApp(
      defineComponent({
        setup() {
          return () =>
            h(TySwitch, {
              modelValue: value.value,
              disabled: true,
              "onUpdate:modelValue": (nextValue: boolean) => {
                value.value = nextValue;
              },
              "aria-label": "Disabled switch",
            });
        },
      }),
    );
    app.mount(host!);
    await nextTick();

    const button = host!.querySelector(
      'button[role="switch"]',
    ) as HTMLButtonElement | null;
    expect(button?.hasAttribute("disabled")).toBe(true);

    button?.click();
    button?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await nextTick();

    expect(value.value).toBe(false);
    expect(button?.getAttribute("aria-checked")).toBe("false");
  });
});
