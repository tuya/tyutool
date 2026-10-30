// Actual production components with disposable local fixtures; no hardware actions.
import { chromium, expect } from "playwright/test";
import { writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL(".", import.meta.url));
const baseURL = process.env.TYUTOOL_UI_BASE_URL || "http://127.0.0.1:1430";
const browser = await chromium.launch();
const results = [];
try {
  for (const [width, height, lang] of [
    [1280, 800, "zh-CN"],
    [1024, 680, "en"],
  ]) {
    for (const theme of ["light", "dark"]) {
      const page = await browser.newPage({ viewport: { width, height } });
      const errors = [];
      let updateAvailable = false;
      page.on("pageerror", (error) => errors.push(error.message));
      await page.addInitScript(
        ({ lang, theme }) => {
          localStorage.setItem("tyutool-locale", lang);
          localStorage.setItem("tyutool-theme", theme);
          localStorage.setItem("tyutool-batch-auth-disclaimer-v1", "true");
        },
        { lang, theme },
      );
      await page.route("https://**/*", (route) =>
        route.fulfill(
          updateAvailable
            ? {
                status: 200,
                json: {
                  version: "99.0.0",
                  notes:
                    "## Preview fixture\n\n- This is local UI evidence.\n- No software download or installation is performed.",
                  pub_date: "2026-10-30",
                  platforms: {},
                  cli: {},
                },
              }
            : { status: 503, body: "Preview fixture: offline" },
        ),
      );
      await page.routeWebSocket(/ws:\/\/127\.0\.0\.1:9527/, (socket) =>
        socket.onMessage((raw) => {
          if (typeof raw !== "string") return;
          const request = JSON.parse(raw);
          if (request.type === "list_ports")
            socket.send(
              JSON.stringify({
                type: "ports",
                ports: ["COM6", "COM8", "COM11"],
              }),
            );
        }),
      );

      async function open(path) {
        await page.goto(baseURL + path);
        await expect(page.locator("#app-main")).toBeVisible();
        await expect(
          page.locator(".ty-route-enter-active,.ty-route-leave-active"),
        ).toHaveCount(0);
        await page.evaluate(() => document.fonts.ready);
      }
      async function capture(state) {
        await page.waitForTimeout(250);
        const metrics = await page.evaluate(() => {
          const main = document.querySelector("#app-main");
          return {
            documentOverflow: document.documentElement.scrollWidth - innerWidth,
            mainOverflow: main.scrollWidth - main.clientWidth,
            dialogs: [
              ...document.querySelectorAll('[role="dialog"],dialog[open]'),
            ].map((element) => {
              const rect = element.getBoundingClientRect();
              return {
                insideWindow:
                  rect.left >= 0 &&
                  rect.top >= 0 &&
                  rect.right <= innerWidth &&
                  rect.bottom <= innerHeight,
              };
            }),
          };
        });
        expect(metrics.documentOverflow).toBeLessThanOrEqual(1);
        expect(metrics.mainOverflow).toBeLessThanOrEqual(1);
        expect(metrics.dialogs.every((dialog) => dialog.insideWindow)).toBe(
          true,
        );
        expect(errors).toEqual([]);
        const filename = `state-${state}-${theme}-${width}-${lang}.png`;
        await page.screenshot({ path: root + filename });
        results.push({
          state,
          theme,
          width,
          height,
          lang,
          filename,
          ...metrics,
        });
      }

      await open("/flash");
      await page.evaluate(async () => {
        const { useFlashStore } = await import("/src/stores/flash.ts");
        Object.assign(useFlashStore(), {
          selectedSerialPort: "COM6",
          selectedChipId: "t5ai",
          connected: true,
          flashPhase: "running",
          runningOp: "flash",
          currentBackendPhase: "write",
          phaseProgress: 67,
          logLines: [
            "Preview fixture · T5AI · COM6",
            "Writing 1.24 MiB to 0x00000000…",
            "Transfer: 67%",
          ],
        });
      });
      await expect(page.getByRole("progressbar")).toHaveAttribute(
        "aria-valuenow",
        "67",
      );
      await capture("flash-running");
      await page.evaluate(async () => {
        const { useFlashStore } = await import("/src/stores/flash.ts");
        Object.assign(useFlashStore(), {
          flashPhase: "success",
          connected: false,
          flashMessage:
            "Preview fixture: firmware verified. Serial port released.",
          logLines: [
            "Preview fixture · Completed",
            "Firmware verified.",
            "Serial port released.",
          ],
        });
      });
      await expect(page.locator(".aside-alert-success")).toBeVisible();
      await capture("flash-success");
      await page.evaluate(async () => {
        const { useFlashStore } = await import("/src/stores/flash.ts");
        Object.assign(useFlashStore(), {
          flashPhase: "error",
          flashMessage:
            "Preview fixture: device handshake timed out. Check the cable and boot mode, then retry.",
          logLines: [
            "Preview fixture · Failed",
            "Device handshake timed out.",
            "Serial port released.",
          ],
        });
      });
      await expect(page.locator(".aside-alert-error")).toBeVisible();
      await capture("flash-error");
      await page.evaluate(async () => {
        const { showConfirmDialog } =
          await import("/src/composables/confirmDialog.ts");
        void showConfirmDialog({
          title: "Preview fixture: erase firmware",
          message:
            "Erasing this range removes the current firmware. Verify the selected serial port and address before continuing.",
          kind: "danger",
          okLabel: "Erase",
          cancelLabel: "Cancel",
        });
      });
      await expect(page.locator(".ty-dialog-container")).toBeVisible();
      await capture("danger-confirm");
      await page.keyboard.press("Escape");

      await open("/serial-debug");
      await page.evaluate(async () => {
        const { useSerialDebugStore } =
          await import("/src/stores/serial-debug.ts");
        const { recordSendHistory } =
          await import("/src/features/serial-debug/send-history.ts");
        const store = useSerialDebugStore();
        store.port = "COM6";
        store.sendHistory = recordSendHistory(
          [],
          "AT+GMR",
          "ascii",
          true,
          Date.now() - 3_600_000,
        );
        store.sendHistory = recordSendHistory(
          store.sendHistory,
          "55 AA 00 01",
          "hex",
          false,
          Date.now() - 120_000,
        );
        store.sendInput = "UNSENT_DRAFT";
        store.appendChunk({
          direction: "rx",
          tsMs: Date.now(),
          bytes: [
            ...new TextEncoder().encode(
              "[boot] console ready\n[system] free_heap=185664\nOK\n",
            ),
          ],
        });
      });
      await page.locator(".history-wrap > button").click();
      await expect(page.locator(".history-dropdown")).toBeVisible();
      await capture("command-history");
      await page.keyboard.press("Escape");
      await page.locator(".conn-btn-action").first().click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await capture("serial-settings");
      await page.keyboard.press("Escape");

      await open("/toolbox/batch-flash-auth");
      await page.evaluate(async () => {
        const { useBatchFlashAuthStore } =
          await import("/src/stores/batch-flash-auth.ts");
        const store = useBatchFlashAuthStore();
        store.addPorts(["COM6", "COM8", "COM11"]);
        Object.assign(store.slots[0], { status: "done" });
        Object.assign(store.slots[1], {
          status: "failed",
          writeUncertain: true,
          error: "Preview fixture: authorization outcome is uncertain",
        });
      });
      await page.locator('[aria-controls="batch-config-panel"]').click();
      await expect(page.locator("#batch-config-panel")).toBeHidden();
      await page.locator('[data-port="COM8"]').scrollIntoViewIfNeeded();
      await capture("batch-quarantine");

      await open("/settings");
      await page.evaluate(() =>
        Object.assign(window, {
          __TAURI_INTERNALS__: {
            invoke: async (command) => {
              if (command === "list_log_files")
                return [
                  {
                    name: "tyutool-preview-current.log",
                    sizeBytes: 512,
                    modifiedMs: 1793318400000,
                  },
                  {
                    name: "tyutool-preview-previous.log",
                    sizeBytes: 1024,
                    modifiedMs: 1793232000000,
                  },
                ];
              if (command === "read_log_tail")
                return "INFO connected to COM6\nWARN retry on COM6\nERROR timed out\nDEBUG frame received\nTRACE protocol detail";
              if (command === "list_log_file_openers")
                return [
                  { id: "default", label: "System default" },
                  { id: "vscode", label: "Visual Studio Code" },
                ];
              return null;
            },
          },
        }),
      );
      const viewLogs = lang === "en" ? "View Logs" : "查看日志";
      await page.getByRole("button", { name: viewLogs, exact: false }).click();
      await expect(page.locator(".log-file-button")).toHaveCount(2);
      await capture("diagnostic-list");
      await page.locator(".log-file-button").first().click();
      await expect(page.locator("[data-line]")).toHaveCount(5);
      await page.getByRole("dialog").getByRole("textbox").fill("COM6");
      await capture("diagnostic-search");
      const openWith = lang === "en" ? "Open With..." : "打开方式...";
      await page
        .getByRole("dialog")
        .getByRole("button", { name: openWith, exact: true })
        .click();
      await expect(page.getByRole("dialog")).toHaveCount(2);
      await capture("diagnostic-open-with");
      await page.keyboard.press("Escape");
      await page.keyboard.press("Escape");
      await page.evaluate(() => {
        delete window.__TAURI_INTERNALS__;
      });
      await open("/settings?update=1");
      await expect(page.locator(".ud-overview-card--failed")).toBeVisible();
      await capture("update-failure");
      updateAvailable = true;
      await open("/settings?update=1");
      await expect(page.locator(".ud-overview-card--available")).toBeVisible();
      await capture("update-available");
      await page.close();
    }
  }
  await writeFile(root + "states.json", JSON.stringify(results, null, 2));
  console.log(JSON.stringify({ captures: results.length, status: "passed" }));
} finally {
  await browser.close();
}
