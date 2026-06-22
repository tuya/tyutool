import { afterEach, beforeEach, describe, it, expect, vi } from "vitest";

// Mock fns must be created via vi.hoisted so they exist when the hoisted
// vi.mock factories run.
const { showConfirmDialog, save, invoke } = vi.hoisted(() => ({
  showConfirmDialog: vi.fn(async () => true),
  save: vi.fn(async () => "/home/u/logs.zip"),
  invoke: vi.fn(
    async (_cmd: string, _args?: Record<string, unknown>) => undefined,
  ),
}));

vi.mock("@/runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime")>()),
  isTauriRuntime: vi.fn(() => false),
}));
vi.mock("@/composables/confirmDialog", () => ({ showConfirmDialog }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save }));
vi.mock("@tauri-apps/api/path", () => ({
  downloadDir: vi.fn(async () => "/home/u/Downloads"),
  homeDir: vi.fn(async () => "/home/u"),
  join: vi.fn(async (...parts: string[]) => parts.join("/")),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { isTauriRuntime } from "@/runtime";
import { buildIssueUrl, detectOs, exportLogsAndReport } from "./report-issue";

// Minimal i18n stub: returns the key so assertions can match on the key.
const t = ((key: string) => key) as unknown as Parameters<
  typeof exportLogsAndReport
>[0];

describe("buildIssueUrl", () => {
  it("prefills the bug_report form's version and os fields", () => {
    const u = new URL(buildIssueUrl({ version: "3.0.11", os: "linux" }));
    expect(u.pathname).toContain("/tuya/tyutool/issues/new");
    expect(u.searchParams.get("template")).toBe("bug_report.yml");
    expect(u.searchParams.get("version")).toBe("3.0.11");
    expect(u.searchParams.get("os")).toBe("linux");
  });

  it("folds the install type into the os field when provided", () => {
    const u = new URL(
      buildIssueUrl({ version: "1.0.0", os: "linux", install: "AppImage" }),
    );
    expect(u.searchParams.get("os")).toBe("linux (AppImage)");
  });
});

describe("detectOs", () => {
  it("maps a Windows UA to 'windows'", () => {
    expect(
      detectOs(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
      ),
    ).toBe("windows");
  });

  it("maps a macOS UA to 'macos'", () => {
    expect(
      detectOs(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
      ),
    ).toBe("macos");
  });

  it("maps a Linux/X11 UA to 'linux'", () => {
    expect(
      detectOs(
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
      ),
    ).toBe("linux");
  });

  it("returns 'unknown' for unrecognized strings", () => {
    expect(detectOs("SomeWeirdBot/1.0")).toBe("unknown");
  });

  it("is case-insensitive", () => {
    expect(detectOs("WINDOWS")).toBe("windows");
    expect(detectOs("MACINTOSH")).toBe("macos");
    expect(detectOs("LINUX")).toBe("linux");
  });
});

describe("exportLogsAndReport", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    save.mockResolvedValue("/home/u/logs.zip");
    invoke.mockResolvedValue(undefined);
    showConfirmDialog.mockResolvedValue(true);
    // navigator with a working clipboard by default
    vi.stubGlobal("navigator", {
      userAgent: "Mozilla/5.0 (X11; Linux x86_64)",
      clipboard: { writeText: vi.fn(async () => undefined) },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("web mode: shows a hint dialog and returns early without touching Tauri APIs", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(false);
    await exportLogsAndReport(t);
    expect(showConfirmDialog).toHaveBeenCalledTimes(1);
    expect(showConfirmDialog).toHaveBeenCalledWith(
      expect.objectContaining({ message: "settings.reportIssue.webHint" }),
    );
    expect(save).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("Tauri mode: copies the issue URL to the clipboard and opens it externally", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(true);
    const writeText = vi.fn(async (_text: string) => undefined);
    vi.stubGlobal("navigator", {
      userAgent: "Mozilla/5.0 (X11; Linux x86_64)",
      clipboard: { writeText },
    });

    await exportLogsAndReport(t);

    // exports the zip, copies the URL, and opens it
    expect(invoke).toHaveBeenCalledWith("export_logs_zip", {
      destPath: "/home/u/logs.zip",
    });
    expect(writeText).toHaveBeenCalledTimes(1);
    const copied = writeText.mock.calls[0][0];
    expect(copied).toContain("template=bug_report.yml");
    expect(invoke).toHaveBeenCalledWith(
      "open_external_url",
      expect.objectContaining({ url: copied }),
    );
    // final success confirmation dialog
    expect(showConfirmDialog).toHaveBeenCalled();
  });

  it("Tauri mode: clipboard failure is swallowed and the flow still opens the URL", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(true);
    const writeText = vi.fn(async () => {
      throw new Error("clipboard unavailable");
    });
    vi.stubGlobal("navigator", {
      userAgent: "Mozilla/5.0 (X11; Linux x86_64)",
      clipboard: { writeText },
    });

    await expect(exportLogsAndReport(t)).resolves.toBeUndefined();
    expect(writeText).toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("open_external_url", expect.anything());
  });

  it("Tauri mode: open_external_url failure is logged but does not throw", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(true);
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "open_external_url") throw new Error("xdg-open missing");
      return undefined;
    });

    await expect(exportLogsAndReport(t)).resolves.toBeUndefined();
    // still reaches the final success dialog after the open failure
    expect(showConfirmDialog).toHaveBeenCalled();
  });

  it("Tauri mode: returns early without copying when the save dialog is cancelled", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(true);
    save.mockResolvedValue(null as unknown as string);
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", {
      userAgent: "Mozilla/5.0 (X11; Linux x86_64)",
      clipboard: { writeText },
    });

    await exportLogsAndReport(t);

    expect(invoke).not.toHaveBeenCalled();
    expect(writeText).not.toHaveBeenCalled();
  });

  it("Tauri mode: export_logs_zip failure shows the export-failed dialog and aborts", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(true);
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "export_logs_zip") throw new Error("disk full");
      return undefined;
    });
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", {
      userAgent: "Mozilla/5.0 (X11; Linux x86_64)",
      clipboard: { writeText },
    });

    await exportLogsAndReport(t);

    expect(showConfirmDialog).toHaveBeenCalledWith(
      expect.objectContaining({
        message: "settings.reportIssue.exportFailed",
      }),
    );
    // never reached the clipboard / open step
    expect(writeText).not.toHaveBeenCalled();
  });
});
