import { afterEach, describe, expect, it, vi } from "vitest";
import { getUpdatePlatformKey, manualUpdateUrl } from "./update-platform";
import { UPDATE_SOURCES, type LatestJson } from "./update-sources";
import { isTauriRuntime } from "@/runtime";
import { invoke } from "@tauri-apps/api/core";
import en from "@/locales/en.json";
import zh from "@/locales/zh-CN.json";

vi.mock("@/runtime", () => ({ isTauriRuntime: vi.fn(() => true) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => vi.clearAllMocks());

const baseManifest = {
  version: "9.9.9",
  notes: "",
  pub_date: "",
  platforms: {},
  cli: {},
};

describe("native update platform", () => {
  it.each([
    "linux-aarch64",
    "linux-x86_64",
    "darwin-aarch64",
    "darwin-x86_64",
    "windows-x86_64",
  ])("uses native %s for the download", async (platform) => {
    const labelKey = platform as keyof typeof en.settings.update.platforms;
    expect(en.settings.update.platforms[labelKey]).toBeTruthy();
    expect(zh.settings.update.platforms[labelKey]).toBeTruthy();
    vi.mocked(invoke).mockResolvedValueOnce(platform);
    const key = await getUpdatePlatformKey();
    expect(key).toBe(platform);
    expect(invoke).toHaveBeenCalledWith("get_update_platform");
    const manifest: LatestJson = {
      ...baseManifest,
      portable: { [platform]: { url: `https://example.test/${platform}` } },
    };
    expect(manualUpdateUrl(manifest, UPDATE_SOURCES[0], key, false)).toBe(
      `https://example.test/${platform}`,
    );
  });
  it("falls back to the release page on native lookup failure or a missing build", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Command unavailable"));
    const key = await getUpdatePlatformKey();
    expect(key).toBe("");
    expect(manualUpdateUrl(null, UPDATE_SOURCES[0], key, false)).toBe(
      UPDATE_SOURCES[0].releasePageUrl,
    );
    expect(
      manualUpdateUrl(
        { ...baseManifest, portable: {} },
        UPDATE_SOURCES[0],
        "unsupported",
        false,
      ),
    ).toBe(UPDATE_SOURCES[0].releasePageUrl);
  });
  it("keeps distro updates on the release page even with a matching portable build", () => {
    expect(
      manualUpdateUrl(
        {
          ...baseManifest,
          portable: { "linux-aarch64": { url: "https://example.test/arm" } },
        },
        UPDATE_SOURCES[0],
        "linux-aarch64",
        true,
      ),
    ).toBe(UPDATE_SOURCES[0].releasePageUrl);
  });
  it("does not guess a native platform in the browser", async () => {
    vi.mocked(isTauriRuntime).mockReturnValueOnce(false);
    expect(await getUpdatePlatformKey()).toBe("");
    expect(invoke).not.toHaveBeenCalled();
  });
});
