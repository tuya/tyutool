import { test, expect } from "playwright/test";
import { openUi } from "./helpers/serial-log";

test.describe("UI-01 English serial controls do not overlap at minimum window", () => {
  test.setTimeout(15_000);
  for (const mockPort of [false, true]) {
    test(`port ${mockPort ? "available" : "unavailable"}`, async ({ page }) => {
      await page.setViewportSize({ width: 1024, height: 680 });
      await openUi(
        page,
        "/serial-debug",
        "en",
        mockPort ? ["MOCK_PORT_1"] : [],
      );
      if (mockPort) {
        await page.locator("#sd-port").click();
        await expect(
          page.getByRole("option", { name: "MOCK_PORT_1" }),
        ).toBeVisible();
        await page.getByRole("option", { name: "MOCK_PORT_1" }).click();
      }
      const port = page.locator("#sd-port");
      const baud = page.locator("#sd-baud");
      const actions = page.locator(".conn-bar > div").last().locator("button");
      await expect
        .poll(async () => actions.count(), { timeout: 2500 })
        .toBeGreaterThan(0);
      for (const field of [port, baud]) {
        for (const action of await actions.all()) {
          const overlap = await field.evaluate(
            (fieldEl, actionEl) => {
              const a = fieldEl.getBoundingClientRect();
              const b = (actionEl as HTMLElement).getBoundingClientRect();
              return (
                Math.max(
                  0,
                  Math.min(a.right, b.right) - Math.max(a.left, b.left),
                ) *
                Math.max(
                  0,
                  Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top),
                )
              );
            },
            await action.elementHandle(),
          );
          expect(
            overlap,
            `control overlap area for ${await action.innerText()}`,
          ).toBe(0);
        }
      }
    });
  }
});

test.describe("UI-02 firmware Browse is discoverable in the minimum-width default view", () => {
  test("Browse is not clipped and its center hit-tests to the Browse button", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/flash", "en");
    const browse = page.getByRole("button", { name: /Browse/ }).first();
    const centerHit = await browse.evaluate((button) => {
      const r = button.getBoundingClientRect();
      const hit = document.elementFromPoint(
        r.left + r.width / 2,
        r.top + r.height / 2,
      );
      return hit === button || button.contains(hit);
    });
    expect(centerHit).toBe(true);
    const [fileChooser] = await Promise.all([
      page.waitForEvent("filechooser", { timeout: 2500 }),
      browse.click(),
    ]);
    expect(fileChooser).toBeTruthy();
  });
});

test.describe("UI-03 serial settings modal focus management", () => {
  test.setTimeout(15_000);
  async function openSettings(page: import("playwright/test").Page) {
    await openUi(page, "/serial-debug", "en");
    const opener = page.getByRole("button", { name: "Serial Settings" });
    await opener.click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    return { dialog, opener };
  }
  function visibleFocusables(dialog: import("playwright/test").Locator) {
    return dialog
      .locator(
        'button:not([disabled]),input:not([disabled]),select:not([disabled]),[tabindex]:not([tabindex="-1"]):not([disabled])',
      )
      .filter({ visible: true });
  }

  test("opening moves focus into the modal", async ({ page }) => {
    const { dialog } = await openSettings(page);
    await expect
      .poll(
        () => dialog.evaluate((el) => el.contains(document.activeElement)),
        { timeout: 2500 },
      )
      .toBe(true);
  });
  test("Tab from the last control wraps to the first", async ({ page }) => {
    const { dialog } = await openSettings(page);
    const items = await visibleFocusables(dialog);
    const first = items.first();
    await items.last().focus();
    await page.keyboard.press("Tab");
    await expect
      .poll(() => first.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
  test("Shift+Tab from the first control wraps to the last", async ({
    page,
  }) => {
    const { dialog } = await openSettings(page);
    const items = await visibleFocusables(dialog);
    const last = items.last();
    await items.first().focus();
    await page.keyboard.press("Shift+Tab");
    await expect
      .poll(() => last.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
  test("Escape closes and restores focus to the opener", async ({ page }) => {
    const { dialog, opener } = await openSettings(page);
    await visibleFocusables(dialog).first().focus();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect
      .poll(() => opener.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
  test("close button closes and restores focus to the opener", async ({
    page,
  }) => {
    const { dialog, opener } = await openSettings(page);
    await dialog.locator("button").first().click();
    await expect(dialog).toBeHidden();
    await expect
      .poll(() => opener.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
  test("Escape closes the select popup before the settings modal", async ({
    page,
  }) => {
    const { dialog, opener } = await openSettings(page);
    const selectTrigger = dialog.locator(".ty-select-trigger").first();
    await selectTrigger.focus();
    await page.keyboard.press("ArrowDown");
    const listbox = page.getByRole("listbox");
    await expect(listbox).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(listbox).toBeHidden();
    await expect(dialog).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect
      .poll(() => opener.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
});
