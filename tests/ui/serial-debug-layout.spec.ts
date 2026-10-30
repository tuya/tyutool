import { test, expect } from "playwright/test";
import { openUi } from "./helpers/serial-log";

test.describe("UI-01 English serial controls do not overlap at minimum window", () => {
  test.setTimeout(15_000);
  for (const mockPort of [false, true]) {
    test(`port ${mockPort ? "available" : "unavailable"}`, async ({ page }) => {
      await page.setViewportSize({ width: 1024, height: 680 });
      await openUi(
        page,
        "/serial-debug",
        "en",
        mockPort ? ["MOCK_PORT_1"] : [],
      );
      if (mockPort) {
        await page.locator("#sd-port").click();
        await expect(
          page.getByRole("option", { name: "MOCK_PORT_1" }),
        ).toBeVisible();
        await page.getByRole("option", { name: "MOCK_PORT_1" }).click();
      }
      const port = page.locator("#sd-port");
      const baud = page.locator("#sd-baud");
      const actions = page.locator(".conn-bar > div").last().locator("button");
      await expect
        .poll(async () => actions.count(), { timeout: 2500 })
        .toBeGreaterThan(0);
      for (const field of [port, baud]) {
        for (const action of await actions.all()) {
          const overlap = await field.evaluate(
            (fieldEl, actionEl) => {
              const a = fieldEl.getBoundingClientRect();
              const b = (actionEl as HTMLElement).getBoundingClientRect();
              return (
                Math.max(
                  0,
                  Math.min(a.right, b.right) - Math.max(a.left, b.left),
                ) *
                Math.max(
                  0,
                  Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top),
                )
              );
            },
            await action.elementHandle(),
          );
          expect(
            overlap,
            `control overlap area for ${await action.innerText()}`,
          ).toBe(0);
        }
      }
    });
  }
});

test.describe("UI-05 safe confirmation keyboard behavior", () => {
  test("erase starts on Cancel, traps focus and restores its opener", async ({
    page,
  }) => {
    await openUi(page, "/flash", "en", ["MOCK_PORT_1"]);
    await page.getByRole("tab", { name: "Erase", exact: true }).click();
    const opener = page.getByRole("button", {
      name: "Start erase",
      exact: true,
    });
    await opener.click();
    const dialog = page.getByRole("alertdialog");
    await expect(dialog).toBeVisible();
    const cancel = dialog.getByRole("button", { name: "Cancel", exact: true });
    await expect(cancel).toBeFocused();
    const first = dialog.locator("button").first();
    const last = dialog.locator("button").last();
    await last.focus();
    await page.keyboard.press("Tab");
    await expect(first).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(last).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(opener).toBeFocused();
  });
});

test.describe("UI-06 batch workflow", () => {
  async function openBatch(page: import("playwright/test").Page) {
    await openUi(page, "/toolbox/batch-flash-auth", "en");
    const notice = page.locator("dialog");
    await expect(notice).toBeVisible();
    await notice.locator("button").first().click();
    await expect(notice).toBeHidden();
  }

  test("start and port filter are discoverable at minimum window size", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openBatch(page);
    for (const name of ["Start All", "Port Filter"]) {
      const button = page.getByRole("button", { name, exact: true });
      expect(
        await button.evaluate((el) => {
          const r = el.getBoundingClientRect();
          return r.top >= 0 && r.bottom <= innerHeight;
        }),
      ).toBe(true);
    }
  });

  test("port filter enters, traps and restores focus, and Escape closes", async ({
    page,
  }) => {
    await openBatch(page);
    const opener = page.getByRole("button", {
      name: "Port Filter",
      exact: true,
    });
    await opener.click();
    const dialog = page.getByRole("dialog", { name: "Port Filter" });
    await expect(dialog).toBeVisible();
    await expect
      .poll(() => dialog.evaluate((el) => el.contains(document.activeElement)))
      .toBe(true);
    const first = dialog.locator("button,input").first();
    const last = dialog.locator("button,input").last();
    await last.focus();
    await page.keyboard.press("Tab");
    await expect(first).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(opener).toBeFocused();
  });

  test("chip and baud controls have field names instead of only selected values", async ({
    page,
  }) => {
    await openBatch(page);
    for (const name of ["Chip", "Flash Baud Rate", "Auth Baud Rate"]) {
      await expect(
        page.getByRole("combobox", { name, exact: true }),
      ).toBeVisible();
    }
  });

  test("missing firmware blocks the configured flash mode instead of silently authorizing only", async ({
    page,
  }) => {
    await openBatch(page);
    await page.evaluate(async () => {
      const { useBatchFlashAuthStore } = await import(
        /* @vite-ignore */ "/src/stores/" + "batch-flash-auth.ts"
      );
      const store = useBatchFlashAuthStore();
      store.chipId = "esp32";
      store.flashFirmware = true;
      store.addPorts(["MOCK_PORT_1"]);
      store.authConfig.excelPath = "synthetic.xlsx";
      await Promise.resolve();
      store.excelStats = {
        total: 1,
        used: 0,
        inProgress: 0,
        remaining: 1,
        invalid: 0,
      };
    });
    const start = page.getByRole("button", { name: "Start All", exact: true });
    await expect(start).toBeDisabled();
    await expect(page.locator("#batch-current-mode")).toContainText(
      "Flash, then authorize",
    );
    await expect(page.locator("#batch-start-prerequisite")).toContainText(
      "firmware",
    );
    await page.evaluate(async () => {
      const { useBatchFlashAuthStore } = await import(
        /* @vite-ignore */ "/src/stores/" + "batch-flash-auth.ts"
      );
      useBatchFlashAuthStore().firmwarePath = "synthetic.bin";
    });
    await expect(start).toBeEnabled();
    await expect(page.locator("#batch-current-mode")).toContainText(
      "Flash, then authorize",
    );
    await expect(page.locator("#batch-start-prerequisite")).toBeHidden();
  });
});

