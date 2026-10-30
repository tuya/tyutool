// @vitest-environment happy-dom
import { createApp, h, nextTick } from "vue";
import { describe, expect, it } from "vitest";
import { useFlashLog } from "./useFlashLog";

describe("flash log tail following", () => {
  it("continues following at the buffer cap while respecting the scroll lock", async () => {
    let log!: ReturnType<typeof useFlashLog>;
    const host = document.createElement("div");
    document.body.append(host);
    const app = createApp({
      setup() {
        log = useFlashLog();
        return () => h("div");
      },
    });
    app.mount(host);
    const el = document.createElement("div");
    Object.defineProperty(el, "scrollHeight", {
      configurable: true,
      value: 1000,
    });
    log.logScrollRef.value = el;
    for (let i = 0; i < 500; i++) log.appendLog(String(i));
    await nextTick();
    await nextTick();
    el.scrollTop = 0;
    Object.defineProperty(el, "scrollHeight", {
      configurable: true,
      value: 1500,
    });
    log.appendLog("New tail");
    await nextTick();
    await nextTick();
    expect(log.logLines.value).toHaveLength(500);
    expect(el.scrollTop).toBe(1500);
    log.lockAutoScroll.value = true;
    el.scrollTop = 40;
    log.appendLog("Locked");
    await nextTick();
    await nextTick();
    expect(el.scrollTop).toBe(40);
    app.unmount();
    host.remove();
  });
});
