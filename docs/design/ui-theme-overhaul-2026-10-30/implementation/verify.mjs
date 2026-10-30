// Production UI evidence. All ports, logs and task states below are local fixtures.
import { chromium } from "playwright";
import { fileURLToPath } from "node:url";
import { mkdir, writeFile } from "node:fs/promises";
const root = fileURLToPath(new URL(".", import.meta.url));
const baseURL = process.env.TYUTOOL_UI_BASE_URL || "http://127.0.0.1:1430";
await mkdir(root, { recursive: true });
const browser = await chromium.launch();
const results = [];
for (const [width, height] of [
  [1280, 800],
  [1024, 680],
])
  for (const lang of ["zh-CN", "en"])
    for (const theme of ["light", "dark"])
      for (const path of [
        "flash",
        "serial-debug",
        "toolbox",
        "toolbox/batch-flash-auth",
        "settings",
      ]) {
        const page = await browser.newPage({ viewport: { width, height } });
        const errors = [];
        await page.addInitScript(
          ({ lang, theme }) => {
            localStorage.setItem("tyutool-locale", lang);
            localStorage.setItem("tyutool-theme", theme);
            localStorage.setItem("tyutool-batch-auth-disclaimer-v1", "true");
          },
          { lang, theme },
        );
        await page.route("https://**/*", (r) =>
          r.fulfill({ status: 503, body: "offline preview" }),
        );
        await page.routeWebSocket(/ws:\/\/127\.0\.0\.1:9527/, (socket) =>
          socket.onMessage((raw) => {
            if (typeof raw !== "string") return;
            const req = JSON.parse(raw);
            if (req.type === "list_ports")
              socket.send(
                JSON.stringify({
                  type: "ports",
                  ports: ["COM6", "COM8", "COM11"],
                }),
              );
            if (req.type === "serial_debug_session_read_page")
              socket.send(
                JSON.stringify({
                  type: "serial_debug_session_page",
                  request_id: req.request_id,
                  page: { start: 0, totalLines: 0, items: [] },
                }),
              );
          }),
        );
        page.on("pageerror", (e) => errors.push(e.message));
        await page.goto(baseURL + "/" + path);
        await page.locator("#app-main").waitFor();
        await page.waitForTimeout(180);
        await page.evaluate(async (path) => {
          if (path === "flash") {
            const { useFlashStore } = await import("/src/stores/flash.ts");
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
              "Previous session · COM6 · T5AI",
              "Firmware: tuyaopen_t5ai_demo.bin · 1.24 MiB",
              "Write and verification completed.",
              "Serial port released.",
            ];
          }
          if (path === "serial-debug") {
            const { useSerialDebugStore } =
              await import("/src/stores/serial-debug.ts");
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
          if (path.includes("batch-flash-auth")) {
            const { useBatchFlashAuthStore } =
              await import("/src/stores/batch-flash-auth.ts");
            const s = useBatchFlashAuthStore();
            s.addPorts(["COM6", "COM8", "COM11"]);
            s.slots[0].status = "done";
            s.slots[1].status = "failed";
            s.slots[1].writeUncertain = true;
            s.slots[1].error = "Demo: authorization result is uncertain";
          }
        }, path);
        await page.evaluate(() => document.fonts.ready);
        await page.waitForTimeout(150);
        const metrics = await page.evaluate(() => {
          const main = document.querySelector("#app-main");
          return {
            documentOverflow: document.documentElement.scrollWidth - innerWidth,
            mainOverflow: main.scrollWidth - main.clientWidth,
            h1: main.querySelector("h1")?.textContent,
            controls: [...main.querySelectorAll("button,input,textarea")]
              .filter((e) => {
                const r = e.getBoundingClientRect();
                return (
                  r.width && r.height && (r.left < 0 || r.right > innerWidth)
                );
              })
              .map((e) => ({
                tag: e.tagName,
                id: e.id,
                text: e.textContent?.slice(0, 50),
              })),
            lowContrast: [...main.querySelectorAll("*")]
              .filter(
                (e) =>
                  e.children.length === 0 &&
                  e.textContent?.trim() &&
                  e.getBoundingClientRect().width > 0 &&
                  !e.closest("button:disabled,input:disabled"),
              )
              .map((e) => {
                const r = e.getBoundingClientRect();
                if (r.bottom < 0 || r.top > innerHeight) return null;
                const canvas = document.createElement("canvas");
                const context = canvas.getContext("2d");
                const rgba = (value) => {
                  context.clearRect(0, 0, 1, 1);
                  context.fillStyle = value;
                  context.fillRect(0, 0, 1, 1);
                  const data = [...context.getImageData(0, 0, 1, 1).data];
                  data[3] /= 255;
                  return data;
                };
                const over = (fg, bg) =>
                  fg
                    .slice(0, 3)
                    .map(
                      (v, i) => v * (fg[3] ?? 1) + bg[i] * (1 - (fg[3] ?? 1)),
                    );
                let bg = [255, 255, 255];
                const chain = [];
                for (let a = e; a; a = a.parentElement) chain.unshift(a);
                for (const a of chain) {
                  const c = rgba(getComputedStyle(a).backgroundColor);
                  if (c.length >= 3) bg = over(c, bg);
                }
                const css = getComputedStyle(e);
                const fg = over(rgba(css.color), bg);
                const lum = (arr) =>
                  arr
                    .map((c) => {
                      c /= 255;
                      return c <= 0.04045
                        ? c / 12.92
                        : ((c + 0.055) / 1.055) ** 2.4;
                    })
                    .reduce(
                      (a, v, i) => a + v * [0.2126, 0.7152, 0.0722][i],
                      0,
                    );
                const f = lum(fg),
                  b = lum(bg);
                const ratio = (Math.max(f, b) + 0.05) / (Math.min(f, b) + 0.05);
                const min =
                  parseFloat(css.fontSize) >= 24 ||
                  (parseFloat(css.fontSize) >= 18.66 &&
                    parseFloat(css.fontWeight) >= 700)
                    ? 3
                    : 4.5;
                return ratio < min
                  ? {
                      text: e.textContent.slice(0, 50),
                      tag: e.tagName,
                      cls: e.className,
                      ratio: +ratio.toFixed(2),
                      fg: css.color,
                      bg,
                    }
                  : null;
              })
              .filter(Boolean),
            consoleHeight: main.querySelector(".pane")?.clientHeight,
          };
        });
        results.push({ width, height, lang, theme, path, errors, ...metrics });
        await page.screenshot({
          path:
            root +
            "/" +
            path.replaceAll("/", "-") +
            "-" +
            theme +
            "-" +
            width +
            "-" +
            lang +
            ".png",
        });
        await page.close();
      }
await writeFile(root + "/views.json", JSON.stringify(results, null, 2));
const failures = results.filter(
  (r) =>
    r.errors.length ||
    r.mainOverflow > 1 ||
    r.documentOverflow > 1 ||
    r.controls.length,
);
console.log(
  JSON.stringify(
    {
      views: results.length,
      failures,
      contrastCandidates: results
        .filter((r) => r.lowContrast.length)
        .map((r) => ({
          path: r.path,
          theme: r.theme,
          width: r.width,
          lang: r.lang,
          lowContrast: r.lowContrast,
        })),
    },
    null,
    2,
  ),
);
if (failures.length) process.exitCode = 1;
await browser.close();
