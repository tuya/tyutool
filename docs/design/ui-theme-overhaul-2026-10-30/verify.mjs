import { chromium } from "playwright";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { writeFile } from "node:fs/promises";
import assert from "node:assert/strict";
const root = dirname(fileURLToPath(import.meta.url));
const browser = await chromium.launch({ headless: true });
const errors = [];
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
page.on("pageerror", (e) => errors.push(e.message));
page.on("response", (res) => {
  if (res.status() >= 400 && !res.url().includes("favicon"))
    errors.push(res.status() + " " + res.url());
});
await page.routeWebSocket(/ws:\/\/127\.0\.0\.1:9527/, (socket) =>
  socket.onMessage((raw) => {
    if (typeof raw !== "string") return;
    const request = JSON.parse(raw);
    if (request.type === "list_ports")
      socket.send(JSON.stringify({ type: "ports", ports: ["COM6", "COM8"] }));
    if (request.type === "serial_debug_session_read_page")
      socket.send(
        JSON.stringify({
          type: "serial_debug_session_page",
          request_id: request.request_id,
          page: { start: 0, totalLines: 0, items: [] },
        }),
      );
  }),
);
await page.addInitScript(() => {
  localStorage.setItem("tyutool-theme", "dark");
  localStorage.setItem("tyutool-locale", "zh-CN");
});
for (const route of ["flash", "serial-debug"]) {
  await page.goto("http://127.0.0.1:1430/" + route);
  await page.locator("#app-main").waitFor();
  await page.evaluate(async (route) => {
    if (route === "flash") {
      const { useFlashStore } = await import("/src/stores/" + "flash.ts");
      const s = useFlashStore();
      s.selectedChipId = "t5ai";
      s.selectedSerialPort = "COM6";
      s.flashSegments = [
        {
          id: "sample",
          firmwarePath: "tuyaopen_t5ai_demo.bin",
          firmwareFile: null,
          startAddr: "0x00000000",
          endAddr: "0x0013D000",
        },
      ];
      s.logLines = [
        "14:32:07 Serial port COM6 opened at 115200 baud.",
        "14:32:07 Connected to Tuya T5AI.",
        "14:32:08 Flash capacity: 8 MiB · Firmware: 1.24 MiB",
        "14:32:08 Erasing target sectors…",
        "14:32:09 Erase completed.",
        "14:32:09 Writing firmware to 0x00000000…",
      ];
    } else {
      const { useSerialDebugStore } = await import(
        "/src/stores/" + "serial-debug.ts"
      );
      const s = useSerialDebugStore();
      s.port = "COM6";
      s.sendInput = "AT+GMR";
      s.appendChunk({
        direction: "rx",
        tsMs: Date.now(),
        bytes: [
          ...new TextEncoder().encode(
            "[boot] TuyaOpen firmware started\n[system] chip=T5AI reset=power_on\n[system] free_heap=185664 version=1.0.0\n[uart] console ready\nOK\n[app] heartbeat=1 free_heap=183520\n",
          ),
        ],
      });
    }
  }, route);
  await page.waitForTimeout(250);
  await page.screenshot({ path: root + "/current-" + route + ".png" });
}
const preview = await browser.newPage({
  viewport: { width: 1280, height: 800 },
});
preview.on("pageerror", (e) => errors.push(e.message));
preview.on("response", (res) => {
  if (res.status() >= 400) errors.push(res.status() + " " + res.url());
});
const observations = [];
for (const [width, height] of [
  [1280, 800],
  [1024, 680],
])
  for (const locale of ["zh", "en"])
    for (const theme of ["dark", "light"])
      for (const route of ["flash", "serial", "batch", "toolbox", "settings"]) {
        await preview.setViewportSize({ width, height });
        await preview.goto(
          "http://127.0.0.1:1430/docs/design/ui-theme-overhaul-2026-10-30/index.html?case=" +
            [width, height, locale, theme, route].join("-") +
            "#" +
            route,
        );
        if (locale === "en")
          await preview.locator(".proposal-bar [data-action=locale]").click();
        await preview
          .locator(".proposal-bar [data-theme-choice=" + theme + "]")
          .click();
        await preview.evaluate(async () => {
          await document.fonts.ready;
          await Promise.allSettled(
            document.getAnimations().map((animation) => animation.finished),
          );
        });
        assert.equal(
          await preview.locator("#breadcrumb").textContent(),
          {
            zh: {
              flash: "工作空间 / 固件工具",
              serial: "工作空间 / 串口调试",
              batch: "工具箱 / 批量烧录授权",
              toolbox: "工作空间 / 工具箱",
              settings: "工作空间 / 设置",
            },
            en: {
              flash: "Workspace / Firmware tools",
              serial: "Workspace / Serial debug",
              batch: "Toolbox / Batch flash & auth",
              toolbox: "Workspace / Toolbox",
              settings: "Workspace / Settings",
            },
          }[locale][route],
        );
        const metrics = await preview.evaluate(() => {
          const main = document.querySelector("#content");
          const rect = main.getBoundingClientRect();
          const fields = [...main.querySelectorAll("button,input,select")];
          return {
            documentOverflow: document.documentElement.scrollWidth - innerWidth,
            mainOverflow: main.scrollWidth - main.clientWidth,
            mainRect: {
              left: rect.left,
              right: rect.right,
              bottom: rect.bottom,
            },
            controlsOutside: fields.filter((el) => {
              const r = el.getBoundingClientRect();
              return r.left < 0 || r.right > innerWidth;
            }).length,
            consoleHeight: main.querySelector(".log")?.clientHeight ?? null,
          };
        });
        observations.push({ width, height, locale, theme, route, ...metrics });
        if (width === 1280 && locale === "zh")
          await preview.screenshot({
            path: root + "/proposal-" + route + "-" + theme + ".png",
          });
        if (
          width === 1024 &&
          locale === "en" &&
          theme === "dark" &&
          ["flash", "serial"].includes(route)
        )
          await preview.screenshot({
            path: root + "/proposal-" + route + "-1024-en.png",
          });
      }
