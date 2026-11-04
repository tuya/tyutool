import { expect, test, type Page } from "playwright/test";

async function openWorkflow(
  page: Page,
  path: string,
  locale = "en",
  theme = "light",
) {
  await page.addInitScript(
    ({ locale, theme }) => {
      localStorage.setItem("tyutool-locale", locale);
      localStorage.setItem("tyutool-theme", theme);
      localStorage.setItem(
        "tyutool-flash-workspace-json",
        JSON.stringify({
          v: 1,
          activeTab: "read",
          selectedChipId: "t5ai",
          selectedSerialPort: "MOCK_PORT_1",
          selectedBaudRate: 123456,
          readFileName: "original.bin",
          readFileNameModified: true,
          flashSegments: [
            { id: "a", firmwarePath: "", startAddr: "0x0", endAddr: "0x1000" },
          ],
        }),
      );
    },
    { locale, theme },
  );
  const state = {
    scans: 0,
    jobs: 0,
    sent: [] as number[][],
    outcome: "success" as "success" | "error" | "cancelled",
    errorMessage: "Port disconnected",
    holdScan: false,
    finishScan: () => {},
    ports: ["MOCK_PORT_1"],
    deviceChanged: () => {},
  };
  await page.routeWebSocket(/^ws:\/\/127\.0\.0\.1:9527\/?$/, (socket) => {
    state.deviceChanged = () =>
      socket.send(JSON.stringify({ type: "serial_ports_changed" }));
    socket.onMessage((message) => {
      if (typeof message !== "string") return;
      const request = JSON.parse(message) as {
        type: string;
        bytes?: number[];
        job?: { mode?: string };
      };
      if (request.type === "list_ports") {
        state.scans++;
        const finish = () =>
          socket.send(JSON.stringify({ type: "ports", ports: state.ports }));
        if (state.holdScan) state.finishScan = finish;
        else finish();
      } else if (request.type === "serial_debug_open") {
        socket.send(JSON.stringify({ type: "serial_debug_opened" }));
      } else if (request.type === "serial_debug_close") {
        socket.send(JSON.stringify({ type: "serial_debug_closed" }));
      } else if (request.type === "serial_debug_send") {
        state.sent.push(request.bytes ?? []);
      } else if (request.type === "run_job") {
        state.jobs++;
        if (state.outcome === "success" && request.job?.mode === "read")
          socket.send(
            JSON.stringify({
              type: "progress",
              payload: {
                kind: "file_content",
                name: "server-temp.bin",
                content: "AAEC",
              },
            }),
          );
        const result =
          state.outcome === "success"
            ? { ok: { elapsed_secs: 3.25 } }
            : state.outcome === "error"
              ? { err: { message: state.errorMessage, elapsed_secs: 2 } }
              : { cancelled: { elapsed_secs: 1 } };
        socket.send(
          JSON.stringify({
            type: "progress",
            payload: { kind: "done", result },
          }),
        );
      }
    });
  });
  await page.goto(path);
  await expect.poll(() => state.scans).toBeGreaterThan(0);
  await expect(page.locator(".main-scroll")).toBeVisible();
  return state;
}

async function noHorizontalOverflow(page: Page) {
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  for (const selector of [".send-bar", ".flash-result", ".conn-bar"]) {
    const element = page.locator(selector);
    if (await element.count())
      expect(
        await element.evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
      ).toBe(true);
  }
}

