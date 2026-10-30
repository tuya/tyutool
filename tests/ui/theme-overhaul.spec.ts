import { test, expect } from "playwright/test";
import { openUi } from "./helpers/serial-log";

for (const [width, height] of [
  [1280, 800],
  [1024, 680],
])
  for (const locale of ["en", "zh-CN"])
    for (const theme of ["light", "dark"])
      for (const path of [
        "/flash",
        "/serial-debug",
        "/toolbox",
        "/toolbox/batch-flash-auth",
        "/settings",
      ]) {
        test(`${width} ${locale} ${theme} ${path}: coordinated page and visible controls`, async ({
          page,
        }) => {
          const errors: string[] = [];
          page.on("pageerror", (error) => errors.push(error.message));
          await page.setViewportSize({ width, height });
          await page.addInitScript(() =>
            localStorage.setItem("tyutool-batch-auth-disclaimer-v1", "true"),
          );
          await page.route("https://**/*", (route) =>
            route.fulfill({ status: 503, body: "UI test: offline" }),
          );
          await openUi(page, path, locale, ["COM6", "COM8"]);
          await page.evaluate(async (theme) => {
            const url = "/src/stores/settings.ts";
            const { useSettingsStore } = await import(url);
            useSettingsStore().setTheme(theme);
          }, theme);
          await expect(
            page.locator(".ty-route-enter-active,.ty-route-leave-active"),
          ).toHaveCount(0);
          await expect(page.locator("#app-main h1")).toHaveCount(1);
          const metrics = await page.evaluate(() => {
            const main = document.querySelector("#app-main")!;
            const toolbar = main.querySelector(".log-toolbar");
            return {
              documentOverflow:
                document.documentElement.scrollWidth - innerWidth,
              mainOverflow: main.scrollWidth - main.clientWidth,
              clippedToolbarControls: toolbar
                ? [...toolbar.querySelectorAll("button")].filter((button) => {
                    const bounds = button.getBoundingClientRect();
                    const area = toolbar.getBoundingClientRect();
                    return (
                      bounds.right > area.right + 1 ||
                      bounds.left < area.left - 1 ||
                      bounds.bottom > area.bottom + 1
                    );
                  }).length
                : 0,
            };
          });
          expect(metrics.documentOverflow).toBeLessThanOrEqual(1);
          expect(metrics.mainOverflow).toBeLessThanOrEqual(1);
          expect(metrics.clippedToolbarControls).toBe(0);
          if (path === "/serial-debug") {
            expect(
              await page
                .locator(".pane")
                .last()
                .evaluate((element) => element.clientHeight),
            ).toBeGreaterThan(120);
          }
          expect(errors).toEqual([]);
        });
      }

test("settings section navigation moves focus and keeps controls available", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 680 });
  await openUi(page, "/settings");
  await page.locator('.settings-nav a[href="#serial-debug-heading"]').click();
  await expect(page.locator("#serial-debug-heading")).toBeFocused();
  await expect(page.locator("#settings-serial-archive-limit")).toBeAttached();
  await page.locator('.settings-nav a[href="#appearance-heading"]').click();
  await expect(page.locator("#appearance-heading")).toBeFocused();
  await expect(page.getByRole("radiogroup")).toBeVisible();
  await page.locator("#app-main").evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect(
    page.locator('.settings-nav a[href="#about-heading"]'),
  ).toHaveAttribute("aria-current", "location");
});