test("UI-07 invalid Hex preserves the draft and shows an inline error", async ({
  page,
}) => {
  await openUi(page, "/serial-debug", "en");
  await page.evaluate(async () => {
    const { useSerialDebugStore } = await import(
      /* @vite-ignore */ "/src/stores/" + "serial-debug.ts"
    );
    const store = useSerialDebugStore();
    store.open = true;
    store.sendMode = "hex";
  });
  const input = page.locator(".send-bar input[type=text]");
  await input.fill("Z");
  await input.press("Enter");
  await expect(input).toHaveValue("Z");
  await expect(page.locator(".send-bar [role=alert]")).toBeVisible();
});

test("UI-08 flash prerequisites and authorization field errors stay near the controls", async ({
  page,
}) => {
  await openUi(page, "/flash", "en", ["MOCK_PORT_1"]);
  const start = page.getByRole("button", { name: "Start flash", exact: true });
  await expect(start).toBeDisabled();
  await expect(page.locator(".ops-footer")).toContainText(
    "Please select a firmware file.",
  );
  await page.getByRole("tab", { name: "Authorize", exact: true }).click();
  await page.locator("#auth-uuid").fill("short");
  await page.locator("#auth-key").fill("k".repeat(32));
  await expect(page.locator("#auth-uuid")).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  const errorId = await page
    .locator("#auth-uuid")
    .getAttribute("aria-describedby");
  expect(errorId).toBeTruthy();
  await expect(page.locator(`[id="${errorId}"]`)).toContainText(
    "UUID must be exactly 20 characters.",
  );
  await expect(
    page.getByRole("button", { name: "Start authorization", exact: true }),
  ).toBeDisabled();
  await page.locator("#auth-uuid").fill("u".repeat(20));
  await expect(
    page.getByRole("button", { name: "Start authorization", exact: true }),
  ).toBeEnabled();
});