for (const width of [1024, 1549]) {
  for (const locale of ["en", "zh-CN"]) {
    for (const path of [
      "/flash",
      "/serial-debug",
      "/toolbox/batch-flash-auth",
    ]) {
      test(`${locale} ${width} ${path}: device events keep the workspace layout stable`, async ({
        page,
      }) => {
        await page.setViewportSize({
          width,
          height: width === 1024 ? 680 : 884,
        });
        await page.addInitScript(() =>
          localStorage.setItem("tyutool-batch-auth-disclaimer-v1", "true"),
        );
        const state = await openWorkflow(page, path, locale);
        const selectors =
          path === "/flash"
            ? [
                ".conn-bar",
                ".flash-workspace",
                "#serial-port",
                "#baud-rate",
                "#chip-select",
              ]
            : path === "/serial-debug"
              ? [".conn-bar", ".send-bar", "#sd-port", "#sd-baud"]
              : ['.main-scroll [class*="min-w-[38rem]"] > div:first-child'];
        const snapshot = () =>
          page.evaluate(
            (selectors) =>
              selectors.map((selector) => {
                const rect = document
                  .querySelector(selector)!
                  .getBoundingClientRect();
                return {
                  selector,
                  x: rect.x,
                  y: rect.y,
                  width: rect.width,
                  height: rect.height,
                };
              }),
            selectors,
          );
        await expect.poll(() => state.scans).toBeGreaterThan(0);
        // Wait for initialization to finish before measuring the settled layout.
        if (path === "/flash")
          await expect(page.locator("#flash-port-feedback")).not.toBeVisible();
        else if (path === "/serial-debug")
          await expect(page.locator(".port-refresh")).toHaveAttribute(
            "aria-busy",
            "false",
          );
        else
          await expect(
            page
              .locator(".main-scroll")
              .getByText("MOCK_PORT_1", { exact: true }),
          ).toBeVisible();
        const before = await snapshot();
        for (const ports of [["MOCK_PORT_1", "MOCK_PORT_2"], []]) {
          state.ports = ports;
          state.holdScan = true;
          const scans = state.scans;
          state.deviceChanged();
          await expect.poll(() => state.scans).toBe(scans + 1);
          expect(await snapshot()).toEqual(before);
          state.holdScan = false;
          state.finishScan();
          await expect.poll(snapshot).toEqual(before);
          // Ensure the device update has settled before measuring or starting another scan.
          if (path === "/flash")
            await expect(page.locator("#serial-port")).toContainText(
              ports.length
                ? "MOCK_PORT_1"
                : locale === "en"
                  ? "No ports"
                  : "无可用串口",
            );
          else if (path === "/serial-debug") {
            await expect(page.locator(".port-refresh")).toHaveAttribute(
              "aria-busy",
              "false",
            );
            await expect(page.locator("#sd-port")).toContainText("MOCK_PORT_1");
            if (ports.length)
              await expect(page.locator("#sd-port-feedback")).toHaveCount(0);
            else
              await expect(page.locator("#sd-port-feedback")).toContainText(
                "MOCK_PORT_1",
              );
          } else
            await expect(
              page
                .locator(".main-scroll")
                .getByText("MOCK_PORT_2", { exact: true }),
            ).toHaveCount(ports.length ? 1 : 0);
          expect(await snapshot()).toEqual(before);
        }
      });
    }
  }
}

test("restored custom baud is visible after asynchronous workspace restoration", async ({
  page,
}) => {
  await openWorkflow(page, "/flash");
  await expect(page.locator("#baud-rate-custom")).toHaveValue("123456");
  await expect(page.locator("#baud-rate")).toContainText("Custom");
});

test("flash port dropdown opens without scanning or a separate refresh button", async ({
  page,
}) => {
  const state = await openWorkflow(page, "/flash");
  const port = page.locator("#serial-port");
  await expect(page.locator("#flash-port-feedback")).not.toBeVisible();
  await expect(
    page
      .locator(".conn-bar")
      .getByRole("button", { name: "Refresh", exact: true }),
  ).toHaveCount(0);
  const before = state.scans;
  await port.click();
  await expect(page.getByRole("listbox")).toBeVisible();
  expect(state.scans).toBe(before);
  await port.press("Escape");
  await page.waitForTimeout(1100);
  await expect(page.locator("#flash-port-feedback")).not.toBeVisible();
  await port.click();
  await expect(page.getByRole("listbox")).toBeVisible();
  expect(state.scans).toBe(before);
});

test("device notifications update flash ports immediately and suppress a nearby dropdown scan", async ({
  page,
}) => {
  const state = await openWorkflow(page, "/flash");
  await expect(page.locator("#flash-port-feedback")).not.toBeVisible();
  const before = state.scans;
  state.ports.push("MOCK_PORT_2");
  state.deviceChanged();
  await expect.poll(() => state.scans).toBe(before + 1);
  const port = page.locator("#serial-port");
  await port.click();
  await expect(
    page.getByRole("option", { name: "MOCK_PORT_2", exact: true }),
  ).toBeVisible();
  expect(state.scans).toBe(before + 1);
  await port.press("ArrowDown");
  state.ports = ["MOCK_PORT_1"];
  state.deviceChanged();
  await expect.poll(() => state.scans).toBe(before + 2);
  await expect(
    page.getByRole("option", { name: "MOCK_PORT_2", exact: true }),
  ).toHaveCount(0);
  await expect(port).toHaveAttribute("aria-expanded", "true");
  const activeOption = await port.getAttribute("aria-activedescendant");
  await expect(page.locator(`[id="${activeOption}"]`)).toContainText(
    "MOCK_PORT_1",
  );
  expect(state.scans).toBe(before + 2);
  state.ports.push("MOCK_PORT_3");
  state.deviceChanged();
  await expect(
    page.getByRole("option", { name: "MOCK_PORT_3", exact: true }),
  ).toBeVisible();
  await expect(port).toHaveAttribute("aria-expanded", "true");
  expect(state.scans).toBe(before + 3);
});

