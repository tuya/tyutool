import { test, expect } from "playwright/test";
import { openUi } from "./helpers/serial-log";

for (const locale of ["en", "zh-CN"] as const) {
  for (const desktop of [false, true]) {
    test(`${locale} ${desktop ? "desktop" : "web"}: settings describe availability and saved log format`, async ({
      page,
    }) => {
      await openUi(page, "/flash", locale);
      await page.evaluate(async (desktop) => {
        if (desktop) {
          Object.assign(window, {
            __TAURI_INTERNALS__: {
              invoke: async (command: string) => {
                if (command === "plugin:store|load") return 1;
                if (command === "plugin:store|get") return [null, false];
                return null;
              },
            },
          });
        }
        const url = "/src/router/index.ts";
        const { router } = await import(url);
        await router.push("/settings");
      }, desktop);
      const en = locale === "en";
      const autoSave = page.getByRole("switch", {
        name: en ? "Auto-save logs" : "自动保存日志",
        exact: true,
      });
      const timestamp = page.getByRole("switch", {
        name: en ? "Record time and direction" : "记录时间与方向",
        exact: true,
      });
      const diagnostic = page.getByRole("switch", {
        name: en ? "Application diagnostic logs" : "应用诊断日志",
        exact: true,
      });
      const chooseDir = page.getByRole("button", {
        name: en ? "Choose directory" : "选择目录",
        exact: true,
      });
      const viewLogs = page.getByRole("button", { name: /View Logs|查看日志/ });
      const checkUpdate = page.getByRole("button", {
        name: /Check for updates|检查更新/,
      });
      for (const control of [
        autoSave,
        timestamp,
        diagnostic,
        chooseDir,
        viewLogs,
        checkUpdate,
        page.locator("#settings-log-level"),
        page.locator("#settings-auto-update-interval"),
      ]) {
        if (desktop) await expect(control).toBeEnabled();
        else await expect(control).toBeDisabled();
      }
      for (const id of [
        "settings-serial-log-window",
        "settings-serial-archive-limit",
        "settings-locale",
      ]) {
        await expect(page.locator(`#${id}`)).toBeEnabled();
      }
      if (!desktop) {
        await expect(
          page
            .getByText(en ? "Desktop app only" : "仅桌面版可用", {
              exact: true,
            })
            .first(),
        ).toBeVisible();
      }
      const sample = timestamp
        .locator("..")
        .locator(".settings-row__hint")
        .first();
      if (desktop) {
        await timestamp.click();
      } else {
        await page.evaluate(async () => {
          const url = "/src/stores/serial-debug.ts";
          const { useSerialDebugStore } = await import(url);
          useSerialDebugStore().autoSaveTimestamp = false;
        });
      }
      await expect(sample).toHaveText(en ? "[data]" : "[数据]");
      if (desktop) {
        await timestamp.click();
        await expect(sample).toContainText("[12:00:00.000] [RX]");
        const interval = page.locator("#settings-auto-update-interval");
        await interval.click();
        await page
          .getByRole("option", {
            name: en ? "Minimum 6 hours" : "最短间隔 6 小时",
            exact: true,
          })
          .click();
        await expect(interval).toHaveText(
          en ? "Minimum 6 hours" : "最短间隔 6 小时",
        );
      }
      await expect(
        page.locator("#settings-auto-update-interval-hint"),
      ).toContainText(en ? "At startup" : "应用启动时");
    });
  }
}
