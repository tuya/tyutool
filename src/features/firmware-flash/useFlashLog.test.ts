// @vitest-environment happy-dom
import { createApp, h, nextTick } from "vue";
import { describe, expect, it, vi } from "vitest";
import { useFlashLog } from "./useFlashLog";

describe("flash log tail following", () => {
  it("follows a synchronous log burst once after the DOM flush", async () => {
    let log!: ReturnType<typeof useFlashLog>;
    const host = document.createElement("div");
    const app = createApp({
      setup() {
        log = useFlashLog();
        return () => h("div");
      },
    });
    app.mount(host);
    const el = document.createElement("div");
    const height = vi.fn(() => 1000);
    const scroll = vi.fn();
    Object.defineProperties(el, {
      scrollHeight: { get: height },
      scrollTop: { set: scroll },
    });
    log.logScrollRef.value = el;
    for (let i = 0; i < 100; i++) log.appendLog(String(i));
    await nextTick();
    await nextTick();
    expect(log.logLines.value[log.logLines.value.length - 1]).toContain("99");
    expect(height).toHaveBeenCalledOnce();
    expect(scroll).toHaveBeenCalledExactlyOnceWith(1000);
    app.unmount();
  });

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
