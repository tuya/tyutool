// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from "vue";
import { createI18n } from "vue-i18n";
import LogViewerDialog from "./LogViewerDialog.vue";
import en from "@/locales/en.json";
import source from "./LogViewerDialog.vue?raw";
import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

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
    host.querySelector<HTMLElement>("tbody tr button")!.click();
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

describe("LogViewerDialog file reads", () => {
  let app: ReturnType<typeof createApp>;
  let host: HTMLElement;
  const files = ["tyutool-A.log", "tyutool-B.log"].map((name) => ({
    name,
    sizeBytes: 100,
    modifiedMs: 0,
  }));
  afterEach(() => {
    app.unmount();
    document.body.replaceChildren();
  });

  async function mount() {
    const reads = new Map<
      string,
      { resolve: (text: string) => void; reject: (e: Error) => void }
    >();
    vi.mocked(invoke).mockImplementation((cmd, args) => {
      if (cmd === "list_log_files") return Promise.resolve(files) as never;
      if (cmd === "read_log_tail")
        return new Promise((resolve, reject) =>
          reads.set((args as { filename: string }).filename, {
            resolve,
            reject,
          }),
        ) as never;
      return Promise.resolve([]) as never;
    });
    host = document.createElement("div");
    document.body.append(host);
    const open = ref(false);
    app = createApp({
      render: () =>
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
      expect(host.querySelectorAll("tbody tr")).toHaveLength(2),
    );
    const button = (name: string) =>
      [...host.querySelectorAll<HTMLButtonElement>("button")].find(
        (b) => b.textContent?.trim() === name,
      )!;
    const choose = async (index: number) => {
      const row = host.querySelectorAll<HTMLElement>("tbody tr")[index];
      (row.querySelector<HTMLElement>("button") ?? row).click();
      await vi.waitFor(() => expect(reads.has(files[index].name)).toBe(true));
    };
    return { reads, button, choose, open };
  }

  it.each(["success", "error"])(
    "prevents late %s for A from changing B's content",
    async (kind) => {
      const { reads, button, choose } = await mount();
      await choose(0);
      button(en.settings.logViewer.fileList).click();
      await nextTick();
      await choose(1);
      reads.get(files[1].name)!.resolve("INFO from B");
      await vi.waitFor(() => expect(host.textContent).toContain("INFO from B"));
      if (kind === "success") reads.get(files[0].name)!.resolve("INFO from A");
      else reads.get(files[0].name)!.reject(new Error("old error"));
      await new Promise((r) => setTimeout(r, 0));
      expect(host.textContent).toContain("INFO from B");
      expect(host.textContent).not.toContain("INFO from A");
      expect(host.textContent).not.toContain("old error");
    },
  );

  it("preserves search and reading offset on refresh instead of reopening the file", async () => {
    const { reads, button, choose } = await mount();
    await choose(0);
    reads.get(files[0].name)!.resolve("INFO first\nINFO second");
    await vi.waitFor(() =>
      expect(host.querySelector("[data-line]")).not.toBeNull(),
    );
    const input = host.querySelector<HTMLInputElement>("input[type=text]")!;
    input.value = "INFO";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await nextTick();
    const content = host.querySelector("[data-line]")!.parentElement!;
    Object.defineProperties(content, {
      scrollHeight: { value: 1000, configurable: true },
      clientHeight: { value: 100, configurable: true },
    });
    content.scrollTop = 40;
    reads.clear();
    button(en.settings.logViewer.refresh).click();
    await vi.waitFor(() => expect(reads.has(files[0].name)).toBe(true));
    reads.get(files[0].name)!.resolve("INFO first\nINFO second\nINFO third");
    await vi.waitFor(() => expect(host.textContent).toContain("INFO third"));
    expect(input.value).toBe("INFO");
    expect(host.querySelector("[data-line]")!.parentElement!.scrollTop).toBe(
      40,
    );
  });
  it("keeps loaded content and search available when a refresh fails", async () => {
    const { reads, button, choose } = await mount();
    await choose(0);
    reads.get(files[0].name)!.resolve("INFO retained");
    await vi.waitFor(() =>
      expect(host.querySelector("[data-line]")).not.toBeNull(),
    );
    const input = host.querySelector<HTMLInputElement>("input[type=text]")!;
    input.value = "INFO";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await nextTick();
    reads.clear();
    button(en.settings.logViewer.refresh).click();
    await vi.waitFor(() => expect(reads.has(files[0].name)).toBe(true));
    reads.get(files[0].name)!.reject(new Error("Refresh refused"));
    await vi.waitFor(() =>
      expect(host.querySelector('[role="alert"]')?.textContent).toContain(
        "Refresh refused",
      ),
    );
    expect(host.querySelector("[data-line]")?.textContent).toBe(
      "INFO retained",
    );
    expect(input.value).toBe("INFO");
  });
});
