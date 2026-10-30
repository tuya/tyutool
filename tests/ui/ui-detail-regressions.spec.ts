import { test, expect, type Locator, type Page } from "playwright/test";
import { openUi } from "./helpers/serial-log";

async function open(
  page: Page,
  path: string,
  locale = "en",
  theme = "light",
  skipNotice = true,
) {
  await page.addInitScript(
    ({ theme, skipNotice }) => {
      localStorage.setItem("tyutool-theme", theme);
      if (skipNotice)
        localStorage.setItem("tyutool-batch-auth-disclaimer-v1", "true");
    },
    { theme, skipNotice },
  );
  await page.route("https://**/*", (route) =>
    route.fulfill({ status: 503, body: "UI test: offline" }),
  );
  await openUi(page, path, locale, ["MOCK_PORT_1"]);
  // The shared helper sets the light baseline; switch the reactive preference too.
  await page.evaluate(async (theme) => {
    const url = "/src/stores/settings.ts";
    const { useSettingsStore } = await import(url);
    useSettingsStore().setTheme(theme);
  }, theme);
  await expect(
    page.locator(".ty-route-enter-active,.ty-route-leave-active"),
  ).toHaveCount(0);
}
async function seed(
  page: Page,
  feature: "serial-debug" | "batch-flash-auth" | "flash",
  code: string,
) {
  await page.evaluate(
    async ({ feature, code }) => {
      const url = `/src/stores/${feature}.ts`;
      const module = await import(url);
      const store =
        module[
          feature === "serial-debug"
            ? "useSerialDebugStore"
            : feature === "flash"
              ? "useFlashStore"
              : "useBatchFlashAuthStore"
        ]();
      Function("s", code)(store);
    },
    { feature, code },
  );
}
async function inViewport(panel: Locator) {
  await expect(panel).toBeVisible();
  // Resize positioning and the opening transition settle on the next frame.
  await expect
    .poll(() =>
      panel.evaluate((el) => {
        const r = el.getBoundingClientRect();
        return (
          r.left >= 7 &&
          r.top >= 7 &&
          r.right <= innerWidth - 7 &&
          r.bottom <= innerHeight - 7
        );
      }),
    )
    .toBe(true);
  const bounds = await panel.evaluate((el) => {
    const rect = el.getBoundingClientRect();
    return {
      left: rect.left,
      right: rect.right,
      top: rect.top,
      bottom: rect.bottom,
      width: innerWidth,
      height: innerHeight,
    };
  });
  expect(bounds.left).toBeGreaterThanOrEqual(7);
  expect(bounds.top).toBeGreaterThanOrEqual(7);
  expect(bounds.right).toBeLessThanOrEqual(bounds.width - 7);
  expect(bounds.bottom).toBeLessThanOrEqual(bounds.height - 7);
}