test("flash and serial debug share a device-event scan and keep a fresh dropdown", async ({
  page,
}) => {
  const state = await openWorkflow(page, "/serial-debug");
  await expect(page.locator(".port-refresh")).toHaveAttribute(
    "aria-busy",
    "false",
  );
  await expect(page.locator("#sd-port")).toContainText("MOCK_PORT_1");
  const port = page.locator("#sd-port");
  const before = state.scans;
  await port.click();
  await expect(page.getByRole("listbox")).toBeVisible();
  await expect(page.locator(".port-refresh")).toHaveAttribute(
    "aria-busy",
    "false",
  );
  await expect(page.locator("#sd-port")).toContainText("MOCK_PORT_1");
  expect(state.scans).toBe(before);
  state.ports.push("MOCK_PORT_2");
  state.deviceChanged();
  await expect(page.locator(".port-refresh")).toHaveAttribute(
    "aria-busy",
    "false",
  );
  expect(state.scans).toBe(before + 1);
  await expect(
    page.getByRole("option", { name: "MOCK_PORT_2", exact: true }),
  ).toBeVisible();
  await expect(port).toHaveAttribute("aria-expanded", "true");
  expect(state.scans).toBe(before + 1);
  state.ports = ["MOCK_PORT_1"];
  state.deviceChanged();
  await expect(
    page.getByRole("option", { name: "MOCK_PORT_2", exact: true }),
  ).toHaveCount(0);
  await expect(port).toHaveAttribute("aria-expanded", "true");
  expect(state.scans).toBe(before + 2);
});

for (const locale of ["en", "zh-CN"]) {
  test(`serial debug replaces a missing idle port after a device event ${locale}`, async ({
    page,
  }) => {
    const state = await openWorkflow(page, "/serial-debug", locale);
    const port = page.locator("#sd-port");
    state.ports = ["COM6"];
    state.deviceChanged();
    await expect(port).toHaveText("COM6");

    await port.click();
    state.ports = [];
    state.deviceChanged();
    await expect(port).toContainText("COM6");
    await expect(page.getByRole("option", { name: /COM6/ })).toHaveAttribute(
      "aria-disabled",
      "true",
    );

    state.ports = ["COM22", "COM23"];
    state.deviceChanged();
    await expect(port).toHaveText("COM22");
    await expect(port).toHaveAttribute("aria-expanded", "true");
    await expect(page.getByRole("option", { name: /COM6/ })).toHaveCount(0);
    await page.getByRole("option", { name: "COM23", exact: true }).click();

    state.ports.push("COM24");
    state.deviceChanged();
    await expect(port).toHaveText("COM23");
    await expect(
      page.getByRole("button", {
        name: locale === "en" ? "Open" : "打开",
        exact: true,
      }),
    ).toBeVisible();
  });
}

test("reboot control dropdown opens without scanning and updates while expanded", async ({
  page,
}) => {
  const state = await openWorkflow(page, "/serial-debug");
  await expect(page.locator(".port-refresh")).toHaveAttribute(
    "aria-busy",
    "false",
  );
  await expect(page.locator("#sd-port")).toContainText("MOCK_PORT_1");
  await page.getByRole("button", { name: "Change", exact: true }).click();
  const port = page.getByRole("combobox", {
    name: "Control port",
    exact: true,
  });
  const before = state.scans;
  await port.click();
  await expect(page.getByRole("listbox")).toBeVisible();
  expect(state.scans).toBe(before);
  await expect(page.locator(".port-refresh")).toHaveAttribute(
    "aria-busy",
    "false",
  );
  await expect(page.locator("#sd-port")).toContainText("MOCK_PORT_1");
  state.ports.push("MOCK_PORT_2");
  state.deviceChanged();
  await expect(
    page.getByRole("option", { name: "MOCK_PORT_2", exact: true }),
  ).toBeVisible();
  await expect(port).toHaveAttribute("aria-expanded", "true");
  expect(state.scans).toBe(before + 1);
  state.ports = ["MOCK_PORT_1"];
  state.deviceChanged();
  await expect(
    page.getByRole("option", { name: "MOCK_PORT_2", exact: true }),
  ).toHaveCount(0);
  await expect(port).toHaveAttribute("aria-expanded", "true");
  expect(state.scans).toBe(before + 2);
  await port.press("Escape");
  await port.click();
  await expect(page.getByRole("listbox")).toBeVisible();
  expect(state.scans).toBe(before + 2);
});

