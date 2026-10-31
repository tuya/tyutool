// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from "vue";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useFloatingPanel } from "./useFloatingPanel";

let app: ReturnType<typeof createApp> | undefined;
afterEach(() => {
  app?.unmount();
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

describe("floating panel viewport placement", () => {
  async function mountPanel() {
    const visible = ref(false);
    const panel = ref<HTMLElement | null>(null);
    let bottom = 140;
    const anchor = vi.fn(() => ({ left: 20, right: 120, top: 100, bottom }));
    let placement!: ReturnType<typeof useFloatingPanel>;
    const host = document.createElement("div");
    document.body.append(host);
    app = createApp({
      setup() {
        placement = useFloatingPanel(panel, () => visible.value, anchor);
        return () =>
          visible.value ? h("div", { ref: panel }, h("div")) : null;
      },
    });
    app.mount(host);
    visible.value = true;
    await nextTick();
    await nextTick();
    anchor.mockClear();
    return {
      visible,
      panel,
      placement,
      anchor,
      move: () => {
        bottom = 180;
      },
    };
  }

  it("does not reposition while scrolling inside its own panel", async () => {
    const { panel, anchor } = await mountPanel();
    panel.value!.dispatchEvent(new Event("scroll"));
    panel.value!.firstElementChild!.dispatchEvent(new Event("scroll"));
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    expect(anchor).not.toHaveBeenCalled();
  });

  it("measures once per frame and cancels pending work on close and unmount", async () => {
    const { visible, placement, anchor, move } = await mountPanel();
    let callback: FrameRequestCallback | undefined;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((fn) => {
      callback = fn;
      return 7;
    });
    const cancel = vi.spyOn(window, "cancelAnimationFrame");
    const scroll = () => window.dispatchEvent(new Event("scroll"));
    for (let i = 0; i < 100; i++) {
      scroll();
      window.dispatchEvent(new Event("resize"));
    }
    expect(anchor).not.toHaveBeenCalled();
    move();
    callback!(0);
    expect(anchor).toHaveBeenCalledOnce();
    expect(placement.style.value.top).toBe("184px");
    scroll();
    visible.value = false;
    await nextTick();
    expect(cancel).toHaveBeenCalledWith(7);
    anchor.mockClear();
    scroll();
    expect(anchor).not.toHaveBeenCalled();
    visible.value = true;
    await nextTick();
    await nextTick();
    scroll();
    cancel.mockClear();
    app!.unmount();
    app = undefined;
    expect(cancel).toHaveBeenCalledWith(7);
  });

  it("flips above a bottom trigger and clamps the right edge, then repositions on resize", async () => {
    let placement!: ReturnType<typeof useFloatingPanel>;
    let anchorOffset = 0;
    const visible = ref(false),
      panel = ref<HTMLElement | null>(null);
    const host = document.createElement("div");
    document.body.append(host);
    app = createApp({
      setup() {
        placement = useFloatingPanel(
          panel,
          () => visible.value,
          () => ({
            left: window.innerWidth - 30,
            right: window.innerWidth - 10,
            top: window.innerHeight - 35 - anchorOffset,
            bottom: window.innerHeight - 10 - anchorOffset,
          }),
        );
        return () => (visible.value ? h("div", { ref: panel }) : null);
      },
    });
    app.mount(host);
    visible.value = true;
    await nextTick();
    Object.defineProperties(panel.value!, {
      scrollHeight: { value: 180 },
      offsetWidth: { value: 250 },
      offsetHeight: { value: 182 },
      clientHeight: { value: 180 },
    });
    placement.position();
    expect(Number.parseFloat(placement.style.value.left)).toBe(
      window.innerWidth - 258,
    );
    expect(Number.parseFloat(placement.style.value.top)).toBe(
      window.innerHeight - 221,
    );
    anchorOffset = 50;
    window.dispatchEvent(new Event("resize"));
    await vi.waitFor(() =>
      expect(Number.parseFloat(placement.style.value.top)).toBe(
        window.innerHeight - 271,
      ),
    );
    expect(Number.parseFloat(placement.style.value.maxHeight)).toBeGreaterThan(
      180,
    );
  });
});
