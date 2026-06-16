import { describe, expect, it } from "vitest";
import { sanitizePortName } from "./utils";

describe("sanitizePortName", () => {
  it("strips leading slash and replaces unsafe chars", () => {
    expect(sanitizePortName("/dev/ttyUSB0")).toBe("dev_ttyUSB0");
    expect(sanitizePortName("COM3")).toBe("COM3");
  });
});