await preview.goto(
  "http://127.0.0.1:1430/docs/design/ui-theme-overhaul-2026-10-30/index.html?case=interactions#serial",
);
await preview.setViewportSize({ width: 1280, height: 800 });
await preview.locator(".sendbar [data-action=history]").click();
await preview.screenshot({ path: root + "/proposal-command-history-dark.png" });
await preview.locator("#history-search").fill("55 AA");
await preview.locator('[data-command="1"]').click();
const historySearchRetained =
  (await preview.locator("#history-search").inputValue()) === "55 AA";
await preview.locator("[data-action=fill-command]").click();
await preview.locator(".sendbar").waitFor();
const filled = await preview.locator("#send-input").inputValue();
const crlfRestored = !(await preview.locator("#crlf").isChecked());
const hexSelected = await preview
  .locator("[data-mode=hex]")
  .getAttribute("aria-pressed");
await preview.locator("[data-action=send]").click();
const sentVisible = await preview.locator("#serial-log").textContent();
await preview.locator(".rail [data-page=flash]").click();
await preview.locator("[data-action=run-flash]").click();
const focusedCancel = await preview
  .locator("[data-action=cancel-dialog]")
  .evaluate((el) => el === document.activeElement);
await preview.keyboard.press("Tab");
const tabToConfirm = await preview
  .locator("[data-action=confirm-flash]")
  .evaluate((el) => el === document.activeElement);
await preview.keyboard.press("Tab");
const tabWrapped = await preview
  .locator("[data-action=cancel-dialog]")
  .evaluate((el) => el === document.activeElement);
await preview.keyboard.press("Shift+Tab");
const shiftTabWrapped = await preview
  .locator("[data-action=confirm-flash]")
  .evaluate((el) => el === document.activeElement);
