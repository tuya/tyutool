// @vitest-environment happy-dom
import { createPinia, setActivePinia } from "pinia";
import { createApp, defineComponent, h, nextTick } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n";
import { useFlashStore } from "@/stores/flash";
import FirmwareFlashPage from "../FirmwareFlashPage.vue";

describe("flash result and log feedback", () => {
  let app: ReturnType<typeof createApp>;
  let host: HTMLDivElement;
  let store: ReturnType<typeof useFlashStore>;
  const result = {
    op: "read" as const,
    chipId: "t5ai",
    port: "COM3",
    baud: 123456,
    outputPath: "original.bin",
    authRead: false,
    status: "success" as const,
    elapsedSecs: 3.25,
    message: "Read complete",
  };
  beforeEach(() => {
    const pinia = createPinia();
    setActivePinia(pinia);
    i18n.global.locale.value = "en";
    store = useFlashStore();
    store.operationResult = { ...result };
    store.flashPhase = "success";
    store.flashMessage = result.message;
    host = document.createElement("div");
    document.body.append(host);
    app = createApp(FirmwareFlashPage);
    app.use(pinia).use(i18n);
    app.component(
      "FontAwesomeIcon",
      defineComponent(() => () => h("i")),
    );
    app.mount(host);
  });
  afterEach(() => {
    app.unmount();
    store.$dispose();
    host.remove();
    vi.restoreAllMocks();
  });
  function button(name: string): HTMLButtonElement {
    return [...host.querySelectorAll("button")].find(
      (b) =>
        b.textContent?.trim() === name || b.getAttribute("aria-label") === name,
    )!;
  }
  async function flush() {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await nextTick();
  }

  it("shows the original summary and returns keyboard focus to the operation configuration", async () => {
    store.selectedSerialPort = "COM99";
    store.readFileName = "changed.bin";
    await nextTick();
    const card = host.querySelector(".flash-result")!;
    expect(card.textContent).toContain("COM3");
    expect(card.textContent).toContain("123456");
    expect(card.textContent).toContain("3.3s");
    expect(card.textContent).toContain("original.bin");
    expect(card.textContent).not.toContain("changed.bin");
    button("Back to configuration").click();
    await flush();
    expect(store.activeTab).toBe("read");
    expect(document.activeElement?.getAttribute("role")).toBe("tab");
    expect(document.activeElement?.getAttribute("aria-selected")).toBe("true");
    expect(store.operationResult).toMatchObject(result);
    expect(button("Open output file")).toBeUndefined();
  });

  it("makes error copying recoverable and keeps cancellation distinct from failure", async () => {
    store.operationResult = {
      ...result,
      status: "error",
      message: "Port disconnected",
    };
    store.flashPhase = "error";
    store.flashMessage = "Port disconnected";
    const write = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockRejectedValueOnce(new Error("denied"))
      .mockResolvedValue(undefined);
    await nextTick();
    button("Copy error").click();
    await flush();
    expect(host.textContent).toContain(i18n.global.t("common.copyFailed"));
    button("Copy error").click();
    await flush();
    expect(write).toHaveBeenLastCalledWith("Port disconnected");
    expect(host.textContent).toContain(i18n.global.t("common.copied"));
    store.operationResult = { ...result, status: "cancelled" };
    await nextTick();
    expect(host.querySelector(".flash-result")?.getAttribute("role")).toBe(
      "status",
    );
    expect(button("Copy error")).toBeUndefined();
    button("Clear result").click();
    await nextTick();
    expect(host.querySelector(".flash-result")).toBeNull();
  });

  it("shows nearby log copy feedback while the log is scroll locked", async () => {
    store.lockAutoScroll = true;
    const write = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockRejectedValueOnce(new Error("denied"))
      .mockResolvedValue(undefined);
    button("Copy all logs").click();
    await flush();
    expect(host.querySelector(".log-copy-feedback")?.textContent).toBe(
      i18n.global.t("common.copyFailed"),
    );
    button("Copy all logs").click();
    await flush();
    expect(write).toHaveBeenCalledTimes(2);
    expect(host.querySelector(".log-copy-feedback")?.textContent).toBe(
      i18n.global.t("common.copied"),
    );
  });
});