for (const locale of ["en", "zh-CN"])
  for (const theme of ["light", "dark"]) {
    test(`${locale} ${theme}: bottom select flips and resizes inside the viewport`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1024, height: 680 });
      await open(page, "/settings", locale, theme);
      const trigger = page.locator("#settings-serial-archive-limit");
      await trigger.scrollIntoViewIfNeeded();
      await page.locator(".main-scroll").evaluate((el) => {
        const bottom = document
          .querySelector("#settings-serial-archive-limit")!
          .getBoundingClientRect().bottom;
        el.scrollTop += bottom - 640;
      });
      await trigger.click();
      const panel = page.getByRole("listbox");
      await inViewport(panel);
      const triggerBounds = await trigger.boundingBox();
      const panelBounds = await panel.boundingBox();
      expect(panelBounds!.y + panelBounds!.height).toBeLessThan(
        triggerBounds!.y,
      );
      await page.setViewportSize({ width: 900, height: 560 });
      await inViewport(panel);
      await trigger.press("End");
      await trigger.press("Enter");
      await expect(panel).toHaveCount(0);
      await expect(trigger).toBeFocused();
    });
    test(`${locale} ${theme}: long blocked-port list scrolls inside its dialog`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1024, height: 680 });
      await open(page, "/toolbox/batch-flash-auth", locale, theme);
      await seed(
        page,
        "batch-flash-auth",
        "s.filterConfig.blockedPorts=Array.from({length:100},(_,i)=>'LONG_PORT_NAME_'+i+'_suffix');",
      );
      await page
        .getByRole("button", {
          name: locale === "en" ? /^Port Filter/ : /^串口过滤/,
        })
        .click();
      const dialog = page.getByRole("dialog");
      await inViewport(dialog);
      const list = dialog.locator(".overflow-y-auto");
      expect(
        await list.evaluate((el) => el.scrollHeight > el.clientHeight),
      ).toBe(true);
      const last = dialog.getByRole("button", {
        name: /LONG_PORT_NAME_99_suffix/,
      });
      await last.scrollIntoViewIfNeeded();
      await last.click();
      await expect(last).toHaveCount(0);
      await dialog.getByRole("textbox").press("Escape");
      await expect(dialog).toHaveCount(0);
    });
  }

test("filter popup clamps right edge and Escape restores the trigger", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 680 });
  await open(page, "/serial-debug");
  await seed(
    page,
    "serial-debug",
    "s.watchChips=Array.from({length:12},(_,i)=>({id:'f'+i,keyword:'FILTER_'+i,useRegex:false,color:'#3b82f6'}));",
  );
  const trigger = page.locator(".tab-add");
  await trigger.scrollIntoViewIfNeeded();
  await trigger.click();
  const popup = page.locator(".add-popover");
  await inViewport(popup);
  await popup.locator(".pop-toggle").focus();
  await page.keyboard.press("Escape");
  await expect(popup).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

test("batch rows keep quarantine guidance and columns aligned with long ports", async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 1024, height: 680 });
  await open(page, "/toolbox/batch-flash-auth", "en", "dark");
  await seed(
    page,
    "batch-flash-auth",
    "s.slots=[];s.addPorts(['/dev/cu.usbserial-VERY-LONG-IDENTIFIER','COM31','COM32']);s.slots[1].status='failed';s.slots[1].error='Synthetic timeout';s.slots[1].writeUncertain=true;s.slots[2].status='failed';s.slots[2].error='Synthetic timeout';",
  );
  const row = page.locator('[data-port="COM31"]');
  await row.scrollIntoViewIfNeeded();
  const dimensions = await row.evaluate((el) => ({
    client: el.clientHeight,
    scroll: el.scrollHeight,
    bottom: el.getBoundingClientRect().bottom,
    hintBottom: el.querySelector("p")!.getBoundingClientRect().bottom,
  }));
  expect(dimensions.client).toBeGreaterThan(40);
  expect(dimensions.scroll).toBeLessThanOrEqual(dimensions.client);
  expect(dimensions.hintBottom).toBeLessThan(dimensions.bottom);
  const nextTop = await page
    .locator('[data-port="COM32"]')
    .evaluate((el) => el.getBoundingClientRect().top);
  expect(nextTop).toBeGreaterThanOrEqual(dimensions.bottom);
  const port = page.locator('[data-port^="/dev/cu"] > span').first();
  await expect(port).toHaveAttribute(
    "title",
    "/dev/cu.usbserial-VERY-LONG-IDENTIFIER",
  );
  const columns = await page.evaluate(() => {
    const header = document.querySelector(".min-w-\\[38rem\\] > div")!;
    const row = document.querySelector('[data-port^="/dev/cu"]')!;
    return [
      header.children[1].getBoundingClientRect().x,
      row.querySelectorAll("span")[1].getBoundingClientRect().x,
    ];
  });
  expect(Math.abs(columns[0] - columns[1])).toBeLessThanOrEqual(4);
  await page.screenshot({ path: info.outputPath("batch-warning-fixed.png") });
});

