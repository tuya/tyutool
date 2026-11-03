import { test, expect } from "playwright/test";
import { openUi } from "./helpers/serial-log";
import { remountSettings } from "./helpers/settings";

for (const theme of ["light", "dark"] as const) {
  for (const width of [375, 1024, 1549]) {
    test(`${theme} ${width}: toolbox entry stays compact and opens its tool`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 884 });
      await openUi(page, "/toolbox", "zh-CN");
      await page.evaluate(async (theme) => {
        const url = "/src/stores/settings.ts";
        const { useSettingsStore } = await import(url);
        useSettingsStore().setTheme(theme);
      }, theme);
      await expect(page.locator(".page-header h1")).toHaveText("工具箱");
      await expect(page.locator(".page-header-section")).toHaveCount(0);
      await expect(page.locator(".page-header-desc")).toHaveCount(0);
      const card = page.locator('[data-tool-id="batch-flash-auth"]');
      await expect(card).toContainText("批量烧录授权");
      const bounds = await card.boundingBox();
      expect(bounds!.height).toBeLessThanOrEqual(140);
      expect(bounds!.width).toBeLessThanOrEqual(576);
      expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
      await card.focus();
      await expect(card).toBeFocused();
      await page.keyboard.press("Enter");
      await expect(page).toHaveURL(/\/toolbox\/batch-flash-auth$/);
    });
  }
}

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
          if (path === "/settings") {
            await expect(page.locator(".page-header-section")).toHaveCount(0);
            await expect(page.locator(".page-header-desc")).toHaveCount(0);
            await expect(
              page.locator('.settings-nav a[href="#serial-debug-heading"]'),
            ).toHaveText(locale === "en" ? "Serial Logs" : "串口日志");
            await expect(
              page.locator("#settings-serial-archive-limit-hint"),
            ).toContainText(
              locale === "en"
                ? "Auto-saved log files and batch flash logs are unaffected"
                : "自动保存的日志文件和批量烧录日志不受影响",
            );
          }
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
    await remountSettings(page);
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
      theme === "light" ? ["#f4f8fb", "#1267a5"] : ["#0c141c", "#78c6f4"],
    );
    await expect(page.locator(".theme-preview")).toHaveCount(3);
    const group = page.getByRole("radiogroup");
    const selected = group.getByRole("radio", { checked: true });
    await selected.focus();
    await page.keyboard.press("ArrowRight");
    await expect(group.getByRole("radio", { checked: true })).toBeFocused();
  });
}

for (const theme of ["light", "dark"] as const) {
  test(`${theme}: current log search result keeps timestamps and direction labels readable`, async ({
    page,
  }) => {
    await openUi(page, "/serial-debug");
    await page.evaluate(async (theme) => {
      const settingsUrl = "/src/stores/settings.ts";
      const serialUrl = "/src/stores/serial-debug.ts";
      const { useSettingsStore } = await import(settingsUrl);
      const { useSerialDebugStore } = await import(serialUrl);
      useSettingsStore().setTheme(theme);
      Object.assign(useSerialDebugStore(), {
        showTimestamp: true,
        showDirBadge: true,
        lines: ["tx", "rx", "sys"].map((direction, i) => ({
          id: i + 1,
          tsMs: 1700000000000 + i,
          direction,
          text: "match sample",
        })),
      });
    }, theme);
    await page.locator(".pane").click();
    await page.keyboard.press("Control+f");
    await page.locator(".search-input").fill("match");
    const current = page.locator(".line-search-current");
    await expect(current).toBeVisible();
    for (let i = 0; i < 3; i++) {
      await expect(current).toHaveAttribute("data-dir", ["tx", "rx", "sys"][i]);
      const ratios = await current.evaluate((line) => {
        const context = document.createElement("canvas").getContext("2d")!;
        const rgba = (color: string) => {
          context.clearRect(0, 0, 1, 1);
          context.fillStyle = color;
          context.fillRect(0, 0, 1, 1);
          return [...context.getImageData(0, 0, 1, 1).data];
        };
        const luminance = (rgb: number[]) =>
          rgb.slice(0, 3).reduce((sum, value, j) => {
            const c = value / 255;
            return (
              sum +
              (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4) *
                [0.2126, 0.7152, 0.0722][j]
            );
          }, 0);
        return [...line.querySelectorAll(".ts,.dir-badge")].map((el) => {
          const ancestors: Element[] = [];
          for (let node: Element | null = el; node; node = node.parentElement)
            ancestors.unshift(node);
          const bg = ancestors.reduce(
            (under, node) => {
              const over = rgba(getComputedStyle(node).backgroundColor);
              return under.map(
                (channel, j) =>
                  (over[j] * over[3]) / 255 + channel * (1 - over[3] / 255),
              );
            },
            [255, 255, 255],
          );
          const a = luminance(rgba(getComputedStyle(el).color)),
            b = luminance(bg);
          return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
        });
      });
      expect(ratios).toHaveLength(2);
      for (const ratio of ratios) expect(ratio).toBeGreaterThanOrEqual(4.5);
      await page.locator(".search-input").press("Enter");
    }
  });
}

