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
  ref,
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
import { formatHexDumpFromChunks } from "@/features/serial-debug/hex-format";
import type {
  DebugLogLine,
  HexBytesPerRow,
} from "@/features/serial-debug/types";
import SerialDebugLogView from "./SerialDebugLogView.vue";

const VIEWPORT_HEIGHT = 200;
// Mirrors the component: overscan rows above/below the viewport, and the
// vertical inset of the hex dump inside its pane.
const OVERSCAN_ROWS = 10;
const HEX_DUMP_INSET_Y = 12;

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
  // Refs, not mount arguments: the hex tests change these after mounting.
  const hexBytesPerRow = ref<HexBytesPerRow>(16);
  const hexViewProp = ref(false);

  beforeEach(() => {
    setActivePinia(createPinia());
    __setSerialDebugTransportForTest(fakeTransport());
    hexBytesPerRow.value = 16;
    hexViewProp.value = false;
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
    hexViewProp.value = hexView;
    app = createApp(
      defineComponent({
        setup() {
          return () =>
            h(SerialDebugLogView, {
              lines: [],
              hexView: hexViewProp.value,
              hexBytesPerRow: hexBytesPerRow.value,
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

  /** The hex dump text actually mounted (the probe row is not part of it). */
  function mountedDump(): string {
    const el = host!.querySelector<HTMLElement>(".virt-window pre");
    if (!el) throw new Error("hex dump not rendered");
    return el.textContent ?? "";
  }

  /**
   * The dump the whole visible buffer would produce — the pre-virtualization
   * output. Every mounted window must be a verbatim row-aligned slice of it,
   * which is what keeps the hex view one continuous dump.
   */
  function fullDumpRows(
    lines: readonly DebugLogLine[],
    bytesPerRow: HexBytesPerRow = hexBytesPerRow.value,
  ): string[] {
    const chunks = lines.map((l) =>
      Uint8Array.from([
        ...(l.rawBytes ?? new TextEncoder().encode(l.text)),
        0x0a,
      ]),
    );
    const dump = formatHexDumpFromChunks(chunks, bytesPerRow);
    return dump === "" ? [] : dump.split("\n");
  }

  /** Row index the mounted window starts at, verifying it is a verbatim slice. */
  function mountedWindowStartRow(rows: string[]): number {
    const dump = rows.join("\n");
    const at = dump.indexOf(mountedDump());
    expect(at).toBeGreaterThanOrEqual(0);
    expect(at === 0 || dump[at - 1] === "\n").toBe(true);
    return at === 0 ? 0 : dump.slice(0, at).split("\n").length - 1;
  }

  function hexRowHeight(rows: number): number {
    // contentHeight = rows * rowHeight + 2 * inset (happy-dom has no layout, so
    // the component falls back to its font-size estimate).
    return (spacerHeight() - 2 * HEX_DUMP_INSET_Y) / rows;
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

  // ── hex view virtualization ───────────────────────────────────────────
  // The hex view is one continuous dump of the buffer's byte stream, so its rows
  // are byte ranges, not log lines. What is asserted below is that the mounted
  // window stays a verbatim, correctly positioned slice of that dump (visual
  // output unchanged) while its size stays bounded.

  it("mounts a bounded hex window no matter how large the buffer gets", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 20000;
    for (let i = 0; i < 500; i++) appendLine(s, i);
    mountComponent(true);
    await flush();
    const el = pane();
    stubGeometry(el);
    await scrollTo(el, Number.MAX_SAFE_INTEGER);

    const dumpAt500 = mountedDump();
    const rowsAt500 = dumpAt500.split("\n").length;
    expect(rowsAt500).toBeGreaterThan(0);
    expect(rowsAt500).toBeLessThan(80);
    // The whole buffer is far bigger than what is mounted.
    expect(fullDumpRows(s.lines).length).toBeGreaterThan(3 * rowsAt500);

    for (let i = 500; i < 5000; i++) appendLine(s, i);
    await flush();

    expect(s.lines.length).toBe(5000);
    const rows = fullDumpRows(s.lines);
    // 10x the buffer, same amount of DOM …
    expect(mountedDump().split("\n").length).toBe(rowsAt500);
    // … still following the tail, and still a verbatim slice of the full dump.
    expect(mountedDump().endsWith(rows[rows.length - 1])).toBe(true);
    expect(mountedWindowStartRow(rows)).toBe(rows.length - rowsAt500);
  });

  it("mounts the hex rows around the current scroll offset", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 2000;
    for (let i = 0; i < 600; i++) appendLine(s, i);
    mountComponent(true);
    await flush();
    const el = pane();
    stubGeometry(el);
    await scrollTo(el, Number.MAX_SAFE_INTEGER);

    const rows = fullDumpRows(s.lines);
    const rowHeight = hexRowHeight(rows.length);
    expect(rowHeight).toBeGreaterThan(0);

    const targetRow = Math.floor(rows.length / 2);
    await scrollTo(el, targetRow * rowHeight + HEX_DUMP_INSET_Y);

    const startRow = mountedWindowStartRow(rows);
    expect(startRow).toBe(targetRow - OVERSCAN_ROWS);
    // Neither end of the buffer is mounted.
    expect(mountedDump()).not.toContain(rows[0]);
    expect(mountedDump()).not.toContain(rows[rows.length - 1]);
  });

  it("re-cuts the hex row model when bytesPerRow changes", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 2000;
    for (let i = 0; i < 400; i++) appendLine(s, i);
    mountComponent(true);
    await flush();
    const el = pane();
    stubGeometry(el);
    await scrollTo(el, Number.MAX_SAFE_INTEGER);

    const rowsAt16 = fullDumpRows(s.lines, 16).length;
    expect(hexRowHeight(rowsAt16)).toBeGreaterThan(0);

    hexBytesPerRow.value = 8;
    await flush();

    // Half as many bytes per row means twice as many rows to scroll through
    // (minus one when the final 16-byte row was already a partial one).
    const rows = fullDumpRows(s.lines, 8);
    expect(rows.length).toBeGreaterThanOrEqual(2 * rowsAt16 - 1);
    expect(rows.length).toBeLessThanOrEqual(2 * rowsAt16);
    expect(hexRowHeight(rows.length)).toBeGreaterThan(0);
    // The index survived the switch: the window is still a slice of the *new*
    // dump, and auto-scroll still sits on the last row.
    expect(mountedWindowStartRow(rows)).toBeGreaterThan(0);
    expect(mountedDump().endsWith(rows[rows.length - 1])).toBe(true);
  });

  it("keeps following the tail in hex view and stops when scrolled up", async () => {
    const s = useSerialDebugStore();
    // Saturated window: every append drops one line off the head, so the byte
    // stream re-bases under the viewport on every batch.
    s.logWindowLines = 200;
    for (let i = 0; i < 200; i++) appendLine(s, i);
    mountComponent(true);
    await flush();
    const el = pane();
    const { writes } = stubGeometry(el);
    await scrollTo(el, Number.MAX_SAFE_INTEGER);
    writes.length = 0;

    for (let i = 200; i < 210; i++) appendLine(s, i);
    await flush();

    expect(s.lines.length).toBe(200);

    expect(writes.length).toBeGreaterThan(0);
    const rows = fullDumpRows(s.lines);
    expect(mountedDump().endsWith(rows[rows.length - 1])).toBe(true);

    // User scrolls up: no more scrollTop writes, and the head of the buffer is
    // what is mounted.
    await scrollTo(el, 0);
    writes.length = 0;
    for (let i = 210; i < 220; i++) appendLine(s, i);
    await flush();

    expect(writes).toEqual([]);
    expect(mountedWindowStartRow(fullDumpRows(s.lines))).toBe(0);
  });

  it("re-measures the hex row height when the font size changes", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 2000;
    for (let i = 0; i < 300; i++) appendLine(s, i);
    mountComponent(true);
    await flush();
    const el = pane();
    stubGeometry(el);
    await scrollTo(el, Number.MAX_SAFE_INTEGER);

    const rows = fullDumpRows(s.lines);
    const before = hexRowHeight(rows.length);

    s.logFontSize = 18;
    await flush();

    // Taller rows, same row count: the byte index does not depend on layout.
    expect(hexRowHeight(rows.length)).toBeGreaterThan(before);
    expect(fullDumpRows(s.lines).length).toBe(rows.length);
    expect(mountedDump().endsWith(rows[rows.length - 1])).toBe(true);
  });

  it("renders an archive-backed filter tab in hex view", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 2000;
    for (let i = 0; i < 100; i++) appendLine(s, i);
    mountComponent(true);
    await flush();
    stubGeometry(pane());
    await scrollTo(pane(), Number.MAX_SAFE_INTEGER);

    // Filter tabs are fed from the Rust session archive: text only, no
    // rawBytes, so the byte index has to re-encode. The first line stands in
    // for the localized archive-cap sentinel — non-ASCII text, several UTF-8
    // bytes per character.
    const items: DebugLogLine[] = Array.from({ length: 400 }, (_, i) => ({
      id: 100_000 + i,
      tsMs: i,
      direction: i === 0 ? "sys" : "rx",
      text: i === 0 ? "日志归档已达上限 256 MiB" : `match-${i}`,
    }));
    s.filterPagesById = {
      chipA: { filterId: "chipA", totalMatches: 400, start: 0, items },
    };
    s.activeChipId = "chipA";
    await flush();
    stubGeometry(pane());
    await scrollTo(pane(), Number.MAX_SAFE_INTEGER);

    const rows = fullDumpRows(items);
    expect(rows.length).toBeGreaterThan(100);
    expect(mountedDump().split("\n").length).toBeLessThan(80);
    expect(mountedDump().endsWith(rows[rows.length - 1])).toBe(true);

    // The head of that tab, including the non-ASCII sys line.
    await scrollTo(pane(), 0);
    expect(mountedWindowStartRow(rows)).toBe(0);
    expect(mountedDump().startsWith(rows[0])).toBe(true);
  });

  it("switches between the hex and line panes without losing the row model", async () => {
    const s = useSerialDebugStore();
    s.logWindowLines = 2000;
    for (let i = 0; i < 300; i++) appendLine(s, i);
    mountComponent(true);
    await flush();
    stubGeometry(pane());
    await scrollTo(pane(), Number.MAX_SAFE_INTEGER);
    const hexSpacer = spacerHeight();
    expect(hexSpacer).toBeGreaterThan(0);

    // hexView is a prop; flip it on the wrapper. v-if/v-else swaps the pane
    // element, so the geometry stub has to be re-applied.
    hexViewProp.value = false;
    await flush();
    stubGeometry(pane());
    await scrollTo(pane(), Number.MAX_SAFE_INTEGER);

    expect(host!.querySelector(".virt-window pre")).toBeNull();
    expect(renderedRows().length).toBeGreaterThan(0);
    // The line row model is in charge now: 300 lines, bounded DOM, at the tail.
    expect(renderedRows().length).toBeLessThan(80);
    expect(renderedIds()).toContain(s.lines[299].id);

    hexViewProp.value = true;
    await flush();
    stubGeometry(pane());
    await scrollTo(pane(), Number.MAX_SAFE_INTEGER);

    const rows = fullDumpRows(s.lines);
    expect(mountedDump().endsWith(rows[rows.length - 1])).toBe(true);
    expect(mountedDump().split("\n").length).toBeLessThan(80);
  });
});
