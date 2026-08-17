// @vitest-environment happy-dom
// Auto-scroll ("follow tail") behaviour and viewport virtualization of the log
// pane. happy-dom has no layout engine, so the scroll geometry of the pane
// element is stubbed and every write to scrollTop is recorded.
import { createPinia, setActivePinia } from "pinia";
import {
  createApp,
  defineComponent,
  h,
  nextTick,
  type ComponentPublicInstance,
} from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { t } = vi.hoisted(() => ({
  t: (key: string): string => key,
}));

vi.mock("vue-i18n", () => ({
  useI18n: () => ({ t }),
  createI18n: () => ({
    global: { t, te: () => true, locale: { value: "en" } },
  }),
}));

vi.mock("@/runtime", () => ({
  isTauriRuntime: () => false,
  getRuntime: () => "web",
}));

import { __setSerialDebugTransportForTest } from "@/features/serial-debug/transport";
import type { SerialDebugTransport } from "@/features/serial-debug/transport";
import { useSerialDebugStore } from "@/stores/serial-debug";
import { SerialDebugLogLineRenderer } from "@/features/serial-debug/log-line-renderer";
import SerialDebugLogView from "./SerialDebugLogView.vue";

const VIEWPORT_HEIGHT = 200;

function fakeTransport(): SerialDebugTransport {
  const noopUnsub = (): void => {};
  return {
    open: async () => {},
    close: async () => {},
    send: async () => {},
    clearSession: async () => {},
    appendSysLine: async () => {},
    addFilter: async () => {
      throw new Error("not used");
    },
    removeFilter: async () => {},
    readFilterMatches: async () => ({
      filterId: "",
      start: 0,
      items: [],
      totalMatches: 0,
    }),
    readSessionPage: async () => ({ start: 0, items: [], totalLines: 0 }),
    onChunk: () => noopUnsub,
    onChunkBatch: () => noopUnsub,
    onDisconnect: () => noopUnsub,
    onFilterUpdated: () => noopUnsub,
  } as unknown as SerialDebugTransport;
}

async function flush(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
}

