import { test, expect } from "playwright/test";
import { openUi } from "./helpers/serial-log";

for (const leave of [false, true]) {
  test(`delayed dialog import ${leave ? "cannot reopen after navigation" : "keeps busy feedback and restores opener focus"}`, async ({
    page,
  }) => {
    await openUi(page, "/settings");
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.route(
      "**/src/features/settings/LogViewerDialog.vue*",
      async (route) => {
        await gate;
        await route.continue();
      },
    );
    const opener = page.getByRole("button", {
      name: "View Logs",
      exact: false,
    });
    await opener.click();
    const loading = page.locator('.diagnostics-action-card[aria-busy="true"]');
    await expect(loading).toBeDisabled();
    await expect(loading).toContainText("Loading");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    if (leave) {
      await page.locator('a[href="/serial-debug"]').click();
      await expect(page.locator(".pane .line-probe")).toBeAttached();
    }
    const response = page.waitForResponse((r) =>
      r.url().includes("/LogViewerDialog.vue"),
    );
    release();
    await response;
    if (leave) {
      await expect(page.locator(".send-bar")).toBeVisible();
      await expect(page.getByRole("dialog")).toHaveCount(0);
    } else {
      await expect(page.getByRole("dialog")).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(opener).toBeFocused();
    }
  });
}
