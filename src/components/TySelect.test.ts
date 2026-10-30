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
  it("supports first/last, closed ArrowUp and prefix search without selecting disabled options", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const update = vi.fn();
    app = createApp({
      render: () =>
        h(TySelect, {
          modelValue: "a",
          options: [
            { value: "x", label: "Excluded", disabled: true },
            { value: "a", label: "Alpha" },
            { value: "b", label: "Beta", optionTooltip: "Port purpose" },
            { value: "c", label: "Charlie" },
            { value: "z", label: "Zulu", disabled: true },
          ],
          "onUpdate:modelValue": update,
        }),
    });
    app.mount(host);
    const trigger = host.querySelector("button")!;
    const key = async (k: string) => {
      trigger.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: k,
          bubbles: true,
          cancelable: true,
        }),
      );
      await nextTick();
      await nextTick();
    };
    const active = () =>
      document.getElementById(trigger.getAttribute("aria-activedescendant")!)
        ?.textContent;
    await key("ArrowUp");
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    await key("End");
    expect(active()).toContain("Charlie");
    await key("Home");
    expect(active()).toContain("Alpha");
    await key("b");
    expect(active()).toContain("Beta");
    expect(document.querySelector('[role="tooltip"]')?.textContent).toContain(
      "Port purpose",
    );
    expect(update).not.toHaveBeenCalled();
    await key("Enter");
    expect(update).toHaveBeenCalledWith("b");
  });
});
