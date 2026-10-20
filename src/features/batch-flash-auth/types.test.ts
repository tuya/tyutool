import { describe, expect, it } from "vitest";
import {
  BATCH_AUTH_TOOL_CHIP_OPTIONS,
  BATCH_FLASH_CAPABLE_CHIPS,
  OTP_CAPABLE_CHIPS,
} from "./types";
import { resolveCommandPrefix } from "./types";

describe("resolveCommandPrefix", () => {
  it.each([
    ["none", "anything", ""],
    ["ap_cmd", "ignored", "ap_cmd"],
    ["custom", "  vendor command  ", "vendor command"],
  ] as const)(
    "resolves %s mode",
    (commandPrefixMode, customCommandPrefix, result) => {
      expect(
        resolveCommandPrefix({ commandPrefixMode, customCommandPrefix }),
      ).toBe(result);
    },
  );

  it.each(["", "  ", "\ncustom", "custom\r", "custom\r\n"])(
    "rejects empty or control-containing custom prefix %j",
    (customCommandPrefix) => {
      expect(
        resolveCommandPrefix({
          commandPrefixMode: "custom",
          customCommandPrefix,
        }),
      ).toBeNull();
    },
  );
});

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

  it("gd32vw553 is selectable but not flash-capable", () => {
    expect(BATCH_AUTH_TOOL_CHIP_OPTIONS).toContain("gd32vw553");
    expect(BATCH_FLASH_CAPABLE_CHIPS).not.toContain("gd32vw553");
  });
});
