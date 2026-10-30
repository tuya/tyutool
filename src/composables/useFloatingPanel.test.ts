// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from "vue";
import { describe, expect, it } from "vitest";
import { useFloatingPanel } from "./useFloatingPanel";

describe("floating panel viewport placement", () => {
  it("flips above a bottom trigger and clamps the right edge, then repositions on resize", async () => {
    let placement!: ReturnType<typeof useFloatingPanel>;
    const visible = ref(false),
      panel = ref<HTMLElement | null>(null);
    const host = document.createElement("div");
    document.body.append(host);
    const app = createApp({
      setup() {
        placement = useFloatingPanel(
          panel,
          () => visible.value,
          () => ({
            left: window.innerWidth - 30,
            right: window.innerWidth - 10,
            top: window.innerHeight - 35,
            bottom: window.innerHeight - 10,
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
    window.dispatchEvent(new Event("resize"));
    expect(Number.parseFloat(placement.style.value.maxHeight)).toBeGreaterThan(
      180,
    );
    app.unmount();
    host.remove();
  });
});
