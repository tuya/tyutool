import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const onMountedMock = vi.fn((cb: () => void | Promise<void>) => {
  void cb();
});

vi.mock("vue", () => ({
  onMounted: onMountedMock,
}));

const isTauriRuntimeMock = vi.fn();
vi.mock("@/runtime", () => ({
  isTauriRuntime: isTauriRuntimeMock,
}));

const fetchLatestJsonMock = vi.fn();
const isNewerVersionMock = vi.fn();
vi.mock("@/features/settings/update-sources", () => ({
  UPDATE_SOURCES: [
    { id: "github", labelKey: "settings.update.sourceGithub", url: "github" },
    { id: "tuya", labelKey: "settings.update.sourceTuya", url: "tuya" },
  ],
  fetchLatestJson: fetchLatestJsonMock,
  isNewerVersion: isNewerVersionMock,
}));

const getManualUpdateFlagsMock = vi.fn();
vi.mock("@/utils/install-type", () => ({
  getManualUpdateFlags: getManualUpdateFlagsMock,
}));

const readyMock = vi.fn();
const getAutoUpdateLastCheckAtMock = vi.fn();
const setAutoUpdateLastCheckAtMock = vi.fn();
const useSettingsStoreMock = vi.fn(() => ({
  autoUpdateInterval: "6h",
  ready: readyMock,
  getAutoUpdateLastCheckAt: getAutoUpdateLastCheckAtMock,
  setAutoUpdateLastCheckAt: setAutoUpdateLastCheckAtMock,
}));
vi.mock("@/stores/settings", () => ({
  useSettingsStore: useSettingsStoreMock,
}));

vi.mock("@/config/app", () => ({
  APP_VERSION: "3.2.0",
}));

vi.mock("@/composables/toastState", () => ({
  toastState: {
    visible: false,
    version: "",
    isPortable: false,
    portableUrl: "",
  },
}));

vi.mock("@/utils/log", () => ({
  rLog: {
    info: vi.fn(),
    warn: vi.fn(),
  },
}));

describe("shouldAutoCheckForUpdate", () => {
  it("returns false outside Tauri runtime", async () => {
    const { shouldAutoCheckForUpdate } = await import("./useAutoUpdate");
    expect(
      shouldAutoCheckForUpdate({
        isTauri: false,
        interval: "6h",
        lastCheckedAt: null,
        now: 1000,
      }),
    ).toBe(false);
  });

  it('returns false when the interval is "off"', async () => {
    const { shouldAutoCheckForUpdate } = await import("./useAutoUpdate");
    expect(
      shouldAutoCheckForUpdate({
        isTauri: true,
        interval: "off",
        lastCheckedAt: null,
        now: 1000,
      }),
    ).toBe(false);
  });

  it("returns false when the cooldown has not elapsed", async () => {
    const { shouldAutoCheckForUpdate } = await import("./useAutoUpdate");
    expect(
      shouldAutoCheckForUpdate({
        isTauri: true,
        interval: "6h",
        lastCheckedAt: 10_000,
        now: 10_000 + 60_000,
      }),
    ).toBe(false);
  });

  it("returns true when there is no successful check history", async () => {
    const { shouldAutoCheckForUpdate } = await import("./useAutoUpdate");
    expect(
      shouldAutoCheckForUpdate({
        isTauri: true,
        interval: "6h",
        lastCheckedAt: null,
        now: 1000,
      }),
    ).toBe(true);
  });

  it("returns true when the cooldown has elapsed", async () => {
    const { shouldAutoCheckForUpdate } = await import("./useAutoUpdate");
    expect(
      shouldAutoCheckForUpdate({
        isTauri: true,
        interval: "1h",
        lastCheckedAt: 1_000,
        now: 1_000 + 3_600_000,
      }),
    ).toBe(true);
  });
});