test("batch authorization updates device slots and shares notification scans with other features", async ({
  page,
}) => {
  await page.addInitScript(() =>
    localStorage.setItem("tyutool-batch-auth-disclaimer-v1", "true"),
  );
  const state = await openWorkflow(page, "/toolbox/batch-flash-auth");
  const slots = page.locator(".main-scroll");
  await expect(slots.getByText("MOCK_PORT_1", { exact: true })).toBeVisible();
  const before = state.scans;
  state.ports.push("MOCK_PORT_2");
  state.deviceChanged();
  await expect(slots.getByText("MOCK_PORT_2", { exact: true })).toBeVisible();
  expect(state.scans).toBe(before + 1);
  state.ports = ["MOCK_PORT_2"];
  state.deviceChanged();
  await expect(slots.getByText("MOCK_PORT_1", { exact: true })).toHaveCount(0);
  expect(state.scans).toBe(before + 2);
  await page.locator('a[href="/settings"]').click();
  state.ports.push("MOCK_PORT_3");
  state.deviceChanged();
  await expect.poll(() => state.scans).toBe(before + 3);
  await page.locator('a[href="/toolbox"]').click();
  await page.locator('a[href="/toolbox/batch-flash-auth"]').click();
  await expect(slots.getByText("MOCK_PORT_3", { exact: true })).toBeVisible();
  expect(state.scans).toBe(before + 3);
});

test("sidebar activation scans once and pending explicit refresh is not duplicated", async ({
  page,
}) => {
  const state = await openWorkflow(page, "/settings");
  const before = state.scans;
  await page.locator('a[href="/serial-debug"]').click();
  await expect(page.locator(".send-bar")).toBeVisible();
  await expect.poll(() => state.scans).toBe(before + 1);
  await expect(page.locator(".port-refresh")).toHaveAttribute(
    "aria-busy",
    "false",
  );
  await expect(page.locator("#sd-port")).toContainText("MOCK_PORT_1");
  state.holdScan = true;
  const refresh = page.locator(".port-refresh");
  await refresh.click();
  await expect(refresh).toBeDisabled();
  await expect(refresh).toHaveAttribute("aria-busy", "true");
  await expect(page.locator("#sd-port-feedback")).toHaveCount(0);
  await expect.poll(() => state.scans).toBe(before + 2);
  await page.locator("#sd-port").click();
  await expect(page.getByRole("listbox")).toBeVisible();
  expect(state.scans).toBe(before + 2);
  await page.locator("#sd-port").press("Escape");
  state.holdScan = false;
  state.finishScan();
  await expect(refresh).toBeEnabled();
  await page.locator('a[href="/settings"]').click();
  await page.locator('a[href="/serial-debug"]').click();
  await expect.poll(() => state.scans).toBe(before + 3);
});