for (const theme of ["light", "dark"] as const) {
  for (const width of [1024, 1282, 1549]) {
    for (const locale of ["en", "zh-CN"])
      test(`${locale} ${theme} ${width}: compact flash workspace aligns fields and wraps long logs`, async ({
        page,
      }, testInfo) => {
        await page.setViewportSize({
          width,
          height: width === 1024 ? 680 : width === 1282 ? 801 : 884,
        });
        await openUi(page, "/flash", locale);
        await page.evaluate(async (theme) => {
          const settingsUrl = "/src/stores/settings.ts";
          const flashUrl = "/src/stores/flash.ts";
          const { useSettingsStore } = await import(settingsUrl);
          const { useFlashStore } = await import(flashUrl);
          useSettingsStore().setTheme(theme);
          useFlashStore().logLines = ["protocol frame " + "A".repeat(800)];
        }, theme);
        await expect(page.locator(".workbench-location")).toHaveCount(0);
        await expect(page.locator(".page-header-section")).toHaveCount(0);
        await expect(page.locator("#flash-port-feedback")).not.toBeVisible();
        await expect(page.locator(".segment-summary")).toHaveCount(0);
        await expect(
          page.locator(".conn-bar").getByRole("button", {
            name: locale === "en" ? "Refresh" : "刷新",
            exact: true,
          }),
        ).toBeEnabled();
        const log = page.getByRole("log");
        await expect(log).toContainText("A".repeat(800));
        const layout = await page.evaluate(() => {
          const ops = document
            .querySelector(".ops-card")!
            .getBoundingClientRect();
          const log = document.querySelector(".aside-log-body")!;
          return {
            opsWidth: ops.width,
            logWidth: log.getBoundingClientRect().width,
            overflow: log.scrollWidth - log.clientWidth,
          };
        });
        expect(layout.logWidth / layout.opsWidth).toBeLessThanOrEqual(0.51);
        expect(layout.logWidth / layout.opsWidth).toBeGreaterThan(0.45);
        expect(layout.overflow).toBeLessThanOrEqual(1);
        for (const name of locale === "en"
          ? ["Erase", "Read", "Authorize", "Flash"]
          : ["擦除", "读取", "授权", "烧录"]) {
          const tab = page.getByRole("tab", { name, exact: true });
          await tab.click();
          await expect(tab).toHaveAttribute("aria-selected", "true");
          const background = await tab.evaluate((el) => {
            const probe = document.createElement("span");
            probe.style.backgroundColor = "var(--ty-primary-soft)";
            el.append(probe);
            const color = getComputedStyle(probe).backgroundColor;
            probe.remove();
            return color;
          });
          await expect(tab).toHaveCSS("background-color", background);
        }
        await page.evaluate(async () => {
          const url = "/src/stores/flash.ts";
          const { useFlashStore } = await import(url);
          const store = useFlashStore();
          store.flashSegments[0].firmwarePath = "/tmp/demo.bin";
          store.flashSegments[0].firmwareSize = 1024;
        });
        await expect(page.locator(".segment-summary").first()).toContainText(
          "1024",
        );
        if (width === 1549) {
          const alignment = await page
            .locator("table tbody tr")
            .first()
            .evaluate((row) => {
              const start = row
                .querySelector('input[id$="-start"]')!
                .getBoundingClientRect();
              const file = row
                .querySelector('input[id$="-file"]')!
                .getBoundingClientRect();
              return Math.abs(start.top - file.top);
            });
          expect(alignment).toBeLessThanOrEqual(1);
        }
        await page.evaluate(async () => {
          const url = "/src/stores/flash.ts";
          const { useFlashStore } = await import(url);
          useFlashStore().portsRefreshError = "Device enumeration failed";
        });
        await expect(page.locator("#flash-port-feedback")).toContainText(
          "Device enumeration failed",
        );
        await expect(page.locator("#serial-port")).toHaveAttribute(
          "aria-describedby",
          "flash-port-feedback",
        );
        const footer = page.locator(".workbench-sidebar > div").last();
        const footerLayout = await footer.evaluate((element) => {
          const buttons = [...element.querySelectorAll("button")].map(
            (button) => button.getBoundingClientRect(),
          );
          const version = element.lastElementChild!.getBoundingClientRect();
          const sidebar = element.parentElement!.getBoundingClientRect();
          return {
            buttonRowDelta: Math.abs(buttons[0].top - buttons[1].top),
            versionBelowButtons: version.top >= buttons[0].bottom,
            versionCenterOffset: Math.abs(
              version.x + version.width / 2 - (sidebar.x + sidebar.width / 2),
            ),
            controlsFit: buttons.every(
              (rect) =>
                rect.left >= sidebar.left &&
                rect.right <= sidebar.right &&
                rect.bottom <= sidebar.bottom,
            ),
          };
        });
        expect(footerLayout.buttonRowDelta).toBeLessThanOrEqual(1);
        expect(footerLayout.versionBelowButtons).toBe(true);
        expect(footerLayout.versionCenterOffset).toBeLessThanOrEqual(1);
        expect(footerLayout.controlsFit).toBe(true);
        await footer.getByRole("button").first().click();
        await expect
          .poll(() =>
            page.evaluate(() => localStorage.getItem("tyutool-theme")),
          )
          .toBe(theme === "light" ? "dark" : "system");
        await footer.getByRole("button").last().click();
        await expect
          .poll(() =>
            page.evaluate(() => localStorage.getItem("tyutool-locale")),
          )
          .toBe(locale === "en" ? "zh-CN" : "en");
        await footer.getByRole("button").last().click();
        await page.evaluate(async (theme) => {
          const url = "/src/stores/settings.ts";
          const { useSettingsStore } = await import(url);
          useSettingsStore().setTheme(theme);
        }, theme);
        await page.screenshot({
          path: testInfo.outputPath(`compact-flash-${width}.png`),
        });
      });
  }
}

