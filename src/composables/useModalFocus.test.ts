// @vitest-environment happy-dom
import { createApp, h, nextTick, ref, type VNode } from "vue";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useModalFocus } from "./useModalFocus";

describe("useModalFocus lifecycle", () => {
  let app: ReturnType<typeof createApp>;
  let host: HTMLElement;
  let opener: HTMLButtonElement;
  const open = ref(false);

  beforeEach(() => {
    open.value = false;
    host = document.createElement("div");
    opener = document.createElement("button");
    document.body.append(opener, host);
    opener.focus();
  });
  afterEach(async () => {
    app.unmount();
    await nextTick();
    document.body.replaceChildren();
  });

  async function settle() {
    await nextTick();
    await nextTick();
  }

  function mount(content: () => VNode[]) {
    app = createApp({
      setup() {
        const dialog = ref<HTMLElement | null>(null);
        useModalFocus(
          dialog,
          () => open.value,
          () => {
            open.value = false;
          },
        );
        return () =>
          open.value
            ? h("div", { ref: dialog, role: "dialog", tabindex: -1 }, content())
            : null;
      },
    });
    app.mount(host);
  }

  it("skips disabled, hidden and negative-tabindex controls and cycles visible endpoints", async () => {
    mount(() => [
      h("button", { disabled: true }, "Disabled"),
      h("button", { hidden: true }, "Hidden"),
      h("div", { style: "display: none" }, [h("button", "Hidden ancestor")]),
      h("button", { tabindex: -1 }, "Not tabbable"),
      h("a", { href: "#first" }, "First"),
      h("textarea"),
    ]);
    open.value = true;
    await settle();
    const first = host.querySelector("a")!;
    const last = host.querySelector("textarea")!;
    expect(document.activeElement).toBe(first);
    const ordinaryTab = new KeyboardEvent("keydown", {
      key: "Tab",
      bubbles: true,
      cancelable: true,
    });
    first.dispatchEvent(ordinaryTab);
    expect(ordinaryTab.defaultPrevented).toBe(false);
    first.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).toBe(last);
    last.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).toBe(first);
  });

  it("keeps focus on the dialog when no enabled controls are available", async () => {
    mount(() => [h("button", { disabled: true }, "Busy")]);
    open.value = true;
    await settle();
    const dialog = host.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(document.activeElement).toBe(dialog);
    const tab = new KeyboardEvent("keydown", {
      key: "Tab",
      bubbles: true,
      cancelable: true,
    });
    dialog.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(dialog);
  });

  it("redirects programmatic background focus and restores existing inert values on close", async () => {
    const background = document.createElement("div");
    background.setAttribute("inert", "already-inert");
    document.body.append(background);
    mount(() => [h("button", "Close")]);
    open.value = true;
    await settle();
    const button = host.querySelector("button")!;
    opener.focus();
    expect(document.activeElement).toBe(button);
    const newBackground = document.createElement("button");
    document.body.append(newBackground);
    await settle();
    expect(newBackground.hasAttribute("inert")).toBe(true);
    open.value = false;
    await settle();
    expect(document.activeElement).toBe(opener);
    expect(background.getAttribute("inert")).toBe("already-inert");
    expect(newBackground.hasAttribute("inert")).toBe(false);
  });

  it("cancels queued focus on immediate unmount and removes all listeners", async () => {
    mount(() => [h("button", "Close")]);
    open.value = true;
    await nextTick();
    app.unmount();
    await settle();
    expect(document.activeElement).toBe(opener);
    const event = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    opener.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(document.querySelector("[inert]")).toBeNull();
  });

  it("unwinds child focus and inert state, then restores the page opener when the whole tree closes", async () => {
    const childOpen = ref(false);
    app = createApp({
      setup() {
        const parent = ref<HTMLElement | null>(null);
        const child = ref<HTMLElement | null>(null);
        useModalFocus(
          parent,
          () => open.value,
          () => {
            open.value = false;
          },
        );
        useModalFocus(
          child,
          () => open.value && childOpen.value,
          () => {
            childOpen.value = false;
          },
        );
        return () => [
          open.value
            ? h("div", { ref: parent, class: "parent-modal", tabindex: -1 }, [
                h(
                  "button",
                  {
                    onClick: () => {
                      childOpen.value = true;
                    },
                  },
                  "Open child",
                ),
              ])
            : null,
          open.value && childOpen.value
            ? h("div", { ref: child, class: "child-modal", tabindex: -1 }, [
                h("button", "Child"),
              ])
            : null,
        ];
      },
    });
    app.mount(host);
    open.value = true;
    await settle();
    const parent = host.querySelector<HTMLElement>(".parent-modal")!;
    const childOpener = parent.querySelector("button")!;
    childOpener.click();
    await settle();
    expect(document.activeElement?.textContent).toBe("Child");
    expect(parent.hasAttribute("inert")).toBe(true);
    expect(host.querySelector(".child-modal")!.closest("[inert]")).toBeNull();
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await settle();
    expect(open.value).toBe(true);
    expect(childOpen.value).toBe(false);
    expect(document.activeElement).toBe(childOpener);
    expect(parent.hasAttribute("inert")).toBe(false);
    expect(opener.hasAttribute("inert")).toBe(true);
    childOpener.click();
    await settle();
    open.value = false;
    await settle();
    expect(document.activeElement).toBe(opener);
    expect(document.querySelector("[inert]")).toBeNull();
  });
});
