// @vitest-environment happy-dom
import { createApp, h, nextTick, reactive } from "vue";
import { createI18n } from "vue-i18n";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import en from "@/locales/en.json";
import TyBaudRateInput from "./TyBaudRateInput.vue";

const OPTIONS = [115200, 460800, 921600, 1000000, 1500000, 2000000];

let app: ReturnType<typeof createApp> | undefined;
let host: HTMLElement;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
});

afterEach(() => {
  app?.unmount();
  document.body.replaceChildren();
  app = undefined;
});

function mount(modelValue = 460800) {
  const state = reactive({
    value: modelValue,
    disabled: false,
    validity: true,
  });
  app = createApp({
    render: () =>
      h(TyBaudRateInput, {
        id: "baud-rate",
        modelValue: state.value,
        options: OPTIONS,
        disabled: state.disabled,
        "onUpdate:modelValue": (value: number) => {
          state.value = value;
        },
        onValidityChange: (valid: boolean) => {
          state.validity = valid;
        },
      }),
  });
  app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
  app.mount(host);

  const input = host.querySelector<HTMLInputElement>("#baud-rate")!;
  return { state, input };
}

async function flush() {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
}

function inputText(input: HTMLInputElement, value: string) {
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function key(input: HTMLInputElement, value: string) {
  return input.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: value,
      bubbles: true,
      cancelable: true,
    }),
  );
}

function mouse(target: Element) {
  return target.dispatchEvent(
    new MouseEvent("mousedown", { bubbles: true, cancelable: true }),
  );
}

describe("TyBaudRateInput", () => {
  it("uses the unified u32 bounds, maxlength, and localized common labels", async () => {
    const { state, input } = mount();
    expect(input.getAttribute("aria-label")).toBe(en.common.baud.label);
    expect(input.getAttribute("maxlength")).toBe("10");
    expect(
      document.getElementById(input.getAttribute("aria-describedby")!)
        ?.textContent,
    ).toContain(en.common.baud.inputHint);

    inputText(input, "9216x00");
    await flush();
    expect(input.value).toBe("9216x00");
    expect(state.value).toBe(460800);
    expect(state.validity).toBe(false);
    expect(input.getAttribute("aria-invalid")).toBe("true");

    inputText(input, "921600");
    await flush();
    expect(input.value).toBe("921600");
    expect(state.value).toBe(921600);
    expect(input.getAttribute("aria-invalid")).toBe("false");

    for (const invalidValue of ["-115200", "1.5"]) {
      inputText(input, invalidValue);
      await flush();
      expect(input.value).toBe(invalidValue);
      expect(state.value).toBe(921600);
      expect(state.validity).toBe(false);
      expect(input.getAttribute("aria-invalid")).toBe("true");
    }

    inputText(input, "4000001");
    await flush();
    expect(state.value).toBe(4000001);
    expect(state.validity).toBe(true);

    inputText(input, "4294967296");
    await flush();
    expect(state.value).toBe(4000001);
    expect(state.validity).toBe(false);
  });

  it("accepts the u32 boundaries and rejects the values around them", async () => {
    const { state, input } = mount(115200);
    expect(input.getAttribute("maxlength")).toBe("10");

    inputText(input, "4294967295");
    await flush();
    expect(state.value).toBe(4_294_967_295);
    expect(state.validity).toBe(true);

    inputText(input, "4294967296");
    await flush();
    expect(state.value).toBe(4_294_967_295);
    expect(state.validity).toBe(false);

    inputText(input, "1");
    await flush();
    expect(state.value).toBe(1);
    expect(state.validity).toBe(true);

    inputText(input, "0");
    await flush();
    expect(state.value).toBe(1);
    expect(state.validity).toBe(false);
  });

  it("restores the last valid value and gives localized feedback on invalid blur or Enter", async () => {
    const { state, input } = mount();

    inputText(input, "0");
    input.dispatchEvent(new FocusEvent("blur", { bubbles: true }));
    await flush();
    expect(state.value).toBe(460800);
    expect(input.value).toBe("460800");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      en.common.baud.invalid,
    );

    inputText(input, "4294967296");
    key(input, "Enter");
    await flush();
    expect(state.value).toBe(460800);
    expect(input.value).toBe("460800");
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      en.common.baud.invalid,
    );
  });

  it("opens common values from the arrow, selects one, and closes outside", async () => {
    const { state, input } = mount();
    const arrow = host.querySelector<HTMLButtonElement>(".ty-baud-arrow")!;

    arrow.click();
    await flush();
    expect(input.getAttribute("aria-expanded")).toBe("true");
    const listbox = document.querySelector<HTMLElement>('[role="listbox"]')!;
    expect(listbox.textContent).not.toContain("Custom");
    expect(listbox.querySelectorAll('[role="option"]')).toHaveLength(
      OPTIONS.length,
    );

    mouse(listbox.querySelectorAll('[role="option"]')[2]);
    await flush();
    expect(state.value).toBe(921600);
    expect(input.value).toBe("921600");
    expect(document.querySelector('[role="listbox"]')).toBeNull();

    arrow.click();
    await flush();
    mouse(document.body);
    await flush();
    expect(document.querySelector('[role="listbox"]')).toBeNull();
  });

  it("supports arrow-key navigation, Enter, Escape and Tab", async () => {
    const { state, input } = mount();
    input.focus();

    inputText(input, "921600");
    await flush();
    expect(state.value).toBe(921600);
    key(input, "Escape");
    await flush();
    expect(input.value).toBe("460800");
    expect(state.value).toBe(460800);

    key(input, "ArrowDown");
    await flush();
    expect(document.querySelector('[role="listbox"]')).not.toBeNull();
    expect(input.getAttribute("aria-controls")).toBeTruthy();
    expect(input.getAttribute("aria-activedescendant")).toBeTruthy();

    key(input, "ArrowDown");
    key(input, "Enter");
    await flush();
    expect(state.value).toBe(921600);

    key(input, "ArrowUp");
    await flush();
    key(input, "ArrowUp");
    key(input, "Enter");
    await flush();
    expect(state.value).toBe(460800);

    inputText(input, "0");
    key(input, "Escape");
    await flush();
    expect(input.value).toBe("460800");
    expect(state.value).toBe(460800);

    key(input, "ArrowDown");
    await flush();
    key(input, "Tab");
    await flush();
    expect(document.querySelector('[role="listbox"]')).toBeNull();
  });

  it("syncs external model changes, reverts Escape drafts, and closes when disabled", async () => {
    const { state, input } = mount();

    state.value = 1500000;
    await flush();
    expect(input.value).toBe("1500000");

    inputText(input, "0");
    key(input, "Escape");
    await flush();
    expect(input.value).toBe("1500000");
    expect(state.value).toBe(1500000);

    key(input, "ArrowDown");
    await flush();
    expect(document.querySelector('[role="listbox"]')).not.toBeNull();
    state.disabled = true;
    await flush();
    expect(document.querySelector('[role="listbox"]')).toBeNull();
    expect(input.disabled).toBe(true);
  });
});
