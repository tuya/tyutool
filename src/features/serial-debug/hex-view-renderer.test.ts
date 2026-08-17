// @vitest-environment node
import { describe, expect, it } from "vitest";
import type { DebugLogLine, HexBytesPerRow } from "./types";
import { formatHexDumpFromChunks } from "./hex-format";
import { SerialDebugHexViewRenderer } from "./hex-view-renderer";

function line(id: number, rawBytes?: number[]): DebugLogLine {
  return {
    id,
    direction: "rx",
    tsMs: id,
    text: `line-${id}`,
    rawBytes: rawBytes ? Uint8Array.from(rawBytes) : undefined,
  };
}

/** Bytes a line contributes to the dump stream: its own plus the line feed. */
function chunkOf(l: DebugLogLine): Uint8Array {
  const raw = l.rawBytes ?? new TextEncoder().encode(l.text);
  return Uint8Array.from([...raw, 0x0a]);
}

/** The dump the pre-virtualization renderer produced for the whole buffer. */
function fullDump(
  lines: readonly DebugLogLine[],
  bytesPerRow: HexBytesPerRow,
): string[] {
  const dump = formatHexDumpFromChunks(lines.map(chunkOf), bytesPerRow);
  return dump === "" ? [] : dump.split("\n");
}

function buffer(count: number, bytesPerLine = 7): DebugLogLine[] {
  return Array.from({ length: count }, (_, i) =>
    line(
      i + 1,
      Array.from({ length: bytesPerLine }, (_, j) => 0x30 + ((i + j) % 0x40)),
    ),
  );
}

describe("SerialDebugHexViewRenderer", () => {
  it("counts hex rows from the byte stream, not from the line count", () => {
    const renderer = new SerialDebugHexViewRenderer();
    // 10 lines x (7 bytes + 1 line feed) = 80 bytes.
    const lines = buffer(10);

    expect(renderer.rowCount(lines, 8)).toBe(10);
    expect(renderer.rowCount(lines, 16)).toBe(5);
    expect(renderer.rowCount(lines, 32)).toBe(3);
    expect(renderer.rowCount([], 16)).toBe(0);
  });

  it("renders a window byte-identical to the same rows of the full dump", () => {
    const renderer = new SerialDebugHexViewRenderer();
    const lines = buffer(200);

    for (const bytesPerRow of [8, 16, 32] as const) {
      const expected = fullDump(lines, bytesPerRow);
      expect(renderer.rowCount(lines, bytesPerRow)).toBe(expected.length);

      // A mid-buffer window: rows here straddle log-line boundaries in both
      // directions, which is the whole reason the byte index exists.
      expect(renderer.renderRows(lines, bytesPerRow, 20, 35)).toBe(
        expected.slice(20, 35).join("\n"),
      );
      // The head and the tail, including the last (padded) partial row.
      expect(renderer.renderRows(lines, bytesPerRow, 0, 3)).toBe(
        expected.slice(0, 3).join("\n"),
      );
      const total = expected.length;
      expect(renderer.renderRows(lines, bytesPerRow, total - 4, total)).toBe(
        expected.slice(total - 4).join("\n"),
      );
      // A window past the end clamps instead of inventing rows.
      expect(
        renderer.renderRows(lines, bytesPerRow, total - 1, total + 50),
      ).toBe(expected[total - 1]);
      expect(renderer.renderRows(lines, bytesPerRow, total, total + 10)).toBe(
        "",
      );
    }
  });

  it("renders a window whose rows all come from one very long line", () => {
    const renderer = new SerialDebugHexViewRenderer();
    // One line of 500 bytes spans ~32 rows on its own.
    const lines = [
      line(
        1,
        Array.from({ length: 500 }, (_, i) => i & 0xff),
      ),
    ];
    const expected = fullDump(lines, 16);

    expect(renderer.rowCount(lines, 16)).toBe(expected.length);
    expect(renderer.renderRows(lines, 16, 10, 14)).toBe(
      expected.slice(10, 14).join("\n"),
    );
  });

  it("re-bases the stream when lines are dropped off the head", () => {
    const renderer = new SerialDebugHexViewRenderer();
    const lines = buffer(40);
    renderer.renderRows(lines, 16, 0, 5);

    // The store drops the oldest lines; every byte offset shifts, so the dump
    // must be the one for the *remaining* buffer, not a suffix of the old one.
    const rolled = lines.slice(10);
    const expected = fullDump(rolled, 16);
    expect(renderer.rowCount(rolled, 16)).toBe(expected.length);
    expect(renderer.renderRows(rolled, 16, 0, 4)).toBe(
      expected.slice(0, 4).join("\n"),
    );
  });

  it("re-encodes text for archive lines that carry no rawBytes", () => {
    const renderer = new SerialDebugHexViewRenderer();
    // Lines paged back from the Rust session archive have no rawBytes: the
    // archive stores text only. The dump must show the text bytes, not blanks.
    // Non-ASCII text (a translated sys line, e.g. the archive-cap notice)
    // contributes its UTF-8 bytes.
    const lines = [
      { ...line(1), text: "AB" },
      { ...line(2), text: "±" },
    ];

    expect(renderer.renderRows(lines, 16, 0, 1)).toBe(fullDump(lines, 16)[0]);
    expect(renderer.renderRows(lines, 16, 0, 1)).toContain("41 42 0a c2 b1 0a");
  });

  it("maps a line index to the hex row its first byte lands on", () => {
    const renderer = new SerialDebugHexViewRenderer();
    const lines = buffer(20); // 8 bytes of stream per line

    expect(renderer.rowOfLine(lines, 16, 0)).toBe(0);
    expect(renderer.rowOfLine(lines, 16, 2)).toBe(1); // byte 16
    expect(renderer.rowOfLine(lines, 16, 5)).toBe(2); // byte 40
    expect(renderer.rowOfLine(lines, 8, 5)).toBe(5); // byte 40
    // Out of range indices clamp instead of returning NaN.
    expect(renderer.rowOfLine(lines, 16, -3)).toBe(0);
    expect(renderer.rowOfLine(lines, 16, 999)).toBe(10);
  });

  it("drops byte-chunk cache entries for lines that are no longer visible", () => {
    const renderer = new SerialDebugHexViewRenderer();
    renderer.rowCount([line(1, [0x41]), line(2, [0x42])], 16);
    expect(renderer.cacheSize()).toBe(2);

    renderer.rowCount([line(2, [0x42])], 16);
    expect(renderer.cacheSize()).toBe(1);

    renderer.rowCount([], 16);
    expect(renderer.cacheSize()).toBe(0);
  });

  it("keeps the byte-chunk cache bounded across a rolling buffer", () => {
    const renderer = new SerialDebugHexViewRenderer();
    const window = 50;
    const perFlush = 5;
    let next = 1;
    let lines = Array.from({ length: window }, () => line(next++, [0x41]));
    renderer.rowCount(lines, 16);

    // Saturated buffer: every flush drops as many lines off the head as it
    // appends. The sweep is amortized, so the bound is the window plus the
    // slack, not an exact match on every publish.
    for (let flush = 0; flush < 50; flush += 1) {
      lines = [
        ...lines.slice(perFlush),
        ...Array.from({ length: perFlush }, () => line(next++, [0x41])),
      ];
      renderer.rowCount(lines, 16);
      expect(renderer.cacheSize()).toBeLessThanOrEqual(window * 1.2 + perFlush);
    }
    expect(renderer.cacheSize()).toBeGreaterThanOrEqual(window);
  });
});
