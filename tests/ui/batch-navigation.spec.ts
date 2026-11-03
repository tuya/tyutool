import { test, expect } from "playwright/test";
import { openUi } from "./helpers/serial-log";

for (const locale of ["en", "zh-CN"]) {
  test(`${locale}: batch page lock blocks navigation until unlocked`, async ({
    page,
  }) => {
    await page.addInitScript(() =>
      localStorage.setItem("tyutool-batch-auth-disclaimer-v1", "true"),
    );
    await openUi(page, "/toolbox/batch-flash-auth", locale);
    const lock = page.getByRole("button", {
      name: locale === "en" ? "Lock page" : "锁定界面",
      exact: true,
    });
    await expect(lock).toHaveAttribute("aria-pressed", "false");
    await expect(
      page.getByRole("navigation", { name: "breadcrumb" }),
    ).toHaveCount(0);
    await expect(page.locator(".page-header-desc")).toHaveCount(0);
    await lock.click();
    const unlock = page.getByRole("button", {
      name: locale === "en" ? "Unlock page" : "解锁界面",
      exact: true,
    });
    await expect(unlock).toHaveAttribute("aria-pressed", "true");
    for (const path of ["/flash", "/serial-debug", "/toolbox", "/settings"]) {
      const blocked = await page.evaluate(async (path) => {
        const url = "/src/router/index.ts";
        const { router } = await import(url);
        return Boolean(await router.push(path));
      }, path);
      expect(blocked).toBe(true);
      await expect(page).toHaveURL(/\/toolbox\/batch-flash-auth$/);
      await expect(page).toHaveTitle(
        locale === "en"
          ? "Batch Flash & Auth · tyutool"
          : "批量烧录授权 · tyutool",
      );
      await page.locator(`.app-shell aside a[href="${path}"]`).click();
      await expect(unlock).toBeVisible();
    }
    await unlock.focus();
    await page.keyboard.press("Enter");
    await expect(lock).toHaveAttribute("aria-pressed", "false");
    await page.locator('.app-shell aside a[href="/settings"]').click();
    await expect(page).toHaveURL(/\/settings$/);
    await page.evaluate(async () => {
      const url = "/src/router/index.ts";
      const { router } = await import(url);
      await router.push("/toolbox/batch-flash-auth");
    });
    await expect(lock).toHaveAttribute("aria-pressed", "false");
    await lock.click();
    await page.reload();
    await expect(lock).toHaveAttribute("aria-pressed", "false");
  });
}

test("batch page lock restores browser back and forward history after unlocking", async ({
  page,
}) => {
  await page.addInitScript(() =>
    localStorage.setItem("tyutool-batch-auth-disclaimer-v1", "true"),
  );
  await openUi(page, "/flash");
  await page.evaluate(async () => {
    const url = "/src/router/index.ts";
    const { router } = await import(url);
    await router.push("/toolbox/batch-flash-auth");
    await router.push("/settings");
  });
  await page.goBack();
  const lock = page.getByRole("button", { name: "Lock page", exact: true });
  await lock.click();
  for (const direction of ["back", "forward"] as const) {
    const path = await page.evaluate(async (direction) => {
      const url = "/src/router/index.ts";
      const { router } = await import(url);
      return new Promise<string>((resolve) => {
        const remove = router.afterEach(() => {
          remove();
          resolve(router.currentRoute.value.path);
        });
        window.history[direction]();
      });
    }, direction);
    expect(path).toBe("/toolbox/batch-flash-auth");
    await expect(page).toHaveURL(/\/toolbox\/batch-flash-auth$/);
  }
  await page.getByRole("button", { name: "Unlock page", exact: true }).click();
  await page.goBack();
  await expect(page).toHaveURL(/\/flash$/);
  await page.goForward();
  await expect(lock).toHaveAttribute("aria-pressed", "false");
  await page.goForward();
  await expect(page).toHaveURL(/\/settings$/);
});
