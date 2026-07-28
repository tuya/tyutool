import { describe, expect, it } from "vitest";
import { archiveLineToLogLine, sanitizePortName } from "./utils";

describe("sanitizePortName", () => {
  it("strips leading slash and replaces unsafe chars", () => {
    expect(sanitizePortName("/dev/ttyUSB0")).toBe("dev_ttyUSB0");
    expect(sanitizePortName("COM3")).toBe("COM3");
  });
});

describe("archiveLineToLogLine", () => {
  it("carries over line fields and applies the given display id", () => {
    expect(
      archiveLineToLogLine(
        { lineNo: 42, tsMs: 1700, direction: "rx", text: "ERR boom" },
        7,
      ),
    ).toEqual({
      id: 7,
      tsMs: 1700,
      direction: "rx",
      text: "ERR boom",
      rawBytes: undefined,
    });
  });

  it("converts rawBytes to a Uint8Array for the hex view", () => {
    const line = archiveLineToLogLine(
      { lineNo: 1, tsMs: 0, direction: "tx", text: "hi", rawBytes: [104, 105] },
      1,
    );
    expect(line.rawBytes).toBeInstanceOf(Uint8Array);
    expect(Array.from(line.rawBytes!)).toEqual([104, 105]);
  });
});