for (const locale of ["en", "zh-CN"])
  for (const theme of ["light", "dark"]) {
    test(`${locale} ${theme}: send preview matches bytes and history remains reachable at minimum size`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1024, height: 680 });
      const state = await openWorkflow(page, "/serial-debug", locale, theme);
      const input = page.locator(".send-bar input[type=text]");
      const send = page.locator(".send-bar .btn-primary");
      await expect(page.locator(".send-feedback")).toHaveCount(0);
      await expect(page.locator("#sd-port-feedback")).toHaveCount(0);
      await expect(input).not.toHaveAttribute("aria-describedby");
      await input.fill("中");
      const preview = page.locator(".send-preview");
      await expect(preview).toContainText(
        locale === "en" ? "5 bytes" : "5 字节",
      );
      await expect(send).toBeDisabled();
      await page
        .getByRole("button", {
          name: locale === "en" ? "Open" : "打开",
          exact: true,
        })
        .click();
      await expect(send).toBeEnabled();
      await send.click();
      await expect.poll(() => state.sent).toEqual([[0xe4, 0xb8, 0xad, 13, 10]]);
      await page.locator(".mode-btn").filter({ hasText: "Hex" }).click();
      await input.fill("41, 42; F!");
      await expect(preview).toContainText(
        locale === "en" ? "2 bytes" : "2 字节",
      );
      await expect(preview).toContainText(
        locale === "en" ? "1 trailing Hex" : "忽略末尾 1",
      );
      await send.click();
      await expect.poll(() => state.sent.at(-1)).toEqual([65, 66]);
      await page.locator(".history-wrap > button").click();
      const history = page.locator(".history-dropdown");
      await expect(history).toBeVisible();
      const bounds = await history.boundingBox();
      expect(bounds!.y).toBeGreaterThanOrEqual(7);
      expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(673);
      await history.locator(".history-search").press("Escape");
      await expect(page.locator(".history-wrap > button")).toBeFocused();
      await noHorizontalOverflow(page);
    });

    test(`${locale} ${theme}: read result preserves the snapshot and recovery actions work`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1024, height: 680 });
      const state = await openWorkflow(page, "/flash", locale, theme);
      const startRead = page.getByRole("button", {
        name: locale === "en" ? "Start read" : "开始读取",
        exact: true,
      });
      await expect(startRead).toBeEnabled();
      const download = page.waitForEvent("download");
      await startRead.click();
      expect((await download).suggestedFilename()).toBe("original.bin");
      const card = page.locator(".flash-result");
      await expect(card).toHaveAttribute("role", "status");
      await expect(card).toContainText("MOCK_PORT_1");
      await expect(card).toContainText("123456");
      await expect(card).toContainText("3.3s");
      await expect(card).toContainText("original.bin");
      const log = await page.locator(".aside-log-body").boundingBox();
      expect(log!.height).toBeGreaterThanOrEqual(80);
      expect(log!.y + log!.height).toBeLessThanOrEqual(680);
      await page.locator("#read-filename").fill("changed.bin");
      await expect(card).not.toContainText("changed.bin");
      await noHorizontalOverflow(page);
      await page
        .getByRole("tab", {
          name: locale === "en" ? "Flash" : "烧录",
          exact: true,
        })
        .click();
      await card
        .getByRole("button", {
          name: locale === "en" ? "Back to configuration" : "返回配置",
        })
        .click();
      await expect(
        page.getByRole("tab", {
          name: locale === "en" ? "Read" : "读取",
          exact: true,
        }),
      ).toBeFocused();
      expect(state.jobs).toBe(1);
      state.outcome = "error";
      await startRead.click();
      await expect(card).toHaveAttribute("role", "alert");
      await page.evaluate(() => {
        let failed = false;
        Object.defineProperty(navigator.clipboard, "writeText", {
          configurable: true,
          value: async (text: string) => {
            if (!failed) {
              failed = true;
              throw new Error("Clipboard denied");
            }
            (window as unknown as { copiedText: string }).copiedText = text;
          },
        });
      });
      const copy = card.getByRole("button", {
        name: locale === "en" ? "Copy error" : "复制错误",
      });
      await copy.click();
      await expect(card.getByRole("status")).toContainText(
        locale === "en" ? "Copy failed" : "复制失败",
      );
      await copy.click();
      await expect
        .poll(() =>
          page.evaluate(
            () => (window as unknown as { copiedText: string }).copiedText,
          ),
        )
        .toBe("Port disconnected");
      state.errorMessage = "T5AI did not answer the download-mode handshake";
      await startRead.click();
      const message = card.locator(".result-message");
      await expect(message).toContainText(
        locale === "en"
          ? "The device did not enter download mode"
          : "设备未进入下载模式",
      );
      await copy.click();
      await expect
        .poll(() =>
          page.evaluate(
            () => (window as unknown as { copiedText: string }).copiedText,
          ),
        )
        .toBe((await message.textContent())!.trim());
      state.outcome = "cancelled";
      await startRead.click();
      await expect(card).toHaveAttribute("role", "status");
      await expect(card).toHaveClass(/aside-alert-cancelled/);
      await expect(card.locator(".result-message")).toHaveText(
        locale === "en" ? "Cancelled." : "已取消。",
      );
      await expect(copy).toHaveCount(0);
      await card
        .getByRole("button", {
          name: locale === "en" ? "Clear result" : "清除结果",
        })
        .click();
      await expect(card).toHaveCount(0);
    });
  }