describe("useAutoUpdate", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-08T12:00:00.000Z"));
    vi.resetModules();
    onMountedMock.mockClear();
    isTauriRuntimeMock.mockReset();
    isTauriRuntimeMock.mockReturnValue(true);
    fetchLatestJsonMock.mockReset();
    fetchLatestJsonMock.mockResolvedValue({
      version: "3.2.1",
      notes: "notes",
      pub_date: "2026-07-08",
      platforms: {},
      cli: {},
    });
    isNewerVersionMock.mockReset();
    isNewerVersionMock.mockReturnValue(true);
    getManualUpdateFlagsMock.mockReset();
    getManualUpdateFlagsMock.mockResolvedValue({
      manualOnly: false,
      debRpm: false,
    });
    readyMock.mockReset();
    readyMock.mockResolvedValue(undefined);
    getAutoUpdateLastCheckAtMock.mockReset();
    getAutoUpdateLastCheckAtMock.mockResolvedValue(null);
    setAutoUpdateLastCheckAtMock.mockReset();
    setAutoUpdateLastCheckAtMock.mockResolvedValue(undefined);
    useSettingsStoreMock.mockReset();
    useSettingsStoreMock.mockReturnValue({
      autoUpdateInterval: "6h",
      ready: readyMock,
      getAutoUpdateLastCheckAt: getAutoUpdateLastCheckAtMock,
      setAutoUpdateLastCheckAt: setAutoUpdateLastCheckAtMock,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("does not schedule a check outside Tauri runtime", async () => {
    isTauriRuntimeMock.mockReturnValue(false);
    const { useAutoUpdate } = await import("./useAutoUpdate");
    useAutoUpdate();
    await vi.runAllTimersAsync();
    expect(fetchLatestJsonMock).not.toHaveBeenCalled();
  });

  it('does not schedule a check when auto-update interval is "off"', async () => {
    useSettingsStoreMock.mockReturnValue({
      autoUpdateInterval: "off",
      ready: readyMock,
      getAutoUpdateLastCheckAt: getAutoUpdateLastCheckAtMock,
      setAutoUpdateLastCheckAt: setAutoUpdateLastCheckAtMock,
    });
    const { useAutoUpdate } = await import("./useAutoUpdate");
    useAutoUpdate();
    await vi.runAllTimersAsync();
    expect(fetchLatestJsonMock).not.toHaveBeenCalled();
  });

  it("does not schedule a check when the cooldown has not elapsed", async () => {
    getAutoUpdateLastCheckAtMock.mockResolvedValue(Date.now() - 60_000);
    const { useAutoUpdate } = await import("./useAutoUpdate");
    useAutoUpdate();
    await vi.runAllTimersAsync();
    expect(fetchLatestJsonMock).not.toHaveBeenCalled();
  });

  it("checks on first run and records the successful timestamp when a newer version is found", async () => {
    const { useAutoUpdate } = await import("./useAutoUpdate");
    useAutoUpdate();
    await vi.advanceTimersByTimeAsync(4000);
    expect(fetchLatestJsonMock).toHaveBeenCalledTimes(1);
    expect(setAutoUpdateLastCheckAtMock).toHaveBeenCalledWith(Date.now());
  });

  it("records the successful timestamp when already up to date", async () => {
    isNewerVersionMock.mockReturnValue(false);
    const { useAutoUpdate } = await import("./useAutoUpdate");
    useAutoUpdate();
    await vi.advanceTimersByTimeAsync(4000);
    expect(fetchLatestJsonMock).toHaveBeenCalledTimes(1);
    expect(setAutoUpdateLastCheckAtMock).toHaveBeenCalledWith(Date.now());
  });

  it("does not record a timestamp when all sources fail", async () => {
    fetchLatestJsonMock.mockRejectedValue(new Error("network"));
    const { useAutoUpdate } = await import("./useAutoUpdate");
    useAutoUpdate();
    await vi.advanceTimersByTimeAsync(4000);
    expect(fetchLatestJsonMock).toHaveBeenCalledTimes(2);
    expect(setAutoUpdateLastCheckAtMock).not.toHaveBeenCalled();
  });
});
