import { test, expect, type Page } from "playwright/test";
import {
  appendAlternatingLines,
  makeMixedSerialLines,
  openUi,
  seedSerialLines,
  SERIAL_STORE_MODULE_URL,
  type SerialDebugBrowserStore,
} from "./helpers/serial-log";

async function setDisplayState(
  page: Page,
  state: { timestamps?: boolean; badges?: boolean; fontSize?: number },
) {
  const moduleUrl = SERIAL_STORE_MODULE_URL;
  await page.evaluate(
    async ({ moduleUrl, state }) => {
      const module = (await import(moduleUrl)) as {
        useSerialDebugStore: () => SerialDebugBrowserStore;
      };
      const store = module.useSerialDebugStore();
      if (state.timestamps !== undefined)
        store.showTimestamp = state.timestamps;
      if (state.badges !== undefined) store.showDirBadge = state.badges;
      if (state.fontSize !== undefined) store.logFontSize = state.fontSize;
    },
    { moduleUrl, state },
  );
}

test.describe("LOG-01 virtual row geometry from actual appendChunk lines", () => {
  test.setTimeout(15_000);
  for (const viewport of [
    { width: 1024, height: 680 },
    { width: 1280, height: 800 },
  ]) {
    for (const timestamps of [true, false]) {
      for (const badges of [true, false]) {
        for (const fontSize of [10, 12, 18]) {
          test(`${viewport.width}x${viewport.height}, timestamp=${timestamps}, badge=${badges}, font=${fontSize}`, async ({
            page,
          }) => {
            await page.setViewportSize(viewport);
            await openUi(page, "/serial-debug", "zh-CN");
            await setDisplayState(page, { timestamps, badges, fontSize });
            await appendAlternatingLines(page, 90);
            const pane = page.locator(".pane").first();
            await expect
              .poll(() => pane.locator("[data-line-id]").count(), {
                timeout: 2500,
              })
              .toBeGreaterThan(0);
            await pane.evaluate((el: HTMLElement) => {
              el.scrollTop = el.scrollHeight;
            });

            const measure = () =>
              pane.evaluate((el) => {
                const probe = el.querySelector<HTMLElement>(".line-probe")!;
                const rows = [
                  ...el.querySelectorAll<HTMLElement>(".line[data-line-id]"),
                ];
                const blank = rows.find(
                  (row) => row.querySelector(".text")?.textContent === "",
                )!;
                const normal = rows.find(
                  (row) => row.querySelector(".text")?.textContent !== "",
                )!;
                const last = rows.at(-1)!;
                const rect = (node: Element) => node.getBoundingClientRect();
                return {
                  probeHeight: rect(probe).height,
                  blankHeight: rect(blank).height,
                  normalHeight: rect(normal).height,
                  bottomGap: rect(el).bottom - rect(last).bottom,
                  lastId: Number(last.dataset.lineId),
                };
              });
            await expect
              .poll(
                async () => {
                  const m = await measure();
                  return Math.max(
                    Math.abs(m.blankHeight - m.probeHeight),
                    Math.abs(m.normalHeight - m.probeHeight),
                  );
                },
                { timeout: 2500 },
              )
              .toBeLessThanOrEqual(1);
            const tail = await measure();
            expect(tail.bottomGap).toBeLessThanOrEqual(tail.probeHeight + 1);
            expect(tail.lastId).toBe(180);

            await pane.evaluate((el: HTMLElement) => {
              el.scrollTop = el.scrollHeight / 2;
            });
            await expect
              .poll(() => pane.locator("[data-line-id]").count(), {
                timeout: 2500,
              })
              .toBeGreaterThan(0);
            await pane.evaluate((el: HTMLElement) => {
              el.scrollTop = el.scrollHeight;
            });
            await expect
              .poll(async () => (await measure()).lastId, { timeout: 2500 })
              .toBe(180);
          });
        }
      }
    }
  }

  test("normal non-empty control rows preserve probe height", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/serial-debug", "zh-CN");
    await seedSerialLines(page, makeMixedSerialLines(180));
    const pane = page.locator(".pane").first();
    await pane.evaluate((el: HTMLElement) => {
      el.scrollTop = el.scrollHeight;
    });
    const heights = await pane.evaluate((el) => {
      const probe = el
        .querySelector(".line-probe")!
        .getBoundingClientRect().height;
      return [...el.querySelectorAll(".line[data-line-id]")].map((row) =>
        Math.abs(row.getBoundingClientRect().height - probe),
      );
    });
    expect(heights.length).toBeGreaterThan(0);
    expect(Math.max(...heights)).toBeLessThanOrEqual(1);
  });

  test("appendChunk splits repeated newlines into empty real log lines", async ({
    page,
  }) => {
    await openUi(page, "/serial-debug", "zh-CN");
    const moduleUrl = SERIAL_STORE_MODULE_URL;
    const added = await page.evaluate(async (moduleUrl) => {
      const module = (await import(moduleUrl)) as {
        useSerialDebugStore: () => SerialDebugBrowserStore;
      };
      const store = module.useSerialDebugStore();
      const firstId = store.lines.at(-1)?.id ?? 0;
      store.appendChunk({
        direction: "rx",
        tsMs: Date.now(),
        bytes: [65, 10, 10, 66, 10, 10],
      });
      return store.lines
        .filter((line) => line.id > firstId)
        .map((line) => line.text);
    }, moduleUrl);
    expect(added).toEqual(["A", "", "B", ""]);
  });

  test("tail viewport has no large blank band after repeated empty lines", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/serial-debug", "zh-CN");
    await setDisplayState(page, { timestamps: false, badges: false });
    await appendAlternatingLines(page, 90);
    const pane = page.locator(".pane").first();
    await pane.evaluate((el: HTMLElement) => {
      el.scrollTop = el.scrollHeight;
    });
    const measureTail = () =>
      pane.evaluate((el) => {
        const rows = [...el.querySelectorAll<HTMLElement>("[data-line-id]")];
        const last = rows.at(-1)!;
        const probe = el.querySelector<HTMLElement>(".line-probe")!;
        return {
          lastId: Number(last.dataset.lineId),
          atBottom: el.scrollHeight - el.clientHeight - el.scrollTop < 1,
          gap:
            el.getBoundingClientRect().bottom -
            last.getBoundingClientRect().bottom,
          probeHeight: probe.getBoundingClientRect().height,
        };
      });
    await expect
      .poll(measureTail, { timeout: 2500 })
      .toMatchObject({ lastId: 180, atBottom: true });
    const tail = await measureTail();
    expect(
      tail.gap,
      "blank space between last mounted row and pane bottom",
    ).toBeLessThanOrEqual(tail.probeHeight + 1);
  });

  test("follow-tail state returns to the newest row after cached-page navigation", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/serial-debug", "zh-CN");
    await seedSerialLines(page, makeMixedSerialLines(180));
    const pane = page.locator(".pane").first();
    await pane.evaluate((el: HTMLElement) => {
      el.scrollTop = el.scrollHeight;
    });
    await page.getByRole("link", { name: "工具箱" }).click();
    await expect(page.getByRole("heading", { name: "工具箱" })).toBeVisible();
    await expect(pane).toHaveCount(0);
    await page.getByRole("link", { name: "串口调试" }).click();
    await expect(pane).toBeVisible();
    await expect
      .poll(
        () =>
          pane.locator("[data-line-id]").last().getAttribute("data-line-id"),
        { timeout: 2500 },
      )
      .toBe("180");
  });

  test("paused middle reading position is preserved after cached-page navigation", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/serial-debug", "zh-CN");
    await seedSerialLines(page, makeMixedSerialLines(360));
    const pane = page.locator(".pane").first();
    await pane.evaluate((el: HTMLElement) => {
      el.scrollTop = el.scrollHeight / 2;
    });
    await expect(
      page.getByRole("button", { name: "滚动到底部" }),
    ).toBeVisible();
    const visibleBefore = await pane
      .locator("[data-line-id]")
      .first()
      .getAttribute("data-line-id");
    const offsetBefore = await pane.evaluate((el: HTMLElement) => el.scrollTop);
    await page.getByRole("link", { name: "工具箱" }).click();
    await expect(page.getByRole("heading", { name: "工具箱" })).toBeVisible();
    await expect(pane).toHaveCount(0);
    await page.getByRole("link", { name: "串口调试" }).click();
    await expect(pane).toBeVisible();
    await expect
      .poll(
        () =>
          pane.locator("[data-line-id]").first().getAttribute("data-line-id"),
        { timeout: 2500 },
      )
      .toBe(visibleBefore);
    await expect
      .poll(() => pane.evaluate((el: HTMLElement) => el.scrollTop), {
        timeout: 2500,
      })
      .toBe(offsetBefore);
  });
});

