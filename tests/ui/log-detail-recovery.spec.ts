import { expect, test, type Page } from "playwright/test";
import { openUi, seedSerialLines } from "./helpers/serial-log";

async function restoreFinished(page: Page, locale: string, theme: string) {
  await page.locator(".ty-route-enter-active").waitFor({ state: "hidden" });
  await page.evaluate(
    async ({ locale, theme }) => {
      const settingsUrl = "/src/stores/settings.ts";
      const settings = (await import(settingsUrl)).useSettingsStore();
      const flashUrl = "/src/stores/flash.ts";
      const serialUrl = "/src/stores/serial-debug.ts";
      await (await import(flashUrl)).useFlashStore().loadWorkspace();
      await (await import(serialUrl)).useSerialDebugStore().loadWorkspace();
      settings.setLocale(locale);
      settings.setTheme(theme);
    },
    { locale, theme },
  );
}

for (const locale of ["en", "zh-CN"]) {
  for (const theme of ["light", "dark"]) {
    for (const viewport of [
      { width: 1024, height: 680 },
      { width: 1280, height: 800 },
    ]) {
      const label = `${locale} ${theme} ${viewport.width}`;
      const zh = locale === "zh-CN";

      test(`long flash paths keep actions and logs visible: ${label}`, async ({
        page,
      }, info) => {
        await page.setViewportSize(viewport);
        await openUi(page, "/flash", locale);
        await restoreFinished(page, locale, theme);
        const outputPath =
          "/home/user/projects/" +
          "deep-project-path/".repeat(18) +
          "firmware.bin";
        await page.evaluate(async (outputPath) => {
          const url = "/src/stores/flash.ts";
          const store = (await import(url)).useFlashStore();
          store.operationResult = {
            op: "read",
            chipId: "t5ai",
            port: "/dev/serial/by-id/" + "usb-device-long-name-".repeat(6),
            baud: 115200,
            outputPath,
            authRead: false,
            status: "success",
            elapsedSecs: 3,
            message: "Read complete",
          };
          store.flashPhase = "success";
          store.flashMessage = "Read complete";
          store.logLines = ["Read complete"];
          let fail = true;
          Object.defineProperty(navigator.clipboard, "writeText", {
            configurable: true,
            value: async (text: string) => {
              if (fail) {
                fail = false;
                throw new Error("denied");
              }
              (window as unknown as { copiedPath: string }).copiedPath = text;
            },
          });
        }, outputPath);
        const result = page.locator(".flash-result");
        const expand = result.getByRole("button", {
          name: zh ? "展开完整路径" : "Show full paths",
          exact: true,
        });
        await expect(expand).toHaveAttribute("aria-expanded", "false");
        await expand.click();
        await expect(
          result.getByRole("button", {
            name: zh ? "收起路径" : "Collapse paths",
          }),
        ).toHaveAttribute("aria-expanded", "true");
        const copy = result.getByRole("button", {
          name: zh ? "复制输出路径" : "Copy output path",
          exact: true,
        });
        await copy.click();
        await expect(result).toContainText(zh ? "复制失败" : "Copy failed");
        await copy.click();
        await expect
          .poll(() =>
            page.evaluate(
              () => (window as unknown as { copiedPath: string }).copiedPath,
            ),
          )
          .toBe(outputPath);
        for (const selector of [".flash-result", ".aside-log-card"]) {
          const box = await page.locator(selector).boundingBox();
          expect(box!.y + box!.height).toBeLessThanOrEqual(viewport.height);
          expect(
            await page
              .locator(selector)
              .evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
          ).toBe(true);
        }
        expect(
          (await page.locator(".aside-log-card").boundingBox())!.height,
        ).toBeGreaterThanOrEqual(128);
        for (const b of await result.getByRole("button").all())
          await expect(b).toBeInViewport({ ratio: 1 });
        await page.screenshot({ path: info.outputPath("expanded-result.png") });
      });

      test(`large diagnostic tail keeps wrapping and full-file navigation: ${label}`, async ({
        page,
      }, info) => {
        await page.setViewportSize(viewport);
        await openUi(page, "/settings", locale);
        await restoreFinished(page, locale, theme);
        await page.evaluate(() => {
          Object.defineProperty(window, "__TAURI_INTERNALS__", {
            configurable: true,
            value: {
              invoke: async (command: string) => {
                if (command === "list_log_files")
                  return [
                    {
                      name: "tyutool-large.log",
                      sizeBytes: 200_000,
                      modifiedMs: 0,
                    },
                  ];
                if (command === "read_log_tail")
                  return Array.from(
                    { length: 12_000 },
                    (_, i) => `INFO entry-${i} <device>`,
                  ).join("\n");
                return [];
              },
            },
          });
        });
        await page
          .getByRole("button", {
            name: zh ? "查看日志" : "View Logs",
          })
          .click();
        await page.locator(".log-file-button").click();
        const dialog = page.getByRole("dialog");
        await expect(dialog.locator("[data-line]")).toHaveCount(300);
        await expect(dialog).toContainText("entry-11999");
        await dialog
          .getByRole("button", {
            name: zh ? "上一段" : "Previous lines",
            exact: true,
          })
          .click();
        await expect(dialog).toContainText("entry-11400");
        await dialog.getByRole("textbox").fill("<device>");
        await expect(dialog).toContainText(
          zh ? "第 1 / 共 12000 条" : "1 of 12000",
        );
        await expect(dialog.locator("[data-line]").first()).toHaveText(
          "INFO entry-0 <device>",
        );
        await dialog
          .getByRole("button", {
            name: zh ? "上一个匹配" : "Previous match",
            exact: true,
          })
          .click();
        await expect(dialog).toContainText(
          zh ? "第 12000 / 共 12000 条" : "12000 of 12000",
        );
        await expect(dialog.locator("[data-line] mark")).toHaveCount(300);
        expect(
          await dialog
            .locator("[data-line]")
            .first()
            .evaluate((el) => getComputedStyle(el).whiteSpace),
        ).toBe("pre-wrap");
        const box = await dialog.boundingBox();
        expect(box!.y).toBeGreaterThanOrEqual(0);
        expect(box!.y + box!.height).toBeLessThanOrEqual(viewport.height);
        expect(
          await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
        ).toBe(true);
        await page.screenshot({
          path: info.outputPath("diagnostic-search.png"),
        });
      });

      test(`serial find respects IME and export failures remain retryable: ${label}`, async ({
        page,
      }) => {
        await page.setViewportSize(viewport);
        const archive: { total: number; failOnceAt?: number } = { total: 0 };
        await openUi(page, "/serial-debug", locale, [], archive);
        await restoreFinished(page, locale, theme);
        await seedSerialLines(page, [
          { id: 10001, tsMs: 1, direction: "rx", text: "ab\x1b[31mcd\x1b[0m" },
          { id: 10002, tsMs: 2, direction: "rx", text: "abcd" },
        ]);
        await page.evaluate(async () => {
          const url = "/src/stores/serial-debug.ts";
          (await import(url)).useSerialDebugStore().ansiEnabled = true;
        });
        await page
          .getByRole("button", { name: zh ? "查找" : "Find", exact: true })
          .click();
        const input = page.locator(".search-input");
        await input.fill("abcd");
        await expect(page.locator(".line-search-current mark")).toHaveCount(2);
        const counter = page.locator(".match-count");
        const before = await counter.textContent();
        const prevented = await input.evaluate((el) => {
          const results = [];
          for (const key of ["Enter", "Escape"]) {
            const event = new KeyboardEvent("keydown", {
              key,
              bubbles: true,
              cancelable: true,
              isComposing: true,
            });
            el.dispatchEvent(event);
            results.push(event.defaultPrevented);
          }
          return results;
        });
        expect(prevented).toEqual([false, false]);
        await expect(input).toBeVisible();
        await expect(counter).toHaveText(before!);
        await input.press("Enter");
        await expect(counter).not.toHaveText(before!);
        await input.press("Escape");
        archive.total = 1;
        archive.failOnceAt = 0;
        const save = page.getByRole("button", {
          name: zh ? "保存日志" : "Save log",
          exact: true,
        });
        await save.click();
        await expect(page.getByRole("alert")).toContainText(
          zh ? "保存日志失败" : "Could not save the log",
        );
        await expect(save).toBeEnabled();

        const download = page.waitForEvent("download");
        await save.click();
        await download;
        await expect(
          page
            .getByRole("status")
            .filter({ hasText: zh ? "日志已保存" : "Log saved" }),
        ).toContainText(zh ? "日志已保存" : "Log saved");
      });
    }
  }
}
