// @vitest-environment happy-dom
import { createPinia, setActivePinia } from "pinia";
import { createApp, defineComponent, nextTick } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SerialDebugTransport } from "./transport";
import { __setSerialDebugTransportForTest } from "./transport";
import { AUTO_SAVE_FLUSH_MAX_CHARS } from "./constants";
import { useSerialDebugStore } from "@/stores/serial-debug";
import { useSerialAutoSave } from "./useSerialAutoSave";

const invokeSpy = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeSpy,
}));

vi.mock("vue-i18n", async (importOriginal) => {
  const actual = await importOriginal<typeof import("vue-i18n")>();
  return {
    ...actual,
    useI18n: () => ({
      t: (key: string, params?: { msg?: string }) =>
        params?.msg ? `${key}: ${params.msg}` : key,
    }),
  };
});

function fakeTransport(): SerialDebugTransport {
  return {
    async open() {},
    async close() {},
    async send() {},
    async clearSession() {},
    async appendSysLine() {},
    async addFilter() {
      throw new Error("not implemented");
    },
    async removeFilter() {},
    async readFilterMatches() {
      throw new Error("not implemented");
    },
    async readSessionPage() {
      throw new Error("not implemented");
    },
    onChunk() {
      return () => {};
    },
    onChunkBatch() {
      return () => {};
    },
    onDisconnect() {
      return () => {};
    },
    onFilterUpdated() {
      return () => {};
    },
  };
}

async function flushMicrotasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await nextTick();
}

describe("useSerialAutoSave", () => {
  let app: ReturnType<typeof createApp> | null = null;
  let host: HTMLDivElement | null = null;

  beforeEach(() => {
    vi.useFakeTimers();
    invokeSpy.mockReset();
    setActivePinia(createPinia());
    __setSerialDebugTransportForTest(fakeTransport());
  });

  afterEach(() => {
    app?.unmount();
    host?.remove();
    app = null;
    host = null;
    __setSerialDebugTransportForTest(null);
    vi.useRealTimers();
  });

  it("waits for an in-flight flush to finish before dropping the session path", async () => {
    const s = useSerialDebugStore();
    host = document.createElement("div");
    document.body.appendChild(host);
    app = createApp(
      defineComponent({
        setup() {
          useSerialAutoSave(s);
          return () => null;
        },
      }),
    );
    app.mount(host);

    s.port = "/dev/ttyUSB0";
    s.autoSave = true;
    s.autoSaveDir = "/logs";
    s.open = true;
    await nextTick();
    expect(s.sessionAutoSavePath).not.toBeNull();

    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from("first\n")],
    });

    let releaseFirstWrite: (() => void) | null = null;
    invokeSpy
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            releaseFirstWrite = resolve;
          }),
      )
      .mockResolvedValueOnce(undefined);

    await vi.advanceTimersByTimeAsync(5000);
    expect(invokeSpy).toHaveBeenCalledTimes(1);

    s.appendChunk({
      direction: "rx",
      tsMs: 1001,
      bytes: [...Buffer.from("second\n")],
    });
    s.open = false;
    await nextTick();

    expect(s.sessionAutoSavePath).not.toBeNull();

    (releaseFirstWrite as (() => void) | null)?.();
    await flushMicrotasks();

    expect(invokeSpy).toHaveBeenCalledTimes(2);
    expect(s.sessionAutoSavePath).toBeNull();
  });

  it("drains a large close-time backlog in bounded append batches", async () => {
    const s = useSerialDebugStore();
    host = document.createElement("div");
    document.body.appendChild(host);
    app = createApp(
      defineComponent({
        setup() {
          useSerialAutoSave(s);
          return () => null;
        },
      }),
    );
    app.mount(host);

    s.port = "/dev/ttyUSB0";
    s.autoSave = true;
    s.autoSaveDir = "/logs";
    s.open = true;
    await nextTick();

    const lineText = "x".repeat(2048);
    const lineCount = Math.ceil(AUTO_SAVE_FLUSH_MAX_CHARS / 2048) + 8;
    s.appendChunk({
      direction: "rx",
      tsMs: 1000,
      bytes: [...Buffer.from(`${`${lineText}\n`.repeat(lineCount)}`)],
    });

    invokeSpy.mockResolvedValue(undefined);

    s.open = false;
    await nextTick();
    await flushMicrotasks();
    await flushMicrotasks();
    await flushMicrotasks();

    expect(invokeSpy.mock.calls.length).toBeGreaterThan(1);
    expect(s.sessionAutoSavePath).toBeNull();
    for (const call of invokeSpy.mock.calls) {
      expect((call[1] as { content: string }).content.length).toBeLessThan(
        AUTO_SAVE_FLUSH_MAX_CHARS * 2,
      );
    }
  });
});