test.describe("LOG-02 timestamp and direction toggle semantics", () => {
  test("aria-pressed tracks both display toggles independently", async ({
    page,
  }) => {
    await openUi(page, "/serial-debug", "zh-CN");
    const timestamp = page.getByRole("button", { name: "时间戳" });
    const direction = page.getByRole("button", { name: "标签" });
    await expect(timestamp).toHaveAttribute("aria-pressed", "true");
    await expect(direction).toHaveAttribute("aria-pressed", "true");
    await timestamp.click();
    await expect(timestamp).toHaveAttribute("aria-pressed", "false");
    await expect(direction).toHaveAttribute("aria-pressed", "true");
    await direction.click();
    await expect(direction).toHaveAttribute("aria-pressed", "false");
  });
});

test.describe("LOG-03 @proposal: keep row prefixes visible during horizontal scroll", () => {
  test("timestamp/direction prefix stays within the log pane while content scrolls horizontally", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/serial-debug", "zh-CN");
    await seedSerialLines(page, makeMixedSerialLines(80));
    const pane = page.locator(".pane").first();
    await pane.evaluate((el: HTMLElement) => {
      el.scrollLeft = 500;
    });
    const prefix = pane.locator(".line[data-line-id] .prefix").last();
    await expect
      .poll(
        async () => prefix.evaluate((el) => el.getBoundingClientRect().left),
        { timeout: 2500 },
      )
      .toBeGreaterThanOrEqual(
        await pane.evaluate((el) => el.getBoundingClientRect().left),
      );
  });
});

