// @vitest-environment happy-dom
import { createApp, h, nextTick } from "vue";
import { createI18n } from "vue-i18n";
import { afterEach, describe, expect, it, vi } from "vitest";
import en from "@/locales/en.json";
import { useCopyFeedback } from "./useCopyFeedback";

describe("copy feedback", () => {
  let app: ReturnType<typeof createApp>;
  afterEach(() => {
    app.unmount();
    document.body.replaceChildren();
    vi.restoreAllMocks();
  });
  function mount() {
    let feedback!: ReturnType<typeof useCopyFeedback>;
    app = createApp({
      setup() {
        feedback = useCopyFeedback();
        return () => h("span", feedback.message.value);
      },
    });
    app.use(createI18n({ legacy: false, locale: "en", messages: { en } }));
    const host = document.createElement("div");
    document.body.append(host);
    app.mount(host);
    return feedback;
  }
  it("reports failure and permits a successful retry", async () => {
    const write = vi
      .fn()
      .mockRejectedValueOnce(new Error("denied"))
      .mockResolvedValue(undefined);
    vi.spyOn(navigator.clipboard, "writeText").mockImplementation(write);
    const f = mount();
    await f.copy("Synthetic");
    await nextTick();
    expect(f.state.value).toBe("failed");
    expect(f.message.value).toBe(en.common.copyFailed);
    await f.copy("Synthetic");
    expect(f.state.value).toBe("copied");
    expect(write).toHaveBeenCalledTimes(2);
  });
  it("does not report an old copy after the panel resets and prevents overlapping writes", async () => {
    let finish!: () => void;
    const write = vi.spyOn(navigator.clipboard, "writeText").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const f = mount();
    const pending = f.copy("Synthetic");
    await f.copy("Other");
    expect(write).toHaveBeenCalledOnce();
    f.reset();
    finish();
    await pending;
    expect(f.state.value).toBe("idle");
  });
});
