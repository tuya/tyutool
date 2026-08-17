// @vitest-environment node
import { describe, expect, it, vi } from "vitest";
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