test.describe("UI-09 primary button text contrast", () => {
  for (const theme of ["light", "dark"]) {
    test(`${theme} enabled flash button meets normal text contrast`, async ({
      page,
    }) => {
      await openUi(page, "/flash", "en", ["MOCK_PORT_1"]);
      await page.evaluate(async (theme) => {
        const { useSettingsStore } = await import(
          /* @vite-ignore */ "/src/stores/" + "settings.ts"
        );
        const { useFlashStore } = await import(
          /* @vite-ignore */ "/src/stores/" + "flash.ts"
        );
        useSettingsStore().setTheme(theme);
        useFlashStore().flashSegments[0].firmwarePath = "synthetic.bin";
      }, theme);
      const button = page.getByRole("button", {
        name: "Start flash",
        exact: true,
      });
      await expect(button).toBeEnabled();
      for (const hover of [false, true]) {
        if (hover) await button.hover();
        const ratios = await button.evaluate((el) => {
          const css = getComputedStyle(el);
          const brightness = Number(
            css.filter.match(/brightness\(([\d.]+)\)/)?.[1] ?? 1,
          );
          const rgb = (s: string) =>
            s
              .match(/[\d.]+/g)!
              .slice(0, 3)
              .map((v) => Math.min(255, Number(v) * brightness));
          const lum = (c: number[]) =>
            c
              .map((v) => v / 255)
              .map((v) =>
                v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4,
              )
              .reduce((a, v, i) => a + v * [0.2126, 0.7152, 0.0722][i], 0);
          const fg = lum(rgb(css.color));
          const stops = css.backgroundImage.match(/rgb\([^)]+\)/g) ?? [
            css.backgroundColor,
          ];
          return stops.map((stop) => {
            const bg = lum(rgb(stop));
            return (Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05);
          });
        });
        for (const ratio of ratios) expect(ratio).toBeGreaterThanOrEqual(4.5);
      }
    });
  }
});

test.describe("UI-02 firmware Browse is discoverable in the minimum-width default view", () => {
  test("Browse is not clipped and its center hit-tests to the Browse button", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await openUi(page, "/flash", "en");
    const browse = page.getByRole("button", { name: /Browse/ }).first();
    const centerHit = await browse.evaluate((button) => {
      const r = button.getBoundingClientRect();
      const hit = document.elementFromPoint(
        r.left + r.width / 2,
        r.top + r.height / 2,
      );
      return hit === button || button.contains(hit);
    });
    expect(centerHit).toBe(true);
    const [fileChooser] = await Promise.all([
      page.waitForEvent("filechooser", { timeout: 2500 }),
      browse.click(),
    ]);
    expect(fileChooser).toBeTruthy();
  });
});

test.describe("UI-03 serial settings modal focus management", () => {
  test.setTimeout(15_000);
  async function openSettings(page: import("playwright/test").Page) {
    await openUi(page, "/serial-debug", "en");
    const opener = page.getByRole("button", { name: "Serial Settings" });
    await opener.click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    return { dialog, opener };
  }
  function visibleFocusables(dialog: import("playwright/test").Locator) {
    return dialog
      .locator(
        'button:not([disabled]),input:not([disabled]),select:not([disabled]),[tabindex]:not([tabindex="-1"]):not([disabled])',
      )
      .filter({ visible: true });
  }

  test("opening moves focus into the modal", async ({ page }) => {
    const { dialog } = await openSettings(page);
    await expect
      .poll(
        () => dialog.evaluate((el) => el.contains(document.activeElement)),
        { timeout: 2500 },
      )
      .toBe(true);
  });
  test("Tab from the last control wraps to the first", async ({ page }) => {
    const { dialog } = await openSettings(page);
    const items = await visibleFocusables(dialog);
    const first = items.first();
    await items.last().focus();
    await page.keyboard.press("Tab");
    await expect
      .poll(() => first.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
  test("Shift+Tab from the first control wraps to the last", async ({
    page,
  }) => {
    const { dialog } = await openSettings(page);
    const items = await visibleFocusables(dialog);
    const last = items.last();
    await items.first().focus();
    await page.keyboard.press("Shift+Tab");
    await expect
      .poll(() => last.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
  test("Escape closes and restores focus to the opener", async ({ page }) => {
    const { dialog, opener } = await openSettings(page);
    await visibleFocusables(dialog).first().focus();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect
      .poll(() => opener.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
  test("close button closes and restores focus to the opener", async ({
    page,
  }) => {
    const { dialog, opener } = await openSettings(page);
    await dialog.locator("button").first().click();
    await expect(dialog).toBeHidden();
    await expect
      .poll(() => opener.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
  test("Escape closes the select popup before the settings modal", async ({
    page,
  }) => {
    const { dialog, opener } = await openSettings(page);
    const selectTrigger = dialog.locator(".ty-select-trigger").first();
    await selectTrigger.focus();
    await page.keyboard.press("ArrowDown");
    const listbox = page.getByRole("listbox");
    await expect(listbox).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(listbox).toBeHidden();
    await expect(dialog).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect
      .poll(() => opener.evaluate((el) => el === document.activeElement), {
        timeout: 2500,
      })
      .toBe(true);
  });
});
