import { describe, expect, it } from "vitest";
import {
  parseBatchSharedConfig,
  type BatchSharedConfig,
} from "./batch-flash-auth-workspace";

/** A minimal valid shared-config record; callers spread a patch over it. */
function base(): BatchSharedConfig {
  return {
    chipId: "esp32",
    baudRate: 460800,
    authBaudRate: 115200,
    flashFirmware: true,
    authorizeEnabled: true,
  };
}

describe("parseBatchSharedConfig", () => {
  it("returns null for non-object input", () => {
    expect(parseBatchSharedConfig(null)).toBeNull();
    expect(parseBatchSharedConfig("x")).toBeNull();
    expect(parseBatchSharedConfig(42)).toBeNull();
    expect(parseBatchSharedConfig(undefined)).toBeNull();
  });

  it("parses a valid record unchanged", () => {
    const w = parseBatchSharedConfig(base());
    expect(w).not.toBeNull();
    expect(w!.chipId).toBe("esp32");
    expect(w!.baudRate).toBe(460800);
    expect(w!.authBaudRate).toBe(115200);
    expect(w!.flashFirmware).toBe(true);
    expect(w!.authorizeEnabled).toBe(true);
  });

  it("falls back to the default chip when chipId is unrecognized", () => {
    // A stale/legacy chipId must not crash chipManifest later.
    const w = parseBatchSharedConfig({ ...base(), chipId: "bk7231n" });
    expect(w!.chipId).toBe("esp32");
    // baud falls back to the default chip's manifest default (esp32 = 460800)
    expect(w!.baudRate).toBe(460800);
  });

  it("falls back to the default chip when chipId is the wrong type", () => {
    const w = parseBatchSharedConfig({ ...base(), chipId: 123 });
    expect(w!.chipId).toBe("esp32");
  });

  it("accepts the 'other' auth-only chip option", () => {
    // 'other' is a valid batch-auth chip (authorize-only); it must pass.
    const w = parseBatchSharedConfig({ ...base(), chipId: "other" });
    expect(w!.chipId).toBe("other");
  });

  it("falls back to the chip manifest default baud when baud is unrecognized", () => {
    const w = parseBatchSharedConfig({
      ...base(),
      chipId: "esp32",
      baudRate: 999999,
      authBaudRate: 999999,
    });
    // esp32 manifest defaults
    expect(w!.baudRate).toBe(460800);
    expect(w!.authBaudRate).toBe(115200);
  });

  it("falls back to chip manifest defaults for t5ai", () => {
    const w = parseBatchSharedConfig({
      ...base(),
      chipId: "t5ai",
      baudRate: "bad" as unknown as number,
      authBaudRate: null as unknown as number,
    });
    expect(w!.chipId).toBe("t5ai");
    expect(w!.baudRate).toBe(921600);
    expect(w!.authBaudRate).toBe(115200);
  });

  it("coerces non-boolean flags to their defaults", () => {
    const w = parseBatchSharedConfig({
      ...base(),
      flashFirmware: "yes",
      authorizeEnabled: 1,
    });
    expect(w!.flashFirmware).toBe(true);
    expect(w!.authorizeEnabled).toBe(true);
  });

  it("preserves explicit false flags", () => {
    const w = parseBatchSharedConfig({
      ...base(),
      flashFirmware: false,
      authorizeEnabled: false,
    });
    expect(w!.flashFirmware).toBe(false);
    expect(w!.authorizeEnabled).toBe(false);
  });

  it("defaults missing optional flags to true", () => {
    const w = parseBatchSharedConfig({
      chipId: "esp32",
      baudRate: 460800,
      authBaudRate: 115200,
    });
    expect(w!.flashFirmware).toBe(true);
    expect(w!.authorizeEnabled).toBe(true);
  });

  it("rejects NaN / Infinity baud values", () => {
    const w = parseBatchSharedConfig({
      ...base(),
      chipId: "esp32",
      baudRate: NaN,
      authBaudRate: Infinity,
    });
    expect(w!.baudRate).toBe(460800);
    expect(w!.authBaudRate).toBe(115200);
  });
});
