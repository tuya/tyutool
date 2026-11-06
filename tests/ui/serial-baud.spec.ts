import { test, expect } from "playwright/test";
import { openUi } from "./helpers/serial-log";

for (const theme of ["light", "dark"] as const) {
  for (const locale of ["zh-CN", "en"]) {
    test(`${locale} ${theme}: serial baud reuses editable input and preserves custom rates`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 730, height: 764 });
      await openUi(page, "/serial-debug", locale);
      await page.evaluate(async (theme) => {
        const url = "/src/stores/settings.ts";
        const { useSettingsStore } = await import(url);
        useSettingsStore().theme = theme;
        const serialUrl = "/src/stores/serial-debug.ts";
        const { useSerialDebugStore } = await import(serialUrl);
        useSerialDebugStore().customBaudRate = 123456;
      }, theme);
      const input = page.locator("input#sd-baud");
      await expect(input).toHaveValue("123456");
      await expect(page.locator("#sd-custom-baud")).toHaveCount(0);
      const rates = () =>
        page.evaluate(async () => {
          const url = "/src/stores/serial-debug.ts";
          const { useSerialDebugStore } = await import(url);
          const s = useSerialDebugStore();
          return { baud: s.baudRate, custom: s.customBaudRate };
        });
      await input.fill("4294967295");
      await input.press("Enter");
      expect((await rates()).custom).toBe(4294967295);
      await input.fill("4294967296");
      await input.press("Tab");
      await expect(input).toHaveValue("4294967295");
      await input.fill("1");
      await input.press("Enter");
      expect((await rates()).custom).toBe(1);
      await input.press("ArrowDown");
      await page.getByRole("option", { name: "115200", exact: true }).click();
      await expect(input).toHaveValue("115200");
      expect(await rates()).toEqual({ baud: 115200, custom: null });
      await page.evaluate(async () => {
        const url = "/src/stores/serial-debug.ts";
        const { useSerialDebugStore } = await import(url);
        useSerialDebugStore().opening = true;
      });
      await expect(input).toBeDisabled();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
      ).toBe(false);
    });
  }
}
