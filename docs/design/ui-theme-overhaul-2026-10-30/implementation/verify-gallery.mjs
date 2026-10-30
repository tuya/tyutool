import { chromium, expect } from "playwright/test";
const baseURL = process.env.TYUTOOL_UI_BASE_URL || "http://127.0.0.1:1430";
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1024, height: 680 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
await page.goto(
  baseURL +
    "/docs/design/ui-theme-overhaul-2026-10-30/implementation/gallery.html",
);
let checked = 0;
for (const kind of ["page", "state"]) {
  await page.locator("#kind").selectOption(kind);
  const cases = await page
    .locator("#case option")
    .evaluateAll((es) => es.map((e) => e.value));
  const profiles = await page
    .locator("#profile option")
    .evaluateAll((es) => es.map((e) => e.value));
  for (const item of cases)
    for (const profile of profiles)
      for (const theme of ["light", "dark"]) {
        await page.locator("#case").selectOption(item);
        await page.locator("#profile").selectOption(profile);
        await page.locator("#theme").selectOption(theme);
        await expect
          .poll(() =>
            page
              .locator("#preview")
              .evaluate(
                (e) =>
                  e.complete &&
                  e.naturalWidth === Number(e.getAttribute("width")) &&
                  e.naturalHeight === Number(e.getAttribute("height")),
              ),
          )
          .toBe(true);
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth - innerWidth,
          ),
        ).toBeLessThanOrEqual(1);
        checked++;
      }
}
await page.goto(new URL("./gallery.html", import.meta.url).href);
await expect
  .poll(() =>
    page.locator("#preview").evaluate((e) => e.complete && e.naturalWidth > 0),
  )
  .toBe(true);
expect(errors).toEqual([]);
console.log(
  JSON.stringify({ checked, status: "passed", directFileOpen: "passed" }),
);
await browser.close();
