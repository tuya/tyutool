import { describe, expect, it, vi } from "vitest";

vi.mock("@/runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime")>()),
  isTauriRuntime: vi.fn(() => false),
}));

import {
  canUseInAppUpdater,
  getManualUpdateFlags,
  getManualUpdateFlagsForInstallType,
  isPortableInstall,
} from "./install-type";

describe("getManualUpdateFlagsForInstallType", () => {
  it("requires manual updates for portable installs", () => {
    expect(
      getManualUpdateFlagsForInstallType("portable (/tmp/tyutool)"),
    ).toEqual({
      manualOnly: true,
      debRpm: false,
    });
  });

  it("requires manual updates for deb/rpm installs", () => {
    expect(getManualUpdateFlagsForInstallType("deb/rpm (installed)")).toEqual({
      manualOnly: true,
      debRpm: true,
    });
  });

  it("allows in-app updates for AppImage installs", () => {
    expect(getManualUpdateFlagsForInstallType("AppImage")).toEqual({
      manualOnly: false,
      debRpm: false,
    });
  });

  it("does not assume manual-only behavior for unknown installs", () => {
    expect(getManualUpdateFlagsForInstallType("unknown")).toEqual({
      manualOnly: false,
      debRpm: false,
    });
  });

  it("matches the install-type substrings case-insensitively", () => {
    expect(getManualUpdateFlagsForInstallType("PORTABLE")).toEqual({
      manualOnly: true,
      debRpm: false,
    });
    expect(getManualUpdateFlagsForInstallType("DEB/RPM")).toEqual({
      manualOnly: true,
      debRpm: true,
    });
  });

  it("treats an empty install type as in-app-updatable", () => {
    expect(getManualUpdateFlagsForInstallType("")).toEqual({
      manualOnly: false,
      debRpm: false,
    });
  });
});

describe("canUseInAppUpdater", () => {
  it("waits for install type detection before enabling the in-app updater", () => {
    expect(
      canUseInAppUpdater(false, { manualOnly: false, debRpm: false }),
    ).toBe(false);
  });

  it("disables the in-app updater for manual-only installs", () => {
    expect(canUseInAppUpdater(true, { manualOnly: true, debRpm: true })).toBe(
      false,
    );
  });

  it("enables the in-app updater after supported install type detection", () => {
    expect(canUseInAppUpdater(true, { manualOnly: false, debRpm: false })).toBe(
      true,
    );
  });
});

describe("browser-mode resolution (non-Tauri)", () => {
  it("reports browser install type as in-app-updatable, not portable", async () => {
    // isTauriRuntime is mocked to false → resolveInstallType returns "browser".
    expect(await getManualUpdateFlags()).toEqual({
      manualOnly: false,
      debRpm: false,
    });
    expect(await isPortableInstall()).toBe(false);
  });
});
