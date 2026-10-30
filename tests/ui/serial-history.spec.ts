import { test, expect, type Page } from "playwright/test";
import { openUi } from "./helpers/serial-log";

async function seedCommands(page: Page) {
  await page.evaluate(async () => {
    const { useSerialDebugStore } = await import(
      "/src/stores/" + "serial-debug.ts"
    );
    const { recordSendHistory } = await import(
      "/src/features/serial-debug/" + "send-history.ts"
    );
    const store = useSerialDebugStore();
    store.sendHistory = recordSendHistory([], "AT+GMR", "ascii", true, 1000);
    store.sendHistory = recordSendHistory(
      store.sendHistory,
      "55 AA 00 01",
      "hex",
      false,
      2000,
    );
    store.sendHistory = recordSendHistory(
      store.sendHistory,
      "AT+" + "ARG_".repeat(100) + "DISTINCT_END",
      "ascii",
      false,
      3000,
    );
    store.sendInput = "UNSENT";
    store.sendMode = "ascii";
    store.sendAppendCrlf = true;
  });
}

for (const locale of ["en", "zh-CN"])
  for (const theme of ["light", "dark"]) {
    test(`${locale} ${theme}: command preview, settings and draft recovery fit the minimum window`, async ({
      page,
    }, testInfo) => {
      await page.setViewportSize({ width: 1024, height: 680 });
      await openUi(page, "/serial-debug", locale);
      await page.evaluate(async (theme) => {
        const { useSettingsStore } = await import(
          "/src/stores/" + "settings.ts"
        );
        useSettingsStore().setTheme(theme);
      }, theme);
      await seedCommands(page);
      await page.locator(".history-wrap > button").click();
      const panel = page.locator(".history-dropdown");
      await expect(panel.locator(".history-search")).toBeFocused();
      await expect(panel.locator("pre")).toContainText("DISTINCT_END");
      if (locale === "zh-CN" && theme === "dark")
        await page.screenshot({
          path: testInfo.outputPath("command-history.png"),
        });
      await expect
        .poll(() =>
          panel.evaluate((el) => {
            const r = el.getBoundingClientRect();
            return (
              r.left >= 0 &&
              r.right <= innerWidth &&
              r.top >= 0 &&
              r.bottom <= innerHeight
            );
          }),
        )
        .toBe(true);
      const preview = panel.locator("pre");
      expect(
        await preview.evaluate((el) => el.scrollWidth <= el.clientWidth),
      ).toBe(true);
      await expect(page.locator(".send-bar input[type=text]")).toHaveValue(
        "UNSENT",
      );
      await panel.locator(".history-search").fill("55 AA");
      await expect(panel.locator(".history-item")).toHaveCount(1);
      await panel.locator(".history-fill").click();
      await expect(page.locator(".send-bar input[type=text]")).toHaveValue(
        "55 AA 00 01",
      );
      await expect(
        page.locator(".mode-btn").filter({ hasText: "Hex" }),
      ).toHaveAttribute("aria-pressed", "true");
      await expect(page.locator(".check-row input")).not.toBeChecked();
      await page.locator(".restore-draft").click();
      await expect(page.locator(".send-bar input[type=text]")).toHaveValue(
        "UNSENT",
      );
      await expect(page.locator(".check-row input")).toBeChecked();
      await page.locator(".history-wrap > button").click();
      await page.keyboard.press("Escape");
      await expect(panel).toHaveCount(0);
      await expect(page.locator(".history-wrap > button")).toBeFocused();
    });
  }

test("favorites, names and deletion survive workspace reload", async ({
  page,
}) => {
  await openUi(page, "/serial-debug", "en");
  await seedCommands(page);
  await page.locator(".history-wrap > button").click();
  const panel = page.locator(".history-dropdown");
  await panel.locator(".history-search").fill("55 AA");
  await panel.getByRole("button", { name: "Favorite", exact: true }).click();
  await panel
    .getByRole("textbox", { name: "Command name", exact: true })
    .fill("Status query");
  await panel.getByRole("button", { name: "Save name", exact: true }).click();
  await panel.locator(".history-search").fill("Status query");
  await expect(panel.locator(".history-item")).toHaveCount(1);
  await panel
    .getByRole("button", { name: "Close history", exact: true })
    .click();
  await expect
    .poll(() =>
      page.evaluate(() =>
        JSON.parse(
          localStorage.getItem("serial-debug-workspace.v1") ?? "{}",
        ).sendHistory?.some(
          (entry: { name: string; favorite: boolean }) =>
            entry.name === "Status query" && entry.favorite,
        ),
      ),
    )
    .toBe(true);
  await page.reload();
  await page.locator(".history-wrap > button").click();
  await panel
    .getByRole("button", { name: "Favorites only", exact: true })
    .click();
  await expect(panel.locator(".history-item")).toHaveCount(1);
  await expect(panel.locator(".history-item")).toContainText("Status query");
  await panel.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(panel.locator(".history-item")).toHaveCount(0);
  await expect(panel.locator(".history-search")).toBeFocused();
});

test("text-only history migrates with unknown settings and does not overwrite the current mode", async ({
  page,
}) => {
  await page.addInitScript(() =>
    localStorage.setItem(
      "serial-debug-workspace.v1",
      JSON.stringify({
        v: 1,
        sendHistory: ["AT+GMR"],
        sendMode: "hex",
        sendAppendCrlf: false,
      }),
    ),
  );
  await openUi(page, "/serial-debug", "en");
  await page.locator(".history-wrap > button").click();
  const panel = page.locator(".history-dropdown");
  await expect(panel.locator(".history-preview")).toContainText(
    "Settings unknown",
  );
  await panel.locator(".history-fill").click();
  await expect(
    page.locator(".mode-btn").filter({ hasText: "Hex" }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".send-bar input[type=text]")).toHaveValue(
    "AT+GMR",
  );
});

