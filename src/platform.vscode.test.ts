// @vitest-environment happy-dom
// VS Code file picking uses window messaging and File APIs, which the node environment lacks.
import { afterEach, describe, expect, it, vi } from "vitest";

describe("VS Code platform file picking", () => {
  afterEach(() => {
    vi.resetModules();
    delete (window as Window & { __TUYAOPEN_IDE_CONFIG?: unknown })
      .__TUYAOPEN_IDE_CONFIG;
    delete (window as Window & { acquireVsCodeApi?: unknown }).acquireVsCodeApi;
  });

  it("correlates the response and returns the picked bytes as a named File", async () => {
    const postMessage = vi.fn();
    (
      window as Window & {
        acquireVsCodeApi?: () => { postMessage: typeof postMessage };
      }
    ).acquireVsCodeApi = () => ({
      postMessage,
    });
    (
      window as Window & {
        __TUYAOPEN_IDE_CONFIG?: { runtime: string; wsUrl: string };
      }
    ).__TUYAOPEN_IDE_CONFIG = {
      runtime: "vscode",
      wsUrl: "ws://127.0.0.1:9527",
    };

    const { createPlatform } = await import("./platform");
    const platform = createPlatform();
    expect(platform.getWsUrl()).toBe("ws://127.0.0.1:9527");
    const resultPromise = platform.pickFile("request-1", ".bin,.hex");

    expect(postMessage).toHaveBeenCalledWith({
      type: "pickFile",
      requestId: "request-1",
      accept: ".bin,.hex",
    });

    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          type: "pickFileResult",
          requestId: "other-request",
          path: "/tmp/ignored.bin",
          content: "AQI=",
        },
      }),
    );
    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          type: "pickFileResult",
          requestId: "request-1",
          path: "C:\\firmware\\device.bin",
          content: "AQI=",
        },
      }),
    );

    const result = await resultPromise;
    expect(result?.path).toBe("C:\\firmware\\device.bin");
    expect(result?.file?.name).toBe("device.bin");
    expect(
      Array.from(new Uint8Array(await result!.file!.arrayBuffer())),
    ).toEqual([1, 2]);
  });

  it("resolves to null when the VS Code picker is cancelled", async () => {
    (
      window as Window & {
        acquireVsCodeApi?: () => { postMessage: () => void };
      }
    ).acquireVsCodeApi = () => ({
      postMessage: () => undefined,
    });
    (
      window as Window & { __TUYAOPEN_IDE_CONFIG?: { runtime: string } }
    ).__TUYAOPEN_IDE_CONFIG = {
      runtime: "vscode",
    };

    const { createPlatform } = await import("./platform");
    const resultPromise = createPlatform().pickFile("request-2", ".bin");
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "pickFileResult", requestId: "request-2", path: null },
      }),
    );

    await expect(resultPromise).resolves.toBeNull();
  });
});