await preview.keyboard.press("Escape");
const escaped = await preview
  .locator("#confirm-dialog")
  .evaluate((el) => !el.open);
const focusRestored = await preview
  .locator("[data-action=run-flash]")
  .evaluate((el) => el === document.activeElement);
await preview.locator("[data-action=run-flash]").click();
await preview.locator("[data-action=confirm-flash]").click();
const completed = await preview.locator(".progress-value").textContent();
await preview.evaluate(() => (location.hash = "settings"));
await preview.locator(".settings-layout").waitFor();
const hashNavigation = await preview.locator("#breadcrumb").textContent();
await preview.emulateMedia({ reducedMotion: "reduce" });
const reduced = await preview
  .locator("button")
  .first()
  .evaluate((el) => getComputedStyle(el).transitionDuration);
function linear(c) {
  c /= 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}
function lum(hex) {
  return hex
    .match(/[a-f0-9]{2}/gi)
    .map((x) => linear(parseInt(x, 16)))
    .reduce((a, v, i) => a + v * [0.2126, 0.7152, 0.0722][i], 0);
}
function contrast(a, b) {
  const x = lum(a),
    y = lum(b);
  return +(Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}
const pairs = [
  ["dark text", "#edf2f2", "#1a2024"],
  ["dark muted", "#a0adb3", "#1a2024"],
  ["dark subtle", "#839298", "#13191c"],
  ["dark action", "#112824", "#74ddc9"],
  ["dark warning", "#f0c780", "#1a2024"],
  ["light text", "#202b28", "#ffffff"],
  ["light muted", "#56665f", "#ffffff"],
  ["light subtle", "#64716a", "#f8faf7"],
  ["light action", "#ffffff", "#006b5e"],
  ["light warning", "#895709", "#ffffff"],
  ["dark selection", "#74ddc9", "#223d39"],
  ["light selection", "#006b5e", "#e4f2ed"],
  ["dark danger", "#301413", "#ff9b98"],
  ["light danger", "#ffffff", "#b03436"],
  ["dark field border", "#607077", "#151b1f", 3],
  ["light field border", "#7a8780", "#f8f9f6", 3],
].map(([name, fg, bg, threshold = 4.5]) => ({
  name,
  fg,
  bg,
  threshold,
  ratio: +contrast(fg, bg).toFixed(2),
}));
const report = {
  kind: "Design prototype validation; not application regression or hardware acceptance",
  observations,
  errors,
  interactions: {
    filled,
    hexSelected,
    crlfRestored,
    historySearchRetained,
    tabToConfirm,
    tabWrapped,
    shiftTabWrapped,
    sendRendered: sentVisible.includes("55 AA"),
    focusedCancel,
    escaped,
    focusRestored,
    completed,
    hashNavigation,
    reduced,
  },
  contrast: pairs,
};
await writeFile(
  root + "/preview-verification.json",
  JSON.stringify(report, null, 2),
);
console.log(
  JSON.stringify(
    {
      views: observations.length,
      errors,
      overflow: observations.filter(
        (x) =>
          x.documentOverflow > 1 || x.mainOverflow > 1 || x.controlsOutside,
      ),
      interactions: report.interactions,
      contrast: pairs,
    },
    null,
    2,
  ),
);
await browser.close();
assert.equal(observations.length, 40);
assert.deepEqual(errors, []);
assert.ok(
  observations.every(
    (x) =>
      x.documentOverflow <= 1 && x.mainOverflow <= 1 && x.controlsOutside === 0,
  ),
);
assert.equal(filled, "55 AA 00 01 00 00 00");
assert.equal(hexSelected, "true");
assert.ok(
  report.interactions.sendRendered &&
    crlfRestored &&
    historySearchRetained &&
    tabToConfirm &&
    tabWrapped &&
    shiftTabWrapped &&
    focusedCancel &&
    escaped &&
    focusRestored,
);
assert.ok(completed.includes("100"));
assert.ok(pairs.every((x) => x.ratio >= x.threshold));
