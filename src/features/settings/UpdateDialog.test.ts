// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from "vue";
import { createI18n } from "vue-i18n";
import UpdateDialog from "./UpdateDialog.vue";
import en from "@/locales/en.json";
import updateDialogSource from "./UpdateDialog.vue?raw";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchLatestJson, type LatestJson } from "./update-sources";
import { updateCheck, updateDownload, updateInstall } from "./in-app-updater";
import { getUpdatePlatformKey } from "./update-platform";
import { getManualUpdateFlags } from "@/utils/install-type";
import { openUrl } from "@tauri-apps/plugin-opener";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("./update-platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./update-platform")>()),
  getUpdatePlatformKey: vi.fn().mockResolvedValue("linux-aarch64"),
}));

vi.mock("@/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("@tauri-apps/plugin-log", () => ({ info: vi.fn(), error: vi.fn() }));
vi.mock("./in-app-updater", () => ({
  updateCheck: vi.fn(),
  updateDownload: vi.fn(),
  updateInstall: vi.fn(),
}));
vi.mock("@/utils/install-type", () => ({
  getManualUpdateFlags: vi
    .fn()
    .mockResolvedValue({ manualOnly: false, debRpm: false }),
  canUseInAppUpdater: (ready: boolean) => ready,
}));

vi.mock("./update-sources", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./update-sources")>()),
  fetchLatestJson: vi.fn().mockRejectedValue(new Error("Offline test")),
}));

