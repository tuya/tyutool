import { describe, expect, it } from "vitest";
import {
  BATCH_AUTH_TOOL_CHIP_OPTIONS,
  BATCH_FLASH_CAPABLE_CHIPS,
  OTP_CAPABLE_CHIPS,
} from "./types";

describe("batch auth chip lists", () => {
  it("flash-capable chips are a subset of the tool's chip options", () => {
    for (const id of BATCH_FLASH_CAPABLE_CHIPS) {
      expect(BATCH_AUTH_TOOL_CHIP_OPTIONS).toContain(id);
    }
  });

  it("OTP-capable chips are a subset of the tool's chip options", () => {
    for (const id of OTP_CAPABLE_CHIPS) {
      expect(BATCH_AUTH_TOOL_CHIP_OPTIONS).toContain(id);
    }
  });

  it("t9 is selectable and flash-capable", () => {
    expect(BATCH_AUTH_TOOL_CHIP_OPTIONS).toContain("t9");
    expect(BATCH_FLASH_CAPABLE_CHIPS).toContain("t9");
  });
});
