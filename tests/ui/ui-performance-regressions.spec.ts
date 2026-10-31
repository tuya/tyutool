import { test, expect, type Page } from "playwright/test";
import { openUi } from "./helpers/serial-log";

async function history(page: Page) {
  await openUi(page, "/serial-debug");
  await page.evaluate(async () => {
    const url = "/src/stores/serial-debug.ts";
    const { useSerialDebugStore } = await import(url);
    useSerialDebugStore().sendHistory = Array.from({ length: 100 }, (_, i) => ({
      text: `command-${i}`,
      name: "",
      mode: "ascii",
      appendCrlf: true,
      lastSentAt: 1_700_000_000_000 + i,
      sendCount: 1,
      favorite: false,
    }));
  });
  await page.locator(".history-wrap > button").click();
}

test("history arrow navigation keeps focus on the selected command", async ({
  page,
}) => {
  await history(page);
  const items = page.locator(".history-item");
  await items.first().click();
  await items.first().press("ArrowDown");
  await expect(items.nth(1)).toBeFocused();
  await expect(items.nth(1)).toHaveAttribute("aria-pressed", "true");
  await items.nth(1).press("Enter");
  await expect(page.locator(".history-preview pre")).toHaveText("command-1");
  const search = page.locator(".history-search");
  await search.focus();
  await search.press("ArrowDown");
  await expect(search).toBeFocused();
  await expect(items.nth(2)).toHaveAttribute("aria-pressed", "true");
  await search.press("Enter");
  await expect(page.locator(".send-bar input[type=text]")).toHaveValue(
    "command-2",
  );
});

test("history name editing reuses date formatting while language changes update it", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const original = Intl.DateTimeFormat;
    (window as unknown as Record<string, unknown>).__dateFormats = 0;
    Intl.DateTimeFormat = new Proxy(original, {
      construct(target, args) {
        const audit = window as unknown as { __dateFormats: number };
        audit.__dateFormats++;
        return Reflect.construct(target, args);
      },
    });
  });
  await history(page);
  const before = await page.evaluate(
    () => (window as unknown as { __dateFormats: number }).__dateFormats,
  );
  await page.locator(".history-name input").fill("unsaved name");
  await page.evaluate(() => new Promise(requestAnimationFrame));
  const after = await page.evaluate(
    () => (window as unknown as { __dateFormats: number }).__dateFormats,
  );
  expect(after - before).toBe(0);
  const previous = await page
    .locator(".history-item .history-meta")
    .first()
    .textContent();
  await page.evaluate(async () => {
    const url = "/src/stores/settings.ts";
    const { useSettingsStore } = await import(url);
    useSettingsStore().setLocale("zh-CN");
  });
  await expect(
    page.locator(".history-item .history-meta").first(),
  ).not.toHaveText(previous!);
});

test("history scrolling avoids anchor measurements and batches external repositioning", async ({
  page,
}) => {
  await history(page);
  await page.evaluate(() => new Promise(requestAnimationFrame));
  const reads = await page.evaluate(async () => {
    const trigger = document.querySelector(".history-wrap > button")!;
    const original = Element.prototype.getBoundingClientRect;
    let count = 0;
    Element.prototype.getBoundingClientRect = function () {
      if (this === trigger) count++;
      return original.call(this);
    };
    const settle = async () => {
      await new Promise(requestAnimationFrame);
      await new Promise(requestAnimationFrame);
    };
    try {
      const list = document.querySelector(".history-list")!;
      list.scrollTop = 200;
      for (let i = 0; i < 100; i++) list.dispatchEvent(new Event("scroll"));
      await settle();
      const internal = count;
      count = 0;
      for (let i = 0; i < 100; i++) {
        window.dispatchEvent(new Event("scroll"));
        window.dispatchEvent(new Event("resize"));
      }
      await settle();
      return { internal, external: count, scrollTop: list.scrollTop };
    } finally {
      Element.prototype.getBoundingClientRect = original;
    }
  });
  expect(reads.internal).toBe(0);
  expect(reads.external).toBe(1);
  expect(reads.scrollTop).toBe(200);
});

test("settings coalesces burst scroll events and selects the final visible section", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 680 });
  await openUi(page, "/settings");
  await expect(page.locator(".ty-route-enter-active")).toHaveCount(0);
  const reads = await page.evaluate(async () => {
    const original = Element.prototype.getBoundingClientRect;
    let count = 0;
    Element.prototype.getBoundingClientRect = function () {
      if (this.id === "app-main" || this.tagName === "SECTION") count++;
      return original.call(this);
    };
    try {
      const main = document.getElementById("app-main")!;
      main.scrollTop = 20;
      for (let i = 0; i < 100; i++) main.dispatchEvent(new Event("scroll"));
      await new Promise(requestAnimationFrame);
      await new Promise(requestAnimationFrame);
      return count;
    } finally {
      Element.prototype.getBoundingClientRect = original;
    }
  });
  expect(reads).toBeLessThanOrEqual(12);
  await page.locator("#app-main").evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await expect(
    page.locator('.settings-nav a[href="#about-heading"]'),
  ).toHaveAttribute("aria-current", "location");
});

for (const theme of ["light", "dark"]) {
  test(`${theme}: diagnostic search preserves spacing and brings the first result into view`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/settings");
    await page.evaluate(async (theme) => {
      const url = "/src/stores/settings.ts";
      const { useSettingsStore } = await import(url);
      useSettingsStore().setTheme(theme);
      Object.assign(window, {
        __TAURI_INTERNALS__: {
          invoke: async (command: string) => {
            if (command === "list_log_files")
              return [
                {
                  name: "tyutool-search.log",
                  sizeBytes: 10_000,
                  modifiedMs: 0,
                },
              ];
            if (command === "read_log_tail")
              return Array.from(
                { length: 200 },
                (_, i) => `INFO a & b <device>\t  column=${i}`,
              ).join("\n");
            return [];
          },
        },
      });
    }, theme);
    await page.getByRole("button", { name: /View Logs/ }).click();
    await page.locator(".log-file-button").click();
    const content = page.getByRole("dialog").locator("div[aria-busy]");
    await expect(content.locator("[data-line]")).toHaveCount(200);
    expect(
      await content
        .locator("[data-line]")
        .first()
        .evaluate((el) => getComputedStyle(el).whiteSpace),
    ).toBe("pre-wrap");
    await page.getByRole("dialog").getByRole("textbox").fill("<device>");
    await expect(content.locator("[data-line] mark")).toHaveCount(200);
    await expect(content.locator("[data-line]").first()).toHaveText(
      "INFO a & b <device>\t  column=0",
    );
    await expect
      .poll(() =>
        content
          .locator("[data-line]")
          .first()
          .evaluate((el) => {
            const line = el.getBoundingClientRect(),
              area = el.parentElement!.getBoundingClientRect();
            return line.top >= area.top && line.bottom <= area.bottom;
          }),
      )
      .toBe(true);
  });
}
