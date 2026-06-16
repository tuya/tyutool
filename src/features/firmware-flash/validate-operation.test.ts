// @vitest-environment happy-dom
import { describe, it, expect } from "vitest";
import {
  validateOperation,
  type ValidateOperationInput,
} from "./validate-operation";

const base: ValidateOperationInput = {
  flashSegments: [
    { firmwarePath: "fw.bin", startAddr: "0x0", endAddr: "0x100" },
  ],
  readDir: "/tmp",
  readFileName: "dump.bin",
  authorizeUuid: "",
  authorizeAuthKey: "",
  selectedSerialPort: "/dev/ttyUSB0",
  eraseStartAddr: "0x0",
  eraseEndAddr: "0x1000",
  readStartAddr: "0x0",
  readEndAddr: "0x1000",
  isTauri: true,
};

describe("validateOperation", () => {
  it("passes a well-formed flash op", () => {
    expect(validateOperation("flash", base)).toBeNull();
  });

  it("rejects flash with an empty firmware path", () => {
    const r = validateOperation("flash", {
      ...base,
      flashSegments: [
        { firmwarePath: "  ", startAddr: "0x0", endAddr: "0x100" },
      ],
    });
    expect(r).not.toBeNull();
  });

  it("rejects read without readDir in Tauri mode", () => {
    expect(validateOperation("read", { ...base, readDir: "" })).not.toBeNull();
  });

  it("allows empty readDir in web mode", () => {
    expect(
      validateOperation("read", { ...base, readDir: "", isTauri: false }),
    ).toBeNull();
  });

  it("rejects read without a file name", () => {
    expect(
      validateOperation("read", { ...base, readFileName: "" }),
    ).not.toBeNull();
  });

  it("rejects authorize when only uuid is filled", () => {
    expect(
      validateOperation("authorize", {
        ...base,
        authorizeUuid: "x".repeat(20),
        authorizeAuthKey: "",
      }),
    ).not.toBeNull();
  });

  it("rejects authorize with wrong uuid length", () => {
    expect(
      validateOperation("authorize", {
        ...base,
        authorizeUuid: "short",
        authorizeAuthKey: "y".repeat(32),
      }),
    ).not.toBeNull();
  });

  it("accepts authorize with correct lengths", () => {
    expect(
      validateOperation("authorize", {
        ...base,
        authorizeUuid: "x".repeat(20),
        authorizeAuthKey: "y".repeat(32),
      }),
    ).toBeNull();
  });

  it("rejects any op without a serial port", () => {
    expect(
      validateOperation("erase", { ...base, selectedSerialPort: "" }),
    ).not.toBeNull();
  });

  it("rejects an invalid erase address range", () => {
    expect(
      validateOperation("erase", {
        ...base,
        eraseStartAddr: "0x2000",
        eraseEndAddr: "0x1000",
      }),
    ).not.toBeNull();
  });
});
