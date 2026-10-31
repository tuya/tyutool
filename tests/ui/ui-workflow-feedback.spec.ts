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
  };
  await page.routeWebSocket(/^ws:\/\/127\.0\.0\.1:9527\/?$/, (socket) => {
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
          socket.send(
            JSON.stringify({ type: "ports", ports: ["MOCK_PORT_1"] }),
          );
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

test("restored custom baud is visible after asynchronous workspace restoration", async ({
  page,
}) => {
  await openWorkflow(page, "/flash");
  await expect(page.locator("#baud-rate-custom")).toHaveValue("123456");
  await expect(page.locator("#baud-rate")).toContainText("Custom");
});

test("sidebar activation scans once and pending explicit refresh is not duplicated", async ({
  page,
}) => {
  const state = await openWorkflow(page, "/settings");
  const before = state.scans;
  await page.locator('a[href="/serial-debug"]').click();
  await expect(page.locator(".send-bar")).toBeVisible();
  await expect.poll(() => state.scans).toBe(before + 1);
  await expect(page.locator("#sd-port-feedback")).toContainText(
    "Available ports: 1.",
  );
  state.holdScan = true;
  const refresh = page
    .locator("#sd-port-feedback")
    .getByRole("button", { name: "Refresh" });
  await refresh.click();
  await expect(refresh).toBeDisabled();
  await expect(page.locator("#sd-port-feedback")).toContainText("Loading");
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
