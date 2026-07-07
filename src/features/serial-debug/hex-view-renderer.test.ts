// @vitest-environment node
import { describe, expect, it, vi } from "vitest";
import type { DebugLogLine } from "./types";
import { SerialDebugHexViewRenderer } from "./hex-view-renderer";

const { formatHexDumpFromChunksMock } = vi.hoisted(() => ({
  formatHexDumpFromChunksMock: vi.fn(() => "hex-dump"),
}));

vi.mock("./hex-format", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./hex-format")>();
  return {
    ...actual,
    formatHexDumpFromChunks: formatHexDumpFromChunksMock,
  };
});

function line(id: number, rawBytes?: number[]): DebugLogLine {
  return {
    id,
    direction: "rx",
    tsMs: id,
    text: `line-${id}`,
    rawBytes: rawBytes ? Uint8Array.from(rawBytes) : undefined,
  };
}

describe("SerialDebugHexViewRenderer", () => {
  it("reuses the cached dump when visible lines and bytesPerRow are unchanged", () => {
    const renderer = new SerialDebugHexViewRenderer();
    const lines = [line(1, [0x41, 0x0a]), line(2, [0x42, 0x0a])];

    expect(renderer.render(lines, 16)).toBe("hex-dump");
    expect(renderer.render(lines, 16)).toBe("hex-dump");

    expect(formatHexDumpFromChunksMock).toHaveBeenCalledTimes(1);
  });

  it("drops byte-chunk cache entries for lines that are no longer visible", () => {
    const renderer = new SerialDebugHexViewRenderer();
    renderer.render([line(1, [0x41]), line(2, [0x42])], 16);
    expect(renderer.cacheSize()).toBe(2);

    renderer.render([line(2, [0x42])], 16);
    expect(renderer.cacheSize()).toBe(1);
  });
});
