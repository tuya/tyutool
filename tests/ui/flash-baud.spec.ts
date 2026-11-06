import { test, expect } from "playwright/test";
import { openUi } from "./helpers/serial-log";

for (const theme of ["light", "dark"]) {
  for (const locale of ["zh-CN", "en"]) {
    test(`${locale} ${theme}: firmware baud can be typed or selected at narrow width`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 730, height: 764 });
      await openUi(page, "/flash", locale);
      await page.evaluate(async (theme) => {
        const url = "/src/stores/settings.ts";
        const { useSettingsStore } = await import(url);
        useSettingsStore().theme = theme;
      }, theme);
      const input = page.locator("input#baud-rate");
      await input.fill("123456");
      await input.press("Enter");
      await expect(input).toHaveValue("123456");
      const storedBaud = () =>
        page.evaluate(async () => {
          const url = "/src/stores/flash.ts";
          const { useFlashStore } = await import(url);
          return useFlashStore().selectedBaudRate;
        });
      expect(await storedBaud()).toBe(123456);
      await input.fill("1");
      await input.press("Enter");
      expect(await storedBaud()).toBe(1);
      await input.fill("4294967295");
      await input.press("Enter");
      expect(await storedBaud()).toBe(4294967295);
      await input.fill("4294967296");
      await input.press("Tab");
      await expect(input).toHaveValue("4294967295");
      await input.fill("123456");
      await input.press("Enter");
      await input.fill("0");
      await input.press("Tab");
      await expect(input).toHaveValue("123456");
      expect(await storedBaud()).toBe(123456);
      await input.fill("999");
      await input.press("Escape");
      await expect(input).toHaveValue("123456");
      await input.press("ArrowDown");
      const list = page.getByRole("listbox");
      await expect(list).toBeVisible();
      const bounds = await list.boundingBox();
      expect(bounds!.x).toBeGreaterThanOrEqual(0);
      expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(730);
      await page.getByRole("option", { name: "115200", exact: true }).click();
      await expect(input).toHaveValue("115200");
      await expect(list).toBeHidden();
      expect(await storedBaud()).toBe(115200);
      const tabs = page.locator(".ops-tabs").getByRole("tab");
      await tabs.last().click();
      await expect(page.locator("input#baud-rate")).toHaveCount(0);
      await tabs.first().click();
      await expect(input).toHaveValue("115200");
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
      ).toBe(false);
    });
  }
}