for (const viewport of [
  { width: 1024, height: 680 },
  { width: 1282, height: 801 },
  { width: 1549, height: 884 },
]) {
  test(`${viewport.width}: flash segments stay contained and usable`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize(viewport);
    await openUi(page, "/flash", "zh-CN", ["MOCK_PORT_1"]);
    await page.evaluate(async () => {
      const url = "/src/stores/flash.ts";
      const { useFlashStore } = await import(url);
      const store = useFlashStore();
      store.selectedChipId = "esp32c6";
      store.flashSegments[0].firmwarePath = "C:\\firmware\\bootloader.bin";
      for (let index = 1; index < 8; index++) {
        store.addSegment();
        store.flashSegments[index].firmwarePath =
          `C:\\firmware\\part-${index}.bin`;
      }
    });
    await expect(page.locator(".conn-bar")).toContainText("ESP32-C6");

    const metrics = await page.evaluate(() => {
      const documentElement = document.documentElement;
      const main = document.querySelector<HTMLElement>("#app-main")!;
      const panel = document.querySelector<HTMLElement>(
        '[role="tabpanel"][aria-labelledby$="-flash"]',
      )!;
      const content = panel.parentElement!.parentElement!;
      return {
        documentHeight: documentElement.scrollHeight,
        viewportHeight: documentElement.clientHeight,
        mainHeight: main.scrollHeight,
        mainClientHeight: main.clientHeight,
        contentHeight: content.scrollHeight,
        contentClientHeight: content.clientHeight,
      };
    });
    expect(metrics.documentHeight).toBeLessThanOrEqual(metrics.viewportHeight);
    expect(metrics.mainHeight).toBeLessThanOrEqual(metrics.mainClientHeight);
    expect(metrics.contentHeight).toBeGreaterThan(metrics.contentClientHeight);

    const content = page
      .locator('[role="tabpanel"][aria-labelledby$="-flash"]')
      .locator("xpath=../..");
    await content.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    await expect(
      page.getByRole("button", { name: /添加烧录分段/ }),
    ).toBeInViewport();
    await expect(
      page.getByRole("button", { name: /开始烧录/ }),
    ).toBeInViewport();

    const browse = page.getByRole("button", { name: /浏览/ }).first();
    const [chooser] = await Promise.all([
      page.waitForEvent("filechooser"),
      browse.click(),
    ]);
    await chooser.setFiles({
      name: "selected.bin",
      mimeType: "application/octet-stream",
      buffer: Buffer.from([0x01, 0x02]),
    });
    await expect(
      page.getByRole("textbox", { name: "分段 1: 固件文件" }),
    ).toHaveValue("selected.bin");

    await page.screenshot({
      path: testInfo.outputPath(`firmware-scroll-${viewport.width}.png`),
    });
  });
}
