// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from "vue";
import { createI18n } from "vue-i18n";
import LogViewerDialog from "./LogViewerDialog.vue";
import en from "@/locales/en.json";
import source from "./LogViewerDialog.vue?raw";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "list_log_files")
      return [{ name: "tyutool-test.log", sizeBytes: 10, modifiedMs: 0 }];
    if (command === "read_log_tail") return "INFO test";
    return [];
  }),
}));
vi.mock("./log-openers", () => ({
  listLogFileOpeners: vi.fn().mockResolvedValue([]),
  openLogFileInEditor: vi.fn(),
}));

describe("LogViewerDialog nested keyboard focus", () => {
  let app: ReturnType<typeof createApp>;
  afterEach(() => {
    app.unmount();
    document.body.replaceChildren();
  });

  it("traps only the top dialog and restores through the open-with button to the page opener", async () => {
    const host = document.createElement("div");
    const opener = document.createElement("button");
    document.body.append(opener, host);
    opener.focus();
    const open = ref(false);
    app = createApp({
      setup: () => () =>
        h(LogViewerDialog, {
          open: open.value,
          onClose: () => {
            open.value = false;
          },
        }),
    });
    app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
    app.mount(host);
    open.value = true;
    await vi.waitFor(() =>
      expect(host.querySelector("tbody tr")).not.toBeNull(),
    );
    const parent = host.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(parent?.getAttribute("aria-modal")).toBe("true");
    expect(document.activeElement).toBe(parent.querySelector("button"));
    host.querySelector<HTMLElement>("tbody tr")!.click();
    await vi.waitFor(() =>
      expect(
        [...host.querySelectorAll("button")].some(
          (button) =>
            button.textContent?.trim() === en.settings.logViewer.openWith,
        ),
      ).toBe(true),
    );
    const openWith = [...host.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === en.settings.logViewer.openWith,
    )!;
    openWith.focus();
    openWith.click();
    await nextTick();
    await nextTick();
    const child = host.querySelectorAll<HTMLElement>('[role="dialog"]')[1];
    expect(child?.getAttribute("aria-modal")).toBe("true");
    const close = child.querySelector("button")!;
    expect(document.activeElement).toBe(close);
    expect(parent.closest("[inert]")).not.toBeNull();
    close.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).toBe(close);
    close.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await nextTick();
    await nextTick();
    expect(open.value).toBe(true);
    expect(document.activeElement).toBe(openWith);
    expect(parent.closest("[inert]")).toBeNull();
    openWith.dispatchEvent(
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

describe("LogViewerDialog", () => {
  it("uses light-mode readable styling for the current-session badge", () => {
    expect(source).not.toContain("bg-blue-600/40");
    expect(source).not.toContain("text-blue-300");
  });

  it("uses light-mode readable styling for the truncated-log notice", () => {
    expect(source).not.toContain(
      "rounded bg-yellow-900/30 px-3 py-1.5 text-xs text-yellow-400",
    );
    expect(source).toContain("log-truncated-notice");
  });
});
