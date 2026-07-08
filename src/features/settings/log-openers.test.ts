import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn(
    async (_cmd: string, _args?: Record<string, unknown>): Promise<unknown> =>
      undefined,
  ),
}));

vi.mock("@/runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime")>()),
  isTauriRuntime: vi.fn(() => false),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { isTauriRuntime } from "@/runtime";
import {
  listLogFileOpeners,
  openLogFileInEditor,
  type LogFileOpener,
} from "./log-openers";

describe("log-openers", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("web mode returns no editor choices", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(false);

    await expect(listLogFileOpeners()).resolves.toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("Tauri mode lists detected editor choices", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(true);
    const openers: LogFileOpener[] = [
      { id: "default", label: "System Default" },
      { id: "vscode", label: "VS Code" },
    ];
    invoke.mockResolvedValue(openers);

    await expect(listLogFileOpeners()).resolves.toEqual(openers);
    expect(invoke).toHaveBeenCalledWith("list_log_file_openers");
  });

  it("opens the selected log file with the chosen editor", async () => {
    vi.mocked(isTauriRuntime).mockReturnValue(true);

    await openLogFileInEditor("tyutool-20260708.log", "vscode");

    expect(invoke).toHaveBeenCalledWith("open_log_file_in_editor", {
      filename: "tyutool-20260708.log",
      editorId: "vscode",
    });
  });
});
