// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from "vue";
import { createI18n } from "vue-i18n";
import UpdateDialog from "./UpdateDialog.vue";
import en from "@/locales/en.json";
import updateDialogSource from "./UpdateDialog.vue?raw";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchLatestJson, type LatestJson } from "./update-sources";
import { updateCheck, updateDownload, updateInstall } from "./in-app-updater";

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