describe("SerialDebugLogView auto-scroll", () => {
  let app: ReturnType<typeof createApp> | null = null;
  let host: HTMLDivElement | null = null;

  beforeEach(() => {
    setActivePinia(createPinia());
    __setSerialDebugTransportForTest(fakeTransport());
    host = document.createElement("div");
    document.body.appendChild(host);
  });

  afterEach(() => {
    app?.unmount();
    host?.remove();
    app = null;
    host = null;
    __setSerialDebugTransportForTest(null);
    vi.restoreAllMocks();
  });

  function mountComponent(hexView = false): ComponentPublicInstance {
    app = createApp(
      defineComponent({
        setup() {
          return () =>
            h(SerialDebugLogView, {
              lines: [],
              hexView,
              hexBytesPerRow: 16,
              ansiEnabled: false,
            });
        },
      }),
    );
    app.component(
      "FontAwesomeIcon",
      defineComponent({
        setup() {
          return () => h("i");
        },
      }),
    );
    return app.mount(host!);
  }

  /**
   * Give the pane a fake layout: scrollHeight comes from the virtual spacer
   * (which carries the height of the whole buffer, not of the mounted slice),
   * and every scrollTop write is recorded so we can tell "the component tried
   * to follow the tail" from "the component did nothing".
   */
  function stubGeometry(el: HTMLElement): { writes: number[] } {
    const writes: number[] = [];
    let scrollTop = 0;
    const scrollHeight = (): number =>
      Math.max(VIEWPORT_HEIGHT, spacerHeight());
    Object.defineProperty(el, "clientHeight", {
      get: () => VIEWPORT_HEIGHT,
      configurable: true,
    });
    Object.defineProperty(el, "scrollHeight", {
      get: scrollHeight,
      configurable: true,
    });
    Object.defineProperty(el, "scrollTop", {
      get: () => scrollTop,
      set: (v: number) => {
        scrollTop = Math.min(v, scrollHeight() - VIEWPORT_HEIGHT);
        writes.push(scrollTop);
      },
      configurable: true,
    });
    return { writes };
  }

  // Feed through the store's real write path. `lines` is a shallowRef, so a
  // direct `s.lines.push(...)` would mutate the array without notifying any
  // watcher — only the store publishes its own writes.
  function appendLine(
    s: ReturnType<typeof useSerialDebugStore>,
    id: number,
  ): void {
    s.appendChunk({
      direction: "rx",
      tsMs: 1_700_000_000_000 + id,
      bytes: [...Buffer.from(`L${id}\n`)],
    });
  }

  function pane(): HTMLElement {
    const el = host!.querySelector(".pane");
    if (!el) throw new Error("log pane not rendered");
    return el as HTMLElement;
  }

  /** Full virtual height of the buffer, as published by the component. */
  function spacerHeight(): number {
    const el = host!.querySelector<HTMLElement>(".virt-spacer");
    return el ? parseFloat(el.style.height) || 0 : 0;
  }

  /** The rows actually mounted (the probe row is not one of them). */
  function renderedRows(): HTMLElement[] {
    return [...host!.querySelectorAll<HTMLElement>(".line:not(.line-probe)")];
  }

  function renderedIds(): number[] {
    return renderedRows().map((el) => Number(el.dataset.lineId));
  }

  /** Open the Ctrl+F bar and type a query. */
  async function searchFor(query: string): Promise<void> {
    const container = host!.querySelector<HTMLElement>('[tabindex="0"]')!;
    container.dispatchEvent(
      new KeyboardEvent("keydown", { key: "f", ctrlKey: true }),
    );
    await flush();
    const input = host!.querySelector<HTMLInputElement>(".search-input")!;
    input.value = query;
    input.dispatchEvent(new Event("input"));
    await flush();
  }

  /**
   * Hand the component its viewport geometry. happy-dom never fires a scroll
   * event on its own, and the component reads clientHeight from the scroll
   * handler / ResizeObserver, so one synthetic scroll bootstraps the model.
   * Scrolling to the very bottom keeps auto-scroll unlocked.
   */
  async function scrollTo(el: HTMLElement, top: number): Promise<void> {
    el.scrollTop = top;
    el.dispatchEvent(new Event("scroll"));
    await flush();
  }

  it("follows new lines while the visible buffer is still growing", async () => {
    const s = useSerialDebugStore();
    mountComponent();
    await flush();
    const { writes } = stubGeometry(pane());

    for (let i = 0; i < 30; i++) appendLine(s, i);
    await flush();

    expect(writes.length).toBeGreaterThan(0);
    expect(pane().scrollTop).toBe(pane().scrollHeight - VIEWPORT_HEIGHT);
  });

  it("keeps following once the visible buffer is saturated and the line count stops changing", async () => {
    const s = useSerialDebugStore();
    // Saturated window: the store drops one line off the head for every line it
    // appends (logWindowLines cap in stores/serial-debug.ts), so the
    // array length no longer changes even though the content does.
    s.logWindowLines = 30;
    for (let i = 0; i < 30; i++) appendLine(s, i);
    mountComponent();
    await flush();
    const { writes } = stubGeometry(pane());
    writes.length = 0;

    for (let i = 30; i < 40; i++) appendLine(s, i);
    await flush();

    expect(s.lines.length).toBe(30);
    expect(writes.length).toBeGreaterThan(0);
    expect(host!.textContent).toContain("L39");
  });

  it("does not scroll while the user has scrolled up (auto-scroll locked)", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 30;
    for (let i = 0; i < 30; i++) appendLine(s, i);
    mountComponent();
    await flush();
    const el = pane();
    const { writes } = stubGeometry(el);

    // User scrolls up: the scroll handler locks auto-scroll.
    el.scrollTop = 0;
    el.dispatchEvent(new Event("scroll"));
    await flush();
    writes.length = 0;

    for (let i = 30; i < 40; i++) appendLine(s, i);
    await flush();

    expect(s.lines.length).toBe(30);
    expect(writes).toEqual([]);
  });

  it("mounts a bounded slice no matter how large the buffer gets", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 20000;
    for (let i = 0; i < 500; i++) appendLine(s, i);
    mountComponent();
    await flush();
    const el = pane();
    stubGeometry(el);
    await scrollTo(el, Number.MAX_SAFE_INTEGER);

    const rowsAt500 = renderedRows().length;
    expect(rowsAt500).toBeGreaterThan(0);
    expect(rowsAt500).toBeLessThan(80);

    for (let i = 500; i < 5000; i++) appendLine(s, i);
    await flush();

    expect(s.lines.length).toBe(5000);
    // 10x the buffer, same amount of DOM.
    expect(renderedRows().length).toBe(rowsAt500);
    expect(host!.textContent).toContain("L4999");
  });

  it("mounts the rows around the current scroll offset", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 2000;
    for (let i = 0; i < 600; i++) appendLine(s, i);
    mountComponent();
    await flush();
    const el = pane();
    stubGeometry(el);
    await scrollTo(el, Number.MAX_SAFE_INTEGER);

    const rowHeight = spacerHeight() / 600;
    expect(rowHeight).toBeGreaterThan(0);
    await scrollTo(el, 300 * rowHeight);

    const ids = renderedIds();
    expect(ids).toContain(s.lines[300].id);
    expect(ids).not.toContain(s.lines[0].id);
    expect(ids).not.toContain(s.lines[599].id);
  });

  it("scrolls a search hit outside the mounted window into view", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 2000;
    for (let i = 0; i < 600; i++) appendLine(s, i);
    mountComponent();
    await flush();
    const el = pane();
    stubGeometry(el);
    await scrollTo(el, Number.MAX_SAFE_INTEGER);
    const rowHeight = spacerHeight() / 600;

    // "L123" is far above the mounted window: finding it at all proves the
    // match scan covers the whole buffer, not just the rendered rows.
    await searchFor("L123");

    expect(el.scrollTop).toBe(123 * rowHeight);
    const current = host!.querySelectorAll(".line-search-current");
    expect(current).toHaveLength(1);
    expect((current[0] as HTMLElement).dataset.lineId).toBe(
      String(s.lines[123].id),
    );
  });

  // The whole-buffer match scan fills the renderer's per-line cache, and it
  // runs in both views (the search bar sits above the pane). The hex view never
  // mounts a single `.line`, so cache eviction must not hang off the ASCII
  // branch's render path — otherwise this combination leaks every line that
  // scrolls out of the buffer.
  it("keeps the line cache bounded in hex view while a search is active", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 50;
    const scan = vi.spyOn(
      SerialDebugLogLineRenderer.prototype,
      "matchingLineIds",
    );
    for (let i = 0; i < 50; i++) appendLine(s, i);
    mountComponent(true);
    await flush();
    await searchFor("L");

    const renderer = scan.mock.contexts[0] as SerialDebugLogLineRenderer;
    expect(renderer).toBeInstanceOf(SerialDebugLogLineRenderer);
    expect(host!.querySelectorAll(".line")).toHaveLength(0);
    expect(renderer.cacheSize()).toBe(50);

    // Roll the buffer over five times: the window still holds 50 lines.
    for (let batch = 1; batch <= 5; batch++) {
      for (let i = batch * 50; i < batch * 50 + 50; i++) appendLine(s, i);
      await flush();
    }

    expect(s.lines.length).toBe(50);
    expect(renderer.cacheSize()).toBe(50);
  });
});
