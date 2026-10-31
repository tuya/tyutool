// @vitest-environment happy-dom
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { nextTick } from "vue";

const { open, invoke } = vi.hoisted(() => ({
  open: vi.fn<(...args: unknown[]) => Promise<string | null>>(),
  invoke: vi.fn<(...args: unknown[]) => Promise<unknown>>(
    async () => "/tmp/archive",
  ),
}));
vi.mock("@/runtime", () => ({
  isTauriRuntime: () => true,
  getRuntime: () => "tauri",
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));
vi.mock("@/stores/batch-flash-auth-workspace", () => ({
  loadBatchFlashAuthWorkspace: vi.fn(),
  saveBatchFlashAuthCumulative: vi.fn(),
  saveBatchFlashAuthFilterConfig: vi.fn(),
  saveBatchFlashAuthFirmwareConfig: vi.fn(),
  saveBatchFlashAuthConfig: vi.fn(),
  saveBatchFlashAuthSharedConfig: vi.fn(),
}));
import { useBatchFlashAuthStore } from "./batch-flash-auth";

describe("batch archive snapshot and recovery", () => {
  let store: ReturnType<typeof useBatchFlashAuthStore>;
  beforeEach(() => {
    setActivePinia(createPinia());
    open.mockReset().mockResolvedValue("/tmp");
    invoke.mockReset().mockResolvedValue("/tmp/archive");
    store = useBatchFlashAuthStore();
  });
  afterEach(() => {
    store.cleanup();
    store.$dispose();
  });
  async function completed() {
    store.authorizeEnabled = false;
    store.firmwarePath = "/original.bin";
    store.addPorts(["COM3"]);
    await nextTick();
    await store.startBatch();
    expect(invoke).toHaveBeenCalledWith("batch_auth_start", expect.anything());
    store.slots[0].status = "done";
    store.checkBatchCompletion();
    expect(store.canArchive).toBe(true);
  }
  it("locks directory selection and archives the executed run after configuration and slots change", async () => {
    await completed();
    store.chipId = "t5ai";
    store.firmwarePath = "/changed.bin";
    let release!: (value: string | null) => void;
    open.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    const first = store.archiveBatch();
    const duplicate = store.archiveBatch();
    await vi.waitFor(() => expect(open).toHaveBeenCalled());
    const locked = store.archiveStatus;
    const canStart = store.canStart;
    store.slots[0].status = "failed";
    release("/tmp");
    await Promise.all([first, duplicate]);
    expect(open).toHaveBeenCalledTimes(1);
    expect(locked).toBe("archiving");
    expect(canStart).toBe(false);
    const calls = invoke.mock.calls.filter(
      ([cmd]) => cmd === "archive_batch_cmd",
    );
    expect(calls).toHaveLength(1);
    const args = calls[0][1] as {
      firmwarePath: string;
      summaryJson: string;
      slotsCsv: string;
    };
    expect(args.firmwarePath).toBe("/original.bin");
    expect(JSON.parse(args.summaryJson)).toMatchObject({
      config: { chipId: "esp32", opMode: "flash-only" },
      lastRun: { stats: { done: 1, failed: 0 } },
    });
    expect(args.slotsCsv).toContain("COM3,true,done");
  });
  it("unlocks after a cancelled or failed directory dialog and permits retry", async () => {
    await completed();
    open
      .mockResolvedValueOnce(null)
      .mockRejectedValueOnce(new Error("dialog unavailable"));
    await store.archiveBatch();
    expect(store.archiveStatus).toBe("idle");
    await store.archiveBatch();
    expect(store.archiveStatus).toBe("error");
    expect(store.archiveError).toBe("dialog unavailable");
    await store.archiveBatch();
    expect(store.archiveStatus).toBe("done");
    open.mockResolvedValueOnce(null);
    await store.archiveBatch();
    expect(store.archiveStatus).toBe("done");
    expect(store.lastArchivePath).toBe("/tmp/archive");
  });

  it("keeps the submitted Excel path and authorization mode when the next configuration is cleared", async () => {
    invoke.mockImplementation(async (cmd) =>
      cmd === "validate_excel_cmd"
        ? {
            total: 1,
            used: 0,
            inProgress: 0,
            remaining: 1,
            invalid: 0,
          }
        : "/tmp/archive",
    );
    store.flashFirmware = false;
    store.authConfig.excelPath = "/original.xlsx";
    store.addPorts(["COM3"]);
    await vi.waitFor(() => expect(store.canStart).toBe(true));
    await store.startBatch();
    store.authConfig.excelPath = "";
    store.authorizeEnabled = false;
    store.slots[0].status = "done";
    store.checkBatchCompletion();
    await nextTick();
    expect(store.canArchive).toBe(true);
    await store.archiveBatch();
    expect(invoke).toHaveBeenCalledWith(
      "archive_batch_cmd",
      expect.objectContaining({
        excelPath: "/original.xlsx",
        firmwarePath: undefined,
      }),
    );
    const args = invoke.mock.calls.find(
      ([cmd]) => cmd === "archive_batch_cmd",
    )![1] as { summaryJson: string };
    expect(JSON.parse(args.summaryJson)).toMatchObject({
      config: { opMode: "auth-only", excelPath: "/original.xlsx" },
    });
  });

  it("archives refreshed completion statistics without accepting a different sheet's statistics", async () => {
    let validations = 0;
    invoke.mockImplementation(async (cmd) => {
      if (cmd === "validate_excel_cmd") {
        validations++;
        return {
          total: validations > 2 ? 99 : 1,
          used: validations > 1 ? 1 : 0,
          inProgress: 0,
          remaining: validations > 1 ? 0 : 1,
          invalid: 0,
        };
      }
      return "/tmp/archive";
    });
    store.flashFirmware = false;
    store.authConfig.excelPath = "/original.xlsx";
    store.addPorts(["COM3"]);
    await vi.waitFor(() => expect(store.canStart).toBe(true));
    await store.startBatch();
    store.slots[0].status = "done";
    store.checkBatchCompletion();
    await vi.waitFor(() => expect(store.excelStats?.remaining).toBe(0));
    store.authConfig.excelPath = "/different.xlsx";
    await vi.waitFor(() => expect(store.excelStats?.total).toBe(99));
    await store.archiveBatch();
    const args = invoke.mock.calls.find(
      ([cmd]) => cmd === "archive_batch_cmd",
    )![1] as { summaryJson: string };
    expect(JSON.parse(args.summaryJson).excelStats).toMatchObject({
      total: 1,
      used: 1,
      remaining: 0,
    });
  });

  it.each(["success", "failure"])(
    "ignores a late archive write %s after cleanup",
    async (outcome) => {
      await completed();
      let resolve!: (path: string) => void;
      let reject!: (error: Error) => void;
      invoke.mockImplementation(async (cmd) =>
        cmd === "archive_batch_cmd"
          ? new Promise<string>((yes, no) => {
              resolve = yes;
              reject = no;
            })
          : "/tmp/archive",
      );
      const pending = store.archiveBatch();
      await vi.waitFor(() => expect(resolve).toBeDefined());
      await store.startBatch();
      await store.retryPort("COM3");
      await store.readPort("COM3");
      await store.readAll();
      expect(
        invoke.mock.calls.filter(([cmd]) => cmd === "batch_auth_start"),
      ).toHaveLength(1);
      expect(
        invoke.mock.calls.filter(([cmd]) => cmd === "batch_auth_read_ports"),
      ).toHaveLength(0);
      store.cleanup();
      if (outcome === "success") resolve("/late/archive");
      else reject(new Error("late failure"));
      await pending;
      expect(store.archiveStatus).toBe("idle");
      expect(store.lastArchivePath).toBe("");
      expect(store.archiveError).toBe("");
    },
  );
  it("ignores a directory response after cleanup", async () => {
    await completed();
    let release!: (value: string | null) => void;
    open.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    const pending = store.archiveBatch();
    await vi.waitFor(() => expect(open).toHaveBeenCalled());
    store.cleanup();
    release("/tmp");
    await pending;
    expect(
      invoke.mock.calls.filter(([cmd]) => cmd === "archive_batch_cmd"),
    ).toHaveLength(0);
    expect(store.archiveStatus).toBe("idle");
  });
});