test.describe("LOG-04 find and persistent-filter interaction", () => {
  test("Ctrl+F opens transient find, highlights matches, and keeps navigation available", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/serial-debug", "zh-CN");
    await seedSerialLines(page, makeMixedSerialLines(80));
    await page.locator(".pane").click();
    await page.keyboard.press("Control+f");
    const search = page.locator(".search-input");
    await expect(search).toBeVisible();
    await search.fill("sensor=");
    await expect(page.locator(".line-search-current")).toBeVisible();
    const countText = await page.locator(".match-count").innerText();
    const totalMatches = Number(countText.split("/").at(-1)?.trim());
    expect(totalMatches).toBeGreaterThan(1);
    const searchButtons = page.locator(".search-bar button");
    await expect(searchButtons).toHaveCount(4);
    const current = page.locator(".line-search-current");
    const firstMatchId = await current.getAttribute("data-line-id");
    await searchButtons.nth(2).click();
    await expect
      .poll(() => current.getAttribute("data-line-id"), { timeout: 2500 })
      .not.toBe(firstMatchId);
    await searchButtons.nth(1).click();
    await expect
      .poll(() => current.getAttribute("data-line-id"), { timeout: 2500 })
      .toBe(firstMatchId);
    // The first search-bar action remains whole-session search; the persistent-filter add control remains separate.
    await expect(searchButtons.first()).toBeEnabled();
    await expect(page.locator(".tab-add")).toBeVisible();
  });
});
