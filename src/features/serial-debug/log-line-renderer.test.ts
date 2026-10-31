// @vitest-environment node
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AnsiSpan } from "./ansi-parse";
import type { DebugLogLine } from "./types";

const { parseAnsiMock, stripAnsiMock } = vi.hoisted(() => ({
  parseAnsiMock: vi.fn((text: string): AnsiSpan[] => [
    { text, style: { fg: "#f00" } },
  ]),
  stripAnsiMock: vi.fn((text: string) => text.replace(/\x1b\[[0-9;]*m/g, "")),
}));

vi.mock("./ansi-parse", () => ({
  parseAnsi: parseAnsiMock,
  stripAnsi: stripAnsiMock,
}));

import { SerialDebugLogLineRenderer } from "./log-line-renderer";

function line(
  id: number,
  text: string,
  direction: DebugLogLine["direction"] = "rx",
): DebugLogLine {
  return {
    id,
    direction,
    tsMs: id,
    text,
    rawBytes: new Uint8Array(),
  };
}

describe("SerialDebugLogLineRenderer", () => {
  beforeEach(() => {
    parseAnsiMock.mockClear();
    stripAnsiMock.mockClear();
  });
  it("highlights whole-text matches across ANSI styles without losing text or colors", () => {
    parseAnsiMock.mockImplementationOnce(() => [
      { text: "ab", style: {} },
      { text: "cd ab", style: { fg: "red" } },
      { text: "cd!", style: { fg: "blue" } },
    ]);
    const renderer = new SerialDebugLogLineRenderer();
    const [view] = renderer.render(
      [line(1, "ab\x1b[31mcd ab\x1b[34mcd!")],
      true,
      "abcd",
    );
    expect(view.hasMatch).toBe(true);
    expect(view.spans.map((s) => s.style.fg)).toEqual([
      undefined,
      "red",
      "blue",
    ]);
    const segments = view.spans.flatMap((s) => s.segments);
    expect(segments.map((s) => s.text).join("")).toBe("abcd abcd!");
    expect(
      segments
        .filter((s) => s.isMatch)
        .map((s) => s.text)
        .join(""),
    ).toBe("abcdabcd");
  });

  it("checks only new text for a stable query while retaining window order and bounded caches", () => {
    const renderer = new SerialDebugLogLineRenderer();
    let lines = Array.from({ length: 20_000 }, (_, i) =>
      line(i, i % 7 === 0 ? "needle" : "absent"),
    );
    const includes = vi.spyOn(String.prototype, "includes");
    renderer.matchingLineIds(lines, "needle");
    includes.mockClear();
    for (let i = 0; i < 20; i++) {
      lines = [...lines.slice(1), line(20_000 + i, "needle")];
      renderer.matchingLineIds(lines, "needle");
    }
    const checks = includes.mock.calls.filter(
      ([query]) => query === "needle",
    ).length;
    includes.mockRestore();
    expect(checks).toBe(20);
    const reordered = [lines[30], lines[0], lines[lines.length - 1]];
    expect(renderer.matchingLineIds(reordered, "needle")).toEqual(
      reordered.filter((l) => l.text.includes("needle")).map((l) => l.id),
    );
    expect(renderer.cacheSize()).toBe(3);
    expect(renderer.matchingLineIds(reordered, "absent")).toEqual(
      reordered.filter((l) => l.text.includes("absent")).map((l) => l.id),
    );
    expect(renderer.matchingLineIds([], "needle")).toEqual([]);
    expect(renderer.cacheSize()).toBe(0);
  });

  it("reuses parsed ANSI/plain-text results for unchanged lines", () => {
    const renderer = new SerialDebugLogLineRenderer();
    const lines = [line(1, "\x1b[31mERR\x1b[0m one"), line(2, "OK two")];

    const first = renderer.render(lines, true, "err");
    const second = renderer.render(lines, true, "err");

    expect(first[0].hasMatch).toBe(true);
    expect(second[0].hasMatch).toBe(true);
    expect(parseAnsiMock).toHaveBeenCalledTimes(2);
    expect(stripAnsiMock).toHaveBeenCalledTimes(2);
  });

  it("drops cached entries for lines that left the buffer", () => {
    const renderer = new SerialDebugLogLineRenderer();

    renderer.render([line(1, "one"), line(2, "two")], true, "");
    expect(renderer.cacheSize()).toBe(2);

    // `render` sees only the visible slice, so rendering fewer lines must not
    // evict anything — eviction is driven by `retain` with the full buffer.
    renderer.render([line(2, "two")], true, "");
    expect(renderer.cacheSize()).toBe(2);

    renderer.retain([line(2, "two")]);
    expect(renderer.cacheSize()).toBe(1);
  });

  it("skips cache sweeps inside bounded slack and evicts by membership", () => {
    const renderer = new SerialDebugLogLineRenderer();
    const lines = Array.from({ length: 1000 }, (_, i) => line(i + 1, `L${i}`));
    renderer.matchingLineIds(lines, "never");
    expect(renderer.cacheSize()).toBe(1000);

    let idReads = 0;
    const stableWindow = lines.map((source) => ({
      ...source,
      get id() {
        idReads += 1;
        return source.id;
      },
    }));
    renderer.retain(stableWindow);
    expect(idReads).toBe(0);
    expect(renderer.cacheSize()).toBe(1000);

    // Shrinking to another history/tab window forces a sweep. Membership,
    // rather than numeric id order, decides which entries survive.
    const retained = [stableWindow[899], stableWindow[0], stableWindow[499]];
    renderer.retain(retained);
    expect(renderer.cacheSize()).toBe(3);
    expect(renderer.matchingLineIds(retained, "l")).toEqual([900, 1, 500]);
  });

  it("keeps search-off cache bounded while scrolling a small slice of a large window", () => {
    const renderer = new SerialDebugLogLineRenderer();
    const all = Array.from({ length: 20_000 }, (_, i) =>
      line(20_000 - i, `line-${i}`),
    );
    const sliceSize = 40;
    const slack = 128;

    for (let start = 0; start < all.length; start += 53) {
      const visible = all.slice(start, start + sliceSize);
      renderer.retain(visible, visible.length + slack);
      renderer.render(visible, true, "");
      expect(renderer.cacheSize()).toBeLessThanOrEqual(sliceSize * 2 + slack);
    }
  });

  it("matches over the whole buffer without building spans", () => {
    const renderer = new SerialDebugLogLineRenderer();
    parseAnsiMock.mockClear();
    const lines = [line(1, "alpha"), line(2, "beta"), line(3, "alphabet")];

    expect(renderer.matchingLineIds(lines, "  ALPHA  ")).toEqual([1, 3]);
    expect(renderer.matchingLineIds(lines, "")).toEqual([]);
    expect(parseAnsiMock).not.toHaveBeenCalled();
  });

  describe("level-prefix fallback colors (no ANSI fg)", () => {
    function mockPlainSpans(): void {
      parseAnsiMock.mockImplementationOnce((text: string) => [
        { text, style: {} },
      ]);
    }

    it.each([
      ["E (1745000) Example: operation failed", "var(--ty-danger)"],
      ["W (1745097) VoiceChatActions: idle timeout", "var(--ty-accent)"],
      ["I (1745106) SpaxSR: wake word enabled", "var(--ty-success)"],
      ["D (1745200) Example: debug detail", "var(--ty-primary)"],
      ["V (1745300) Example: verbose detail", "var(--ty-text-muted)"],
    ])("colors RX line %s with fallback fg", (text, expectedFg) => {
      mockPlainSpans();
      const renderer = new SerialDebugLogLineRenderer();
      const [view] = renderer.render([line(1, text)], true, "");
      expect(view.spans).toHaveLength(1);
      expect(view.spans[0].style.fg).toBe(expectedFg);
    });

    it("keeps device-provided ANSI fg untouched", () => {
      // default parseAnsi mock returns fg: '#f00'
      const renderer = new SerialDebugLogLineRenderer();
      const [view] = renderer.render(
        [line(1, "I (1) Tag: already colored")],
        true,
        "",
      );
      expect(view.spans[0].style.fg).toBe("#f00");
    });

    it("does not infer colors for tx or sys lines", () => {
      mockPlainSpans();
      mockPlainSpans();
      const renderer = new SerialDebugLogLineRenderer();
      const views = renderer.render(
        [line(1, "E (1) Tag: sent", "tx"), line(2, "E (2) Tag: note", "sys")],
        true,
        "",
      );
      expect(views[0].spans[0].style.fg).toBeUndefined();
      expect(views[1].spans[0].style.fg).toBeUndefined();
    });

    it("leaves RX lines without a level prefix uncolored", () => {
      mockPlainSpans();
      const renderer = new SerialDebugLogLineRenderer();
      const [view] = renderer.render([line(1, "plain output")], true, "");
      expect(view.spans[0].style.fg).toBeUndefined();
    });

    it("does not apply fallback when ANSI rendering is disabled", () => {
      mockPlainSpans();
      const renderer = new SerialDebugLogLineRenderer();
      const [view] = renderer.render([line(1, "E (1) Tag: failed")], false, "");
      expect(view.spans[0].style.fg).toBeUndefined();
    });
  });
});
