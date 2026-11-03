import { expect, type Page } from "playwright/test";

// Runtime is fixed before mounting in the app. Tests that install a desktop
// IPC stub after bootstrap must remount the page to reflect that runtime.
export async function remountSettings(page: Page) {
  await page.evaluate(async () => {
    const url = "/src/router/index.ts";
    const { router } = await import(url);
    await router.push("/toolbox");
  });
  await expect(page.locator(".tool-card")).toBeVisible();
  await expect(
    page.locator(".ty-route-enter-active,.ty-route-leave-active"),
  ).toHaveCount(0);
  await page.evaluate(async () => {
    const url = "/src/router/index.ts";
    const { router } = await import(url);
    await router.push("/settings");
  });
  await expect(page.locator(".settings-layout")).toBeVisible();
  await expect(
    page.locator(".ty-route-enter-active,.ty-route-leave-active"),
  ).toHaveCount(0);
}