for (const theme of ["light", "dark"] as const) {
  test(`${theme}: firmware progress, results and invalid ranges remain usable in the minimum window`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/flash", "en", ["COM6"]);
    await page.evaluate(async (theme) => {
      const settingsUrl = "/src/stores/settings.ts";
      const flashUrl = "/src/stores/flash.ts";
      const { useSettingsStore } = await import(settingsUrl);
      useSettingsStore().setTheme(theme);
      const { useFlashStore } = await import(flashUrl);
      Object.assign(useFlashStore(), {
        selectedSerialPort: "COM6",
        connected: true,
        flashPhase: "running",
        runningOp: "flash",
        currentBackendPhase: "write",
        phaseProgress: 67,
      });
    }, theme);
    const progress = page.getByRole("progressbar");
    await expect(progress).toHaveAttribute("aria-valuenow", "67");
    await expect(page.locator(".aside-progress-pct")).toHaveText("67%");
    await page.evaluate(async () => {
      const url = "/src/stores/flash.ts";
      const { useFlashStore } = await import(url);
      useFlashStore().phaseIndeterminate = true;
    });
    await expect(progress).not.toHaveAttribute("aria-valuenow");
    await expect(page.locator(".aside-progress-pct")).toHaveCount(0);
    await page.evaluate(async () => {
      const url = "/src/stores/flash.ts";
      const { useFlashStore } = await import(url);
      Object.assign(useFlashStore(), {
        connected: false,
        flashPhase: "success",
        flashMessage: "Firmware verified and serial port released.",
      });
    });
    await expect(progress).toHaveCount(0);
    await expect(page.locator(".aside-alert-success")).toContainText(
      "Firmware verified",
    );
    await page
      .getByRole("button", { name: "Clear result", exact: true })
      .click();
    await expect(page.locator(".aside-alert-success")).toHaveCount(0);
    await page.getByRole("tab", { name: "Read", exact: true }).click();
    await page.locator("#read-end").fill("invalid-end");
    await expect(page.locator("#read-end")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    await expect(page.locator("#read-end")).toHaveAttribute(
      "aria-describedby",
      "read-range-error",
    );
    await expect(page.locator("#read-range-error")).toBeVisible();
    await page.locator("#read-end").fill("0x1000");
    await expect(page.locator("#read-range-error")).toHaveCount(0);
  });

  test(`${theme}: diagnostic logs preserve readability and nested dialog focus`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/settings");
    await page.evaluate(async (theme) => {
      const url = "/src/stores/settings.ts";
      const { useSettingsStore } = await import(url);
      useSettingsStore().setTheme(theme);
      Object.assign(window, {
        __TAURI_INTERNALS__: {
          invoke: async (command: string) => {
            if (command === "list_log_files")
              return [
                {
                  name: "tyutool-2026-10-30.log",
                  sizeBytes: 512,
                  modifiedMs: 1_700_000_000_000,
                },
              ];
            if (command === "read_log_tail")
              return "INFO connected to COM6\nWARN retry on COM6\nERROR timed out\nDEBUG frame received\nTRACE protocol detail";
            if (command === "list_log_file_openers")
              return [{ id: "default", label: "System default" }];
            return null;
          },
        },
      });
    }, theme);
    const opener = page.getByRole("button", {
      name: "View Logs",
      exact: false,
    });
    await opener.click();
    const dialog = page.getByRole("dialog", {
      name: "Application Logs",
      exact: true,
    });
    await dialog
      .getByRole("button", { name: "tyutool-2026-10-30", exact: true })
      .click();
    const content = dialog.locator("[aria-busy]");
    await expect(content.locator("[data-line]")).toHaveCount(5);
    const contrast = await content.evaluate((element) => {
      const canvas = document.createElement("canvas");
      const context = canvas.getContext("2d")!;
      const color = (value: string) => {
        context.clearRect(0, 0, 1, 1);
        context.fillStyle = value;
        context.fillRect(0, 0, 1, 1);
        return [...context.getImageData(0, 0, 1, 1).data];
      };
      const luminance = (rgb: number[]) =>
        rgb.slice(0, 3).reduce((sum, value, index) => {
          const c = value / 255;
          return (
            sum +
            (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4) *
              [0.2126, 0.7152, 0.0722][index]
          );
        }, 0);
      const background = color(getComputedStyle(element).backgroundColor);
      const palette = getComputedStyle(document.documentElement);
      const expectedBackground = color(palette.getPropertyValue("--ty-log-bg"));
      return {
        background,
        expectedBackground,
        ratios: [...element.querySelectorAll("[data-line]")].map((line) => {
          const fg = luminance(color(getComputedStyle(line).color));
          const bg = luminance(background);
          return (Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05);
        }),
      };
    });
    expect(contrast.background).toEqual(contrast.expectedBackground);
    for (const ratio of contrast.ratios)
      expect(ratio).toBeGreaterThanOrEqual(4.5);
    await dialog.getByRole("textbox").fill("COM6");
    await expect(content.locator("[data-line]")).toHaveCount(2);
    await dialog
      .getByRole("button", { name: "Open With...", exact: true })
      .click();
    const nested = page.getByRole("dialog", {
      name: "Choose a text editor",
      exact: true,
    });
    await expect(nested).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(nested).toHaveCount(0);
    await expect(
      dialog.getByRole("button", { name: "Open With...", exact: true }),
    ).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(opener).toBeFocused();
    await page.evaluate(() => {
      delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    });
  });
}

for (const theme of ["light", "dark"] as const) {
  test(`${theme}: workbench palette and theme cards stay coordinated`, async ({
    page,
  }) => {
    await openUi(page, "/settings");
    await page.evaluate(async (theme) => {
      const url = "/src/stores/settings.ts";
      const { useSettingsStore } = await import(url);
      useSettingsStore().setTheme(theme);
    }, theme);
    await expect(page.locator(".ty-route-enter-active")).toHaveCount(0);
    const colors = await page.evaluate(() => {
      const css = getComputedStyle(document.documentElement);
      return ["--ty-canvas", "--ty-primary"].map((key) =>
        css.getPropertyValue(key).trim(),
      );
    });
    expect(colors).toEqual(
      theme === "light" ? ["#f1f2ef", "#006b5e"] : ["#111518", "#74ddc9"],
    );
    await expect(page.locator(".theme-preview")).toHaveCount(3);
    const group = page.getByRole("radiogroup");
    const selected = group.getByRole("radio", { checked: true });
    await selected.focus();
    await page.keyboard.press("ArrowRight");
    await expect(group.getByRole("radio", { checked: true })).toBeFocused();
  });
}
