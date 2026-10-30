import type { Page } from "playwright/test";

export const SERIAL_STORE_MODULE_URL = "/src/stores/serial-debug.ts";

export interface SerialDebugBrowserStore {
  lines: Array<{
    id: number;
    tsMs: number;
    direction: "rx" | "tx" | "sys";
    text: string;
    rawBytes?: Uint8Array;
  }>;
  logWindowLines: number;
  showTimestamp: boolean;
  showDirBadge: boolean;
  logFontSize: number;
  port: string;
  appendChunk(chunk: {
    direction: "rx" | "tx";
    tsMs: number;
    bytes: number[];
  }): void;
  activeDisplayLines(): Array<{ id: number; text: string }>;
}

export interface SyntheticLine {
  id: number;
  tsMs: number;
  direction: "rx" | "tx" | "sys";
  text: string;
  rawBytes?: number[];
}

export async function openUi(
  page: Page,
  path: string,
  locale = "en",
  mockPorts: string[] = [],
  archive: { total: number; failOnceAt?: number } = { total: 0 },
) {
  await page.addInitScript(
    ({ locale }) => {
      localStorage.setItem("tyutool-locale", locale);
      localStorage.setItem("tyutool-theme", "light");
    },
    { locale },
  );
  await page.routeWebSocket(
    new RegExp("^ws://127[.]0[.]0[.]1:9527/?$"),
    (socket) => {
      let failed = false;
      socket.onMessage((message) => {
        if (typeof message !== "string") return;
        let request: {
          type?: string;
          start?: number;
          limit?: number;
          request_id?: string;
        };
        try {
          request = JSON.parse(message) as typeof request;
        } catch {
          return;
        }
        if (request.type === "list_ports") {
          socket.send(JSON.stringify({ type: "ports", ports: mockPorts }));
        } else if (request.type === "serial_debug_session_read_page") {
          const start = Math.min(archive.total, request.start ?? 0);
          if (!failed && archive.failOnceAt === start) {
            failed = true;
            socket.send(
              JSON.stringify({
                type: "error",
                request_id: request.request_id,
                message: "temporary archive failure",
              }),
            );
            return;
          }
          socket.send(
            JSON.stringify({
              type: "serial_debug_session_page",
              request_id: request.request_id,
              page: {
                totalLines: archive.total,
                start,
                items: Array.from(
                  {
                    length: Math.min(request.limit ?? 0, archive.total - start),
                  },
                  (_, i) => ({
                    lineNo: start + i + 1,
                    tsMs: 1_700_000_000_000 + start + i,
                    direction: "rx",
                    text: `archive-${start + i + 1}`,
                    rawBytes: [
                      ...new TextEncoder().encode(
                        `archive-${start + i + 1}\r\n`,
                      ),
                    ],
                  }),
                ),
              },
            }),
          );
        }
      });
    },
  );
  await page.goto(path);
  await page.locator(".main-scroll").waitFor({ state: "visible" });
  if (path.includes("serial-debug"))
    await page.locator(".pane .line-probe").waitFor({ state: "attached" });
}

export async function seedSerialLines(page: Page, lines: SyntheticLine[]) {
  const moduleUrl = SERIAL_STORE_MODULE_URL;
  await page.evaluate(
    async ({ items, moduleUrl }) => {
      const module = (await import(moduleUrl)) as {
        useSerialDebugStore: () => SerialDebugBrowserStore;
      };
      const store = module.useSerialDebugStore();
      store.logWindowLines = Math.max(1000, items.length + 20);
      store.lines = items.map((line) => ({
        ...line,
        rawBytes: line.rawBytes
          ? Uint8Array.from(line.rawBytes)
          : new TextEncoder().encode(line.text),
      }));
    },
    { items: lines, moduleUrl },
  );
  await page
    .locator(".pane [data-line-id]")
    .first()
    .waitFor({ state: "attached" });
}

export async function appendAlternatingLines(page: Page, pairs: number) {
  const moduleUrl = SERIAL_STORE_MODULE_URL;
  await page.evaluate(
    ({ moduleUrl, pairs }) => {
      const modulePromise = import(moduleUrl) as Promise<{
        useSerialDebugStore: () => SerialDebugBrowserStore;
      }>;
      return modulePromise.then(({ useSerialDebugStore }) => {
        const store = useSerialDebugStore();
        store.logWindowLines = pairs * 2 + 20;
        const bytes = new TextEncoder().encode("normal\n\n".repeat(pairs));
        store.appendChunk({
          direction: "rx",
          tsMs: Date.now(),
          bytes: Array.from(bytes),
        });
      });
    },
    { moduleUrl, pairs },
  );
  await page
    .locator(".pane [data-line-id]")
    .first()
    .waitFor({ state: "attached" });
}

export function makeMixedSerialLines(count: number): SyntheticLine[] {
  const now = Date.now();
  return Array.from({ length: count }, (_, index) => {
    const direction = index % 17 === 0 ? "sys" : index % 3 === 0 ? "tx" : "rx";
    const text =
      index % 37 === 0
        ? JSON.stringify({
            event: "device_status",
            payload: "protocol-field ".repeat(12),
            sequence: index,
          })
        : `sensor=${index % 8} value=${index} direction=${direction}`;
    return {
      id: index + 1,
      tsMs: now - (count - index) * 50,
      direction,
      text,
      rawBytes: [...new TextEncoder().encode(text)],
    };
  });
}