for (const point of [
  { x: 9, y: 9 },
  { x: 1014, y: 9 },
  { x: 9, y: 670 },
  { x: 1014, y: 670 },
]) {
  test(`batch context menu stays in bounds at ${point.x},${point.y}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 680 });
    await open(page, "/toolbox/batch-flash-auth");
    await seed(page, "batch-flash-auth", "s.addPorts(['COM31']);");
    const row = page.locator('[data-port="COM31"]');
    await row.evaluate(
      (el, point) =>
        el.dispatchEvent(
          new MouseEvent("contextmenu", {
            bubbles: true,
            cancelable: true,
            clientX: point.x,
            clientY: point.y,
          }),
        ),
      point,
    );
    const menu = page.getByRole("menu");
    await inViewport(menu);
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(row).toBeFocused();
  });
}

test("Hex is read-only, traps focus and exposes active formats", async ({
  page,
}, info) => {
  await open(page, "/serial-debug", "en", "dark");
  const opener = page.getByRole("button", { name: "Find", exact: true });
  await opener.focus();
  await seed(
    page,
    "serial-debug",
    "s.showHexPopup(new Uint8Array([65,66,67]),'hex');",
  );
  const dialog = page.getByRole("dialog");
  await expect(dialog.locator("textarea")).toHaveAttribute("readonly", "");
  await dialog.getByRole("button", { name: "ASCII", exact: true }).click();
  await expect(dialog.locator("textarea")).toHaveValue("ABC");
  await expect(
    dialog.getByRole("button", { name: "ASCII", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await page.screenshot({ path: info.outputPath("hex-readonly-fixed.png") });
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(opener).toBeFocused();
});

test("flash tabs, removal focus and field-specific range errors work in both layouts", async ({
  page,
}) => {
  await open(page, "/flash");
  const tab = page.getByRole("tab", { name: "Flash", exact: true });
  await tab.focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { selected: true })).toHaveText("Erase");
  await page.keyboard.press("Home");
  await seed(page, "flash", "s.flashSegments[0].startAddr='BAD!';");
  for (const prefix of ["flash-", "flash-m-"]) {
    const input = page.locator(`input[id^=${prefix}][id$=-start]`).first();
    await expect(input).toHaveAttribute("aria-invalid", "true");
    await expect(
      page.locator(`input[id^=${prefix}][id$=-end]`).first(),
    ).toHaveAttribute("aria-invalid", "false");
  }
  await page.getByRole("button", { name: /Add segment/ }).click();
  const remove = page.getByRole("button", {
    name: /Segment 2: Remove this segment/,
  });
  await remove.click();
  await expect(page.getByRole("button", { name: /Add segment/ })).toBeFocused();
});

test("settings controls are named, themes navigate and the update query opens its dialog", async ({
  page,
}) => {
  await open(page, "/settings");
  for (const id of [
    "settings-log-level",
    "settings-serial-log-window",
    "settings-serial-archive-limit",
  ]) {
    await expect(page.locator(`#${id}`)).toHaveAttribute("aria-label", /.+/);
    await expect(page.locator(`#${id}`)).toHaveAttribute(
      "aria-describedby",
      `${id}-hint`,
    );
  }
  const light = page.getByRole("radio", { name: "Light", exact: true });
  await light.focus();
  await light.press("ArrowRight");
  await expect(
    page.getByRole("radio", { name: "Dark", exact: true }),
  ).toHaveAttribute("aria-checked", "true");
  await page.goto("/settings?update=1");
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.locator(".ud-source-card").first()).toBeVisible();
});

test("risk notice starts at its title and Escape cancels entry", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 560 });
  await open(page, "/toolbox/batch-flash-auth", "en", "light", false);
  const dialog = page.locator("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.locator("h2")).toBeFocused();
  expect(await dialog.evaluate((el) => el.scrollTop)).toBe(0);
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(page).toHaveURL(/\/toolbox$/);
});

