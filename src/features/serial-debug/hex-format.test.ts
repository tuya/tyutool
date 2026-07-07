import { describe, expect, it } from "vitest";
import {
  formatHexDump,
  formatHexDumpFromChunks,
  parseHexInput,
} from "./hex-format";

describe("formatHexDump", () => {
  it("formats a full 16-byte row with ascii separator", () => {
    const bytes = new Uint8Array([
      0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x2c, 0x20, 0x77, 0x6f, 0x72, 0x6c, 0x64,
      0x21, 0x0a, 0x00, 0xff,
    ]);
    const out = formatHexDump(bytes, 16);
    // Expect: hex16 | ascii with non-printable replaced by '.'
    expect(out).toContain("48 65 6c 6c 6f 2c 20 77 6f 72 6c 64 21 0a 00 ff");
    expect(out).toContain("| Hello, world!...");
  });

  it("pads the last incomplete row so the | aligns", () => {
    const bytes = new Uint8Array([0x41, 0x42, 0x43]);
    const out = formatHexDump(bytes, 8);
    const lines = out.split("\n");
    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatch(/^41 42 43 {15} \| ABC$/);
  });

  it("supports 32 bytes per row", () => {
    const bytes = new Uint8Array(Array.from({ length: 32 }, (_, i) => i));
    const out = formatHexDump(bytes, 32);
    expect(out.split("\n")).toHaveLength(1);
    expect(out).toContain("00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f");
    expect(out).toContain("10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f");
  });

  it("emits multiple rows when input exceeds bytesPerRow", () => {
    const bytes = new Uint8Array(Array.from({ length: 20 }, (_, i) => i));
    const out = formatHexDump(bytes, 8);
    expect(out.split("\n")).toHaveLength(3);
  });

  it('replaces non-printable bytes with "." in the ascii column', () => {
    const bytes = new Uint8Array([0x00, 0x07, 0x1f, 0x20, 0x7e, 0x7f, 0xff]);
    const out = formatHexDump(bytes, 8);
    expect(out).toContain("| ... ~..");
  });

  it("formats split byte chunks the same as a contiguous buffer", () => {
    const bytes = new Uint8Array(
      Array.from({ length: 19 }, (_, i) => i + 0x30),
    );
    const chunks = [bytes.slice(0, 5), bytes.slice(5, 11), bytes.slice(11)];
    expect(formatHexDumpFromChunks(chunks, 8)).toBe(formatHexDump(bytes, 8));
  });
});

describe("parseHexInput", () => {
  it("parses whitespace-separated pairs", () => {
    expect(Array.from(parseHexInput("AA BB cc").bytes)).toEqual([
      0xaa, 0xbb, 0xcc,
    ]);
  });

  it("parses a contiguous hex string", () => {
    expect(Array.from(parseHexInput("aabbcc").bytes)).toEqual([
      0xaa, 0xbb, 0xcc,
    ]);
  });

  it("ignores non-hex characters", () => {
    const r = parseHexInput("AA,BB;CC \t dd");
    expect(Array.from(r.bytes)).toEqual([0xaa, 0xbb, 0xcc, 0xdd]);
    expect(r.ignoredCount).toBe(0);
  });

  it("drops a trailing half byte and reports ignoredCount=1", () => {
    const r = parseHexInput("AA BB C");
    expect(Array.from(r.bytes)).toEqual([0xaa, 0xbb]);
    expect(r.ignoredCount).toBe(1);
  });

  it("handles empty input", () => {
    const r = parseHexInput("");
    expect(r.bytes.length).toBe(0);
    expect(r.ignoredCount).toBe(0);
  });
});
