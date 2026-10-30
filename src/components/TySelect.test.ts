// @vitest-environment happy-dom
import { createApp, h, nextTick } from "vue";
import { afterEach, describe, expect, it, vi } from "vitest";
import TySelect from "./TySelect.vue";

let app: ReturnType<typeof createApp> | undefined;
afterEach(() => {
  app?.unmount();
  document.body.innerHTML = "";
});

describe("TySelect keyboard semantics", () => {
  it("names the combobox, refreshes on keyboard open and announces the active option", async () => {
    const onOpen = vi.fn();
    const onUpdate = vi.fn();
    const host = document.createElement("div");
    document.body.append(host);
    app = createApp({
      render: () =>
        h(TySelect, {
          modelValue: "",
          ariaLabel: "Chip",
          onOpen,
          "onUpdate:modelValue": onUpdate,
          options: [
            { value: "", label: "Choose chip", disabled: true },
            { value: "T5AI", label: "T5AI" },
            { value: "ESP32", label: "ESP32" },
          ],
        }),
    });
    app.mount(host);
    const trigger = host.querySelector("button")!;
    expect(trigger.getAttribute("role")).toBe("combobox");
    expect(trigger.getAttribute("aria-label")).toBe("Chip");
    trigger.focus();
    trigger.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowDown",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    expect(onOpen).toHaveBeenCalledOnce();
    const list = document.getElementById(
      trigger.getAttribute("aria-controls")!,
    )!;
    expect(list.getAttribute("role")).toBe("listbox");
    const option = () =>
      document.getElementById(trigger.getAttribute("aria-activedescendant")!)!;
    expect(option().textContent).toContain("T5AI");
    trigger.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowDown",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    expect(option().textContent).toContain("ESP32");
    trigger.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    expect(onUpdate).toHaveBeenCalledWith("ESP32");
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
  });
});
