// @vitest-environment node
import { describe, expect, it, vi } from "vitest";
import type { DebugLogLine } from "./types";

const { parseAnsiMock, stripAnsiMock } = vi.hoisted(() => ({
  parseAnsiMock: vi.fn((text: string) => [{ text, style: { fg: "#f00" } }]),
  stripAnsiMock: vi.fn((text: string) => text.replace(/\x1b\[[0-9;]*m/g, "")),
}));

vi.mock("./ansi-parse", () => ({
  parseAnsi: parseAnsiMock,
  stripAnsi: stripAnsiMock,
}));

import { SerialDebugLogLineRenderer } from "./log-line-renderer";

function line(id: number, text: string): DebugLogLine {
  return {
    id,
    direction: "rx",
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

  it("drops cached entries for lines that are no longer visible", () => {
    const renderer = new SerialDebugLogLineRenderer();

    renderer.render([line(1, "one"), line(2, "two")], true, "");
    expect(renderer.cacheSize()).toBe(2);

    renderer.render([line(2, "two")], true, "");
    expect(renderer.cacheSize()).toBe(1);
  });
});