test("toast stays while focused or hovered and reaches the update task", async ({
  page,
}) => {
  await open(page, "/flash");
  await page.evaluate(async () => {
    const url = "/src/composables/toastState.ts";
    const { toastState } = await import(url);
    Object.assign(toastState, { visible: true, version: "9.9.9" });
  });
  const toast = page.locator(".ty-toast");
  const action = toast.locator(".ty-toast-action");
  await action.focus();
  await page.waitForTimeout(6500);
  await expect(action).toBeFocused();
  await expect(toast).toBeVisible();
  await action.click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page).toHaveURL(/\/settings$/);
});

test("selected serial text actions stay in bounds and copy rejection is visible", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 680 });
  await open(page, "/serial-debug");
  await seed(
    page,
    "serial-debug",
    "s.lines=[{id:1,tsMs:1700000000000,direction:'rx',text:'DISPLAY TEXT'}];",
  );
  const line = page.locator(".pane [data-line-id]").first();
  await expect(line).toBeVisible();
  await line.evaluate((el) => {
    const selection = window.getSelection()!;
    selection.selectAllChildren(el);
    el.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        clientX: 1014,
        clientY: 670,
      }),
    );
  });
  const menu = page.getByRole("menu");
  await inViewport(menu);
  await page.evaluate(() =>
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: () => Promise.reject(new Error("UI test clipboard denial")),
      },
    }),
  );
  await menu.getByRole("menuitem").first().click();
  await expect(page.getByRole("alert")).toContainText("Copy failed");
  await expect(menu).toHaveCount(0);
  const root = page
    .locator('[tabindex="0"]')
    .filter({ has: page.locator(".pane") })
    .first();
  await expect(root).toBeFocused();
  await root.press("Shift+F10");
  await expect(menu).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(root).toBeFocused();
});

test("focused long send history reveals the entire command", async ({
  page,
}) => {
  await open(page, "/serial-debug");
  await seed(
    page,
    "serial-debug",
    "s.sendHistory=['CMD '+('argument_'.repeat(70))+' DISTINCT_END'];",
  );
  await page
    .getByRole("button", { name: "Send history", exact: true })
    .press("ArrowDown");
  const item = page.locator(".history-item");
  await expect(item).toBeFocused();
  const metrics = await item.evaluate((el) => ({
    overflow: getComputedStyle(el).whiteSpace,
    scroll: el.scrollWidth,
    width: el.clientWidth,
    title: el.getAttribute("title"),
  }));
  expect(metrics.overflow).toBe("pre-wrap");
  expect(metrics.scroll).toBeLessThanOrEqual(metrics.width);
  expect(metrics.title).toContain("DISTINCT_END");
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "Send history", exact: true }),
  ).toBeFocused();
});

for (const theme of ["light", "dark"])
  test(`${theme}: Hex active controls have readable foreground contrast`, async ({
    page,
  }) => {
    await open(page, "/serial-debug", "en", theme);
    await seed(
      page,
      "serial-debug",
      "s.showHexPopup(new Uint8Array([65,66]),'hex');",
    );
    const ratio = await page
      .getByRole("dialog")
      .locator(".mode-btn.active")
      .first()
      .evaluate((el) => {
        function luminance(color: string) {
          const rgb = color
            .match(/[\d.]+/g)!
            .slice(0, 3)
            .map(Number)
            .map((value) => value / 255)
            .map((value) =>
              value <= 0.04045
                ? value / 12.92
                : ((value + 0.055) / 1.055) ** 2.4,
            );
          return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
        }
        const style = getComputedStyle(el);
        const a = luminance(style.color),
          b = luminance(style.backgroundColor);
        return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
      });
    expect(ratio).toBeGreaterThanOrEqual(4.5);
  });