describe("UpdateDialog keyboard focus", () => {
  let app: ReturnType<typeof createApp>;
  afterEach(() => {
    app.unmount();
    document.body.replaceChildren();
  });

  it("enters, traps focus, ignores handled Escape and restores on close", async () => {
    const host = document.createElement("div");
    const opener = document.createElement("button");
    document.body.append(opener, host);
    opener.focus();
    const open = ref(false);
    app = createApp({
      setup: () => () =>
        h(UpdateDialog, {
          open: open.value,
          onClose: () => {
            open.value = false;
          },
        }),
    });
    app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
    app.component("FontAwesomeIcon", () => h("span"));
    app.mount(host);
    open.value = true;
    await nextTick();
    await nextTick();
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    const button = dialog.querySelector("button")!;
    expect(document.activeElement).toBe(button);
    const tab = new KeyboardEvent("keydown", {
      key: "Tab",
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    button.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(true);
    const buttons = [...dialog.querySelectorAll("button:not([disabled])")];
    expect(document.activeElement).toBe(buttons[buttons.length - 1]);
    const event = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    event.preventDefault();
    button.dispatchEvent(event);
    expect(open.value).toBe(true);
    button.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    await nextTick();
    expect(open.value).toBe(false);
    expect(document.activeElement).toBe(opener);
  });
});

describe("UpdateDialog source actions", () => {
  it("renders a per-source update action in each source card", () => {
    expect(updateDialogSource).toContain(
      '@click="triggerSourceAction(source)"',
    );
    expect(updateDialogSource).toContain(
      't("settings.update.updateFromSource")',
    );
    expect(updateDialogSource).toContain(
      "v-if=\"sourceActionKind(source) !== 'none'\"",
    );
  });
});

describe("UpdateDialog checks and failures", () => {
  let app: ReturnType<typeof createApp>;
  afterEach(() => {
    app.unmount();
    document.body.replaceChildren();
    vi.mocked(fetchLatestJson).mockRejectedValue(new Error("Offline test"));
    vi.mocked(getManualUpdateFlags).mockResolvedValue({
      manualOnly: false,
      debRpm: false,
    });
    vi.mocked(getUpdatePlatformKey).mockResolvedValue("linux-aarch64");
    vi.mocked(openUrl).mockClear();
  });
  const manifest = (version: string): LatestJson => ({
    version,
    notes: "Synthetic notes",
    pub_date: "",
    platforms: {},
    cli: {},
  });
  async function mount() {
    const host = document.createElement("div");
    document.body.append(host);
    const open = ref(false);
    app = createApp({
      render: () =>
        h(UpdateDialog, {
          open: open.value,
          onClose: () => {
            open.value = false;
          },
        }),
    });
    app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
    app.component("FontAwesomeIcon", () => h("span"));
    app.mount(host);
    open.value = true;
    await nextTick();
    await nextTick();
    return open;
  }

  it("retries one failed source without clearing the successful source and ignores repeated clicks", async () => {
    vi.mocked(fetchLatestJson).mockImplementation((url) =>
      url.includes("github")
        ? Promise.reject(new Error("Offline test"))
        : Promise.resolve(manifest("9.9.9")),
    );
    await mount();
    await vi.waitFor(() =>
      expect(document.body.textContent).toContain("v9.9.9"),
    );
    const card = [...document.querySelectorAll(".ud-source-card")].find((el) =>
      el.textContent?.includes("Offline test"),
    )!;
    const retry = [...card.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === "Retry",
    )!;
    expect(retry).toBeDefined();
    let complete!: (m: LatestJson) => void;
    vi.mocked(fetchLatestJson).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    const before = vi.mocked(fetchLatestJson).mock.calls.length;
    retry.click();
    retry.click();
    await nextTick();
    expect(fetchLatestJson).toHaveBeenCalledTimes(before + 1);
    expect(document.body.textContent).toContain("v9.9.9");
    complete(manifest("9.9.10"));
    await vi.waitFor(() =>
      expect(
        document.querySelector(".ud-overview-highlight-value")?.textContent,
      ).toContain("9.9.10"),
    );
  });

  it("rechecks failed sources in place", async () => {
    await mount();
    await vi.waitFor(() =>
      expect(document.body.textContent).toContain("Offline test"),
    );
    const recheck = [...document.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === "Check again",
    )!;
    expect(recheck).toBeDefined();
    vi.mocked(fetchLatestJson).mockResolvedValue(manifest("9.9.9"));
    recheck.click();
    await vi.waitFor(() =>
      expect(
        document.querySelector(".ud-overview-highlight-value")?.textContent,
      ).toContain("9.9.9"),
    );
    expect(document.body.textContent).not.toContain("Offline test");
  });

  it("does not steal focus when the user moves away during a source retry", async () => {
    vi.mocked(fetchLatestJson).mockImplementation((url) =>
      url.includes("github")
        ? Promise.reject(new Error("Offline test"))
        : Promise.resolve(manifest("9.9.9")),
    );
    await mount();
    await vi.waitFor(() =>
      expect(
        document.querySelector(".ud-overview-actions .ud-btn-primary"),
      ).not.toBeNull(),
    );
    let complete!: (m: LatestJson) => void;
    vi.mocked(fetchLatestJson).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    const retry = document.querySelector<HTMLButtonElement>(
      '[data-source-id="github"] .ud-source-action',
    )!;
    retry.focus();
    retry.click();
    await nextTick();
    const other = document.querySelector<HTMLButtonElement>(
      ".ud-overview-actions .ud-btn-primary",
    )!;
    other.focus();
    complete(manifest("9.9.10"));
    await vi.waitFor(() =>
      expect(
        document.querySelector(".ud-overview-highlight-value")?.textContent,
      ).toContain("9.9.10"),
    );
    expect(document.activeElement).toBe(other);
  });

  it("ignores a late retry after closing and reopening the dialog", async () => {
    const open = await mount();
    await vi.waitFor(() =>
      expect(document.body.textContent).toContain("Offline test"),
    );
    let complete!: (m: LatestJson) => void;
    vi.mocked(fetchLatestJson).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    const retry = [...document.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === "Retry",
    )!;
    retry.click();
    open.value = false;
    await nextTick();
    vi.mocked(fetchLatestJson).mockResolvedValue(manifest("9.9.9"));
    open.value = true;
    await vi.waitFor(() =>
      expect(
        document.querySelector(".ud-overview-highlight-value")?.textContent,
      ).toContain("9.9.9"),
    );
    complete(manifest("99.0.0"));
    await nextTick();
    await nextTick();
    expect(document.body.textContent).not.toContain("99.0.0");
  });

  it("downloads the native ARM64 package and offers explicit choice when native detection fails", async () => {
    vi.mocked(getManualUpdateFlags).mockResolvedValue({
      manualOnly: true,
      debRpm: false,
    });
    const available = {
      ...manifest("9.9.9"),
      portable: {
        "linux-aarch64": { url: "https://example.test/arm64" },
        "linux-x86_64": { url: "https://example.test/x64" },
      },
    };
    vi.mocked(fetchLatestJson).mockResolvedValue(available);
    const open = await mount();
    const download = () =>
      [...document.querySelectorAll<HTMLButtonElement>("button")].find(
        (button) =>
          button.textContent?.trim() === en.settings.update.downloadPackage,
      );
    await vi.waitFor(() => expect(download()).toBeDefined());
    download()!.click();
    await vi.waitFor(() =>
      expect(openUrl).toHaveBeenCalledWith("https://example.test/arm64"),
    );
    open.value = false;
    await nextTick();
    vi.mocked(getUpdatePlatformKey).mockResolvedValue("");
    open.value = true;
    await vi.waitFor(() =>
      expect(document.querySelector("#update-platform")).not.toBeNull(),
    );
    expect(download()).toBeUndefined();
    const release = [
      ...document.querySelectorAll<HTMLButtonElement>("button"),
    ].find(
      (button) =>
        button.textContent?.trim() === en.settings.update.openReleasePage,
    )!;
    release.click();
    await vi.waitFor(() =>
      expect(openUrl).toHaveBeenCalledWith(
        "https://github.com/tuya/tyutool/releases/latest",
      ),
    );
  });

  it("ignores checks from a previous open after a newer check completed", async () => {
    const requests: ((m: LatestJson) => void)[] = [];
    vi.mocked(fetchLatestJson).mockImplementation(
      () => new Promise((resolve) => requests.push(resolve)),
    );
    const open = await mount();
    expect(requests).toHaveLength(2);
    open.value = false;
    await nextTick();
    open.value = true;
    await nextTick();
    expect(requests).toHaveLength(4);
    requests[2](manifest("9.9.9"));
    requests[3](manifest("9.9.9"));
    await vi.waitFor(() =>
      expect(document.body.textContent).toContain("v9.9.9"),
    );
    requests[0](manifest("0.0.1"));
    requests[1](manifest("0.0.1"));
    await nextTick();
    await nextTick();
    expect(document.body.textContent).toContain("v9.9.9");
    expect(document.body.textContent).not.toContain(
      en.settings.update.summaryUpToDateTitle,
    );
  });

  it("shows a user-visible failure when the update operation rejects", async () => {
    vi.mocked(fetchLatestJson).mockResolvedValue(manifest("9.9.9"));
    vi.mocked(updateCheck).mockRejectedValue(
      new Error("Synthetic connection refused"),
    );
    await mount();
    await vi.waitFor(() =>
      expect(document.querySelector(".ud-btn-primary")).not.toBeNull(),
    );
    document.querySelector<HTMLButtonElement>(".ud-btn-primary")!.click();
    await vi.waitFor(() =>
      expect(document.querySelector('[role="alert"]')?.textContent).toContain(
        "Synthetic connection refused",
      ),
    );
  });
  it("retains the downloaded package for retry after installation fails", async () => {
    vi.mocked(fetchLatestJson).mockResolvedValue(manifest("9.9.9"));
    vi.mocked(updateCheck).mockResolvedValue({
      available: true,
      version: "9.9.9",
      currentVersion: "3.2.10",
      date: null,
      body: null,
    });
    vi.mocked(updateDownload).mockResolvedValue(undefined);
    vi.mocked(updateInstall).mockRejectedValue(new Error("Installer refused"));
    await mount();
    await vi.waitFor(() =>
      expect(document.querySelector(".ud-btn-primary")).not.toBeNull(),
    );
    document.querySelector<HTMLButtonElement>(".ud-btn-primary")!.click();
    const restart = () =>
      [...document.querySelectorAll<HTMLButtonElement>("button")].find(
        (button) =>
          button.textContent?.trim() === en.settings.update.restartNow,
      );
    await vi.waitFor(() => expect(restart()).toBeDefined());
    restart()!.click();
    await vi.waitFor(() =>
      expect(document.querySelector('[role="alert"]')?.textContent).toContain(
        "Installer refused",
      ),
    );
    expect(restart()!.disabled).toBe(false);
    restart()!.click();
    await vi.waitFor(() => expect(updateInstall).toHaveBeenCalledTimes(2));
  });
});
