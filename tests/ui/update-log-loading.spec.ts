import { test, expect } from "playwright/test";
import { openUi, seedSerialLines } from "./helpers/serial-log";

for (const locale of ["en", "zh-CN"])
  for (const theme of ["light", "dark"])
    for (const viewport of [
      { width: 1024, height: 680 },
      { width: 1280, height: 800 },
    ]) {
      const label = `${locale} ${theme} ${viewport.width}`;
      test(`sticky log context and Unicode highlights: ${label}`, async ({
        page,
      }) => {
        await page.setViewportSize(viewport);
        await openUi(page, "/serial-debug", locale);
        await page.evaluate(
          async ({ theme, locale }) => {
            const url = "/src/stores/settings.ts";
            const { useSettingsStore } = await import(url);
            await useSettingsStore().ready();
            useSettingsStore().setTheme(theme);
            useSettingsStore().setLocale(locale);
          },
          { theme, locale },
        );
        await seedSerialLines(
          page,
          Array.from({ length: 80 }, (_, i) => ({
            id: 10000 + i,
            tsMs: 1700000000000 + i,
            direction: i % 2 ? "tx" : "rx",
            text: `İ \x1b[31mER\x1b[0mROR ${"protocol-field ".repeat(80)}`,
          })),
        );
        const pane = page.locator(".pane");
        await pane.click();
        await page.keyboard.press("Control+f");
        await page.locator(".search-input").fill("error");
        const current = page.locator(".line-search-current");
        await expect(current).toBeVisible();
        expect(await current.locator("mark").allTextContents()).toEqual([
          "ER",
          "ROR",
        ]);
        const prefixLeft = await current
          .locator(".prefix")
          .evaluate((el) => el.getBoundingClientRect().left);
        await pane.evaluate((el: HTMLElement) => {
          el.scrollLeft = 500;
        });
        await expect
          .poll(() =>
            current
              .locator(".prefix")
              .evaluate((el) => el.getBoundingClientRect().left),
          )
          .toBe(prefixLeft);
        const coverage = await current.evaluate((el) => {
          const prefix = el.querySelector(".prefix")!.getBoundingClientRect();
          const text = el.querySelector(".text")!.getBoundingClientRect();
          return {
            top: prefix.top <= text.top,
            bottom: prefix.bottom >= text.bottom,
          };
        });
        expect(coverage).toEqual({ top: true, bottom: true });
        const text = await current.locator(".text").innerText();
        expect(text.startsWith("İ ERROR ")).toBe(true);
        const before = await pane.evaluate((el: HTMLElement) => ({
          top: el.scrollTop,
          left: el.scrollLeft,
        }));
        await page.locator('a[href="/settings"]').click();
        await expect(page.locator(".settings-layout")).toBeVisible();
        await page.locator('a[href="/serial-debug"]').click();
        await expect
          .poll(() =>
            pane.evaluate((el: HTMLElement) => ({
              top: el.scrollTop,
              left: el.scrollLeft,
            })),
          )
          .toEqual(before);
        await expect(page.locator(".ty-route-enter-active")).toHaveCount(0);
        await page.screenshot({
          path: `/tmp/tyutool-log-${locale}-${theme}-${viewport.width}.png`,
        });
      });

      test(`update retry, source versions and native ARM64 download: ${label}`, async ({
        page,
      }) => {
        await page.setViewportSize(viewport);
        await openUi(page, "/settings", locale);
        await page.evaluate(
          async ({ theme, locale, width }) => {
            const url = "/src/stores/settings.ts";
            const { useSettingsStore } = await import(url);
            await useSettingsStore().ready();
            useSettingsStore().setTheme(theme);
            useSettingsStore().setLocale(locale);
            let githubChecks = 0;
            Object.assign(window, {
              openedUpdateUrls: [],
              __TAURI_INTERNALS__: {
                invoke: async (command: string, args?: { url?: string }) => {
                  if (command === "get_install_type") return "portable";
                  if (command === "get_update_platform")
                    return width === 1280 ? "" : "linux-aarch64";
                  if (command === "fetch_url") {
                    const github = args?.url?.includes("github");
                    if (github && ++githubChecks === 1)
                      throw new Error("Synthetic offline");
                    return JSON.stringify({
                      version: github
                        ? githubChecks === 2
                          ? "9.9.11"
                          : "9.9.13"
                        : "9.9.12",
                      notes: "Release notes",
                      platforms: {},
                      cli: {},
                      portable: {
                        "linux-aarch64": { url: "https://example.test/arm64" },
                        "linux-x86_64": { url: "https://example.test/x64" },
                      },
                    });
                  }
                  if (command.startsWith("plugin:opener|"))
                    (
                      window as unknown as { openedUpdateUrls: string[] }
                    ).openedUpdateUrls.push(args?.url ?? "");
                  return null;
                },
              },
            });
            const app = document.querySelector("#app") as HTMLElement & {
              __vue_app__: {
                config: {
                  globalProperties: {
                    $router: { push(path: string): Promise<unknown> };
                  };
                };
              };
            };
            await app.__vue_app__.config.globalProperties.$router.push(
              "/settings?update=1",
            );
          },
          { theme, locale, width: viewport.width },
        );
        const dialog = page.locator(".ty-dialog-container");
        await expect(dialog.locator(".ud-overview-highlight-value")).toHaveText(
          "v9.9.12",
        );
        const failed = dialog.locator('[data-source-id="github"]');
        await failed
          .getByRole("button", {
            name: locale === "en" ? "Retry" : "重试",
            exact: true,
          })
          .click();
        await expect(failed).toContainText("9.9.11");
        await expect(dialog.locator(".ud-overview-highlight-value")).toHaveText(
          "v9.9.12",
        );
        await dialog
          .getByRole("button", {
            name: locale === "en" ? "Check again" : "重新检查",
            exact: true,
          })
          .click();
        await expect(dialog.locator(".ud-overview-highlight-value")).toHaveText(
          "v9.9.13",
        );
        if (viewport.width === 1280) {
          await dialog.locator("#update-platform").click();
          await page
            .getByRole("option", {
              name: locale === "en" ? "Linux (ARM64)" : "Linux（ARM64）",
              exact: true,
            })
            .click();
          await expect(dialog.locator("#update-platform")).toContainText(
            "ARM64",
          );
        }
        await expect(
          dialog.locator(".ud-overview-actions .ud-btn-primary"),
        ).toHaveText(locale === "en" ? "Download package" : "下载安装包");
        await dialog.locator(".ud-overview-actions .ud-btn-primary").click();
        await expect
          .poll(() =>
            page.evaluate(
              () =>
                (window as unknown as { openedUpdateUrls: string[] })
                  .openedUpdateUrls,
            ),
          )
          .toEqual(["https://example.test/arm64"]);
        expect(
          await dialog.evaluate((el) => el.getBoundingClientRect().right),
        ).toBeLessThanOrEqual(viewport.width);
        await page.screenshot({
          path: `/tmp/tyutool-update-${locale}-${theme}-${viewport.width}.png`,
        });
        await page.keyboard.press("Escape");
        await expect(dialog).not.toBeVisible();
        const opener = page.locator(".update-entry-card--hero");
        await opener.click();
        await expect(dialog).toBeVisible();
        await page.keyboard.press("Escape");
        await expect(opener).toBeFocused();
      });
    }