test("history entry, jump to start and live reception preserve the reading position", async ({
  page,
}, testInfo) => {
  await openUi(page, "/serial-debug", "en", [], { total: 5000 });
  await page.getByRole("button", { name: "History", exact: true }).click();
  await expect(page.locator(".history-status")).toContainText(
    "Reading history",
  );
  await page
    .getByRole("button", { name: "Jump to start", exact: true })
    .click();
  await expect(page.locator(".history-status")).toContainText("Lines 1–1200");
  const pane = page.locator(".pane");
  const before = await pane.evaluate((el) => el.scrollTop);
  await page.evaluate(async () => {
    const { useSerialDebugStore } = await import(
      "/src/stores/" + "serial-debug.ts"
    );
    useSerialDebugStore().appendChunk({
      direction: "rx",
      tsMs: Date.now(),
      bytes: [...new TextEncoder().encode("LIVE_NEW\n".repeat(120))],
    });
  });
  await expect(page.locator(".history-status")).toContainText("120 new lines");
  expect(await pane.evaluate((el) => el.scrollTop)).toBe(before);
  await expect(pane).not.toContainText("LIVE_NEW");
  await page.screenshot({ path: testInfo.outputPath("log-history.png") });
  await page.getByRole("button", { name: "Back to live", exact: true }).click();
  await expect(page.locator(".history-status")).toHaveCount(0);
  await expect(pane).toContainText("LIVE_NEW");
});

test("a failed older read keeps the pane and retry restores paging", async ({
  page,
}) => {
  await openUi(page, "/serial-debug", "en", [], {
    total: 5000,
    failOnceAt: 3400,
  });
  await page.getByRole("button", { name: "History", exact: true }).click();
  const pane = page.locator(".pane");
  await pane.evaluate((el) => {
    el.scrollTop = 100;
    el.dispatchEvent(new Event("scroll"));
  });
  const alert = page.getByRole("alert");
  await expect(alert).toContainText("temporary archive failure");
  await expect(page.locator(".history-status")).toContainText(
    "Lines 3801–5000",
  );
  await expect(pane).toContainText("archive-3801");
  await alert.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(alert).toHaveCount(0);
  await expect(page.locator(".history-status")).toContainText(
    "Lines 3401–5000",
  );
});

test("filter results open surrounding archive lines and return to the same results", async ({
  page,
}) => {
  await openUi(page, "/serial-debug", "en", [], { total: 5000 });
  await page.evaluate(async () => {
    const { useSerialDebugStore } = await import(
      "/src/stores/" + "serial-debug.ts"
    );
    const s = useSerialDebugStore();
    s.watchChips = [
      { id: "f1", keyword: "hit", useRegex: false, color: "#000" },
    ];
    s.filterPagesById = {
      f1: {
        filterId: "f1",
        totalMatches: 1,
        start: 0,
        items: [
          {
            id: 42,
            archiveLineNo: 2000,
            direction: "rx",
            text: "hit-at-2000",
            tsMs: 1000,
          },
        ],
      },
    };
    s.activeChipId = "f1";
    s.activeFilterPinned = true;
  });
  await page.getByRole("button", { name: "View context", exact: true }).click();
  await expect(page.locator(".history-status")).toBeVisible();
  await expect(page.locator(".pane")).toContainText("archive-2000");
  await page
    .getByRole("button", { name: "Back to results", exact: true })
    .click();
  await expect(page.locator(".pane")).toContainText("hit-at-2000");
});

test("paused live reading keeps its rows after the 500-line buffer rolls over", async ({
  page,
}) => {
  await openUi(page, "/serial-debug", "en");
  await page.evaluate(async () => {
    const { useSerialDebugStore } = await import(
      "/src/stores/" + "serial-debug.ts"
    );
    const store = useSerialDebugStore();
    store.logWindowLines = 500;
    store.appendChunk({
      direction: "rx",
      tsMs: Date.now(),
      bytes: [
        ...new TextEncoder().encode(
          Array.from({ length: 500 }, (_, i) => `original-${i}\n`).join(""),
        ),
      ],
    });
  });
  const pane = page.locator(".pane");
  await expect(pane).toContainText("original-499");
  await pane.evaluate((el) => {
    el.scrollTop = 2500;
    el.dispatchEvent(new Event("scroll"));
  });
  await expect(page.locator(".paused-badge")).toBeVisible();
  const before = await pane.locator(".line:not(.line-probe)").allTextContents();
  await page.evaluate(async () => {
    const { useSerialDebugStore } = await import(
      "/src/stores/" + "serial-debug.ts"
    );
    useSerialDebugStore().appendChunk({
      direction: "rx",
      tsMs: Date.now(),
      bytes: [...new TextEncoder().encode("new-row\n".repeat(600))],
    });
  });
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const { useSerialDebugStore } = await import(
          "/src/stores/" + "serial-debug.ts"
        );
        return useSerialDebugStore().lines[0]?.text;
      }),
    )
    .toBe("new-row");
  expect(
    await pane.locator(".line:not(.line-probe)").allTextContents(),
  ).toEqual(before);
  await page.locator(".paused-badge").click();
  await expect(pane).toContainText("new-row");
});
