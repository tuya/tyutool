/**
 * WebSocket transport for browser-mode flashing.
 *
 * Connects to the local tyutool-cli serve process on the current page host.
 * Used automatically when isTauriRuntime() === false.
 */

import type {
  FlashJobPayload,
  FlashProgressPayload,
} from "@/features/firmware-flash/flash-ipc-types";
import type {
  SerialDebugFilterPage,
  SerialDebugSessionPage,
  SerialDebugFilterUpdatePayload,
} from "@/features/serial-debug/types";
import type { TauriSerialPortRow } from "@/utils/serial-port-label";
import { platform } from "@/platform";
import { i18n } from "@/i18n";

const WS_PORT = "9527";

function wsUrl(): string {
  const injected = platform.getWsUrl();
  if (injected) return injected;
  const host =
    typeof window !== "undefined" && window.location.hostname
      ? window.location.hostname
      : "127.0.0.1";
  return `ws://${host}:${WS_PORT}`;
}

export interface WsProgressEvent {
  payload: FlashProgressPayload;
  fileContent?: { name: string; content: string } | null;
}

export class WsTransport {
  private ws: WebSocket | null = null;
  private connectPromise: Promise<WebSocket> | null = null;
  private activeSerialDebugChunkHandler: ((ev: MessageEvent) => void) | null =
    null;
  private nextSerialDebugRequestId = 1;

  private newSerialDebugRequestId(prefix: string): string {
    const id = `${prefix}-${this.nextSerialDebugRequestId}`;
    this.nextSerialDebugRequestId += 1;
    return id;
  }

  private closeCurrentConnection(): void {
    const ws = this.ws;
    this.ws = null;
    this.connectPromise = null;
    if (
      ws &&
      (ws.readyState === WebSocket.OPEN ||
        ws.readyState === WebSocket.CONNECTING)
    ) {
      ws.close();
    }
  }

  private async connect(): Promise<WebSocket> {
    if (this.ws?.readyState === WebSocket.OPEN) return this.ws;
    if (this.connectPromise) return this.connectPromise;

    this.connectPromise = new Promise<WebSocket>((resolve, reject) => {
      const url = wsUrl();
      const ws = new WebSocket(url);
      ws.onopen = () => {
        this.ws = ws;
        this.connectPromise = null;
        resolve(ws);
      };
      ws.onerror = () =>
        reject(new Error(`Cannot connect to tyutool-cli serve at ${url}`));
      ws.onclose = () => {
        if (this.ws === ws) {
          this.ws = null;
          this.connectPromise = null;
        }
      };
    });

    return this.connectPromise;
  }

  async isAvailable(): Promise<boolean> {
    try {
      await this.connect();
      return true;
    } catch {
      return false;
    }
  }

  async deviceReset(port: string, chipId: string): Promise<void> {
    const ws = await this.connect();
    return new Promise((resolve, reject) => {
      const finish = (fn: () => void) => {
        clearTimeout(timeout);
        ws.removeEventListener("message", handler);
        fn();
      };

      const timeout = setTimeout(() => {
        ws.removeEventListener("message", handler);
        reject(new Error(i18n.global.t("flash.log.deviceResetTimeout")));
      }, 15000);

      const handler = (ev: MessageEvent) => {
        let msg: {
          type: string;
          ok?: boolean;
          error?: string;
          message?: string;
        };
        try {
          msg = JSON.parse(ev.data as string) as typeof msg;
        } catch {
          return;
        }
        if (msg.type === "error") {
          finish(() => reject(new Error(msg.message ?? "server error")));
          return;
        }
        if (msg.type === "device_reset_result") {
          finish(() => {
            if (msg.ok) {
              resolve();
            } else {
              reject(new Error(msg.error ?? "device reset failed"));
            }
          });
        }
      };
      ws.addEventListener("message", handler);
      ws.send(JSON.stringify({ type: "device_reset", port, chip_id: chipId }));
    });
  }

  async serialDebugDeviceReset(chipId: string): Promise<void> {
    const ws = await this.connect();
    return new Promise((resolve, reject) => {
      const finish = (fn: () => void) => {
        clearTimeout(timeout);
        ws.removeEventListener("message", handler);
        fn();
      };

      const timeout = setTimeout(() => {
        ws.removeEventListener("message", handler);
        reject(
          new Error(i18n.global.t("flash.log.serialDebugDeviceResetTimeout")),
        );
      }, 15000);

      const handler = (ev: MessageEvent) => {
        let msg: {
          type: string;
          ok?: boolean;
          error?: string;
          message?: string;
        };
        try {
          msg = JSON.parse(ev.data as string) as typeof msg;
        } catch {
          return;
        }
        if (msg.type === "error") {
          finish(() => reject(new Error(msg.message ?? "server error")));
          return;
        }
        if (msg.type === "serial_debug_device_reset_result") {
          finish(() => {
            if (msg.ok) {
              resolve();
            } else {
              reject(
                new Error(msg.error ?? "serial debug device reset failed"),
              );
            }
          });
        }
      };
      ws.addEventListener("message", handler);
      ws.send(
        JSON.stringify({
          type: "serial_debug_device_reset",
          chip_id: chipId,
        }),
      );
    });
  }

  async listPorts(): Promise<TauriSerialPortRow[]> {
    // Reuse an open socket instead of forcibly resetting it. A previous
    // implementation called closeCurrentConnection() here, which tore down the
    // shared WebSocket and orphaned the active serial-debug chunk handler
    // (registered via addEventListener on the old socket — it was never
    // re-attached on reconnect), silently stopping serial-debug RX/TX after a
    // device refresh. Only reset when the socket is in a bad (non-OPEN,
    // non-CONNECTING) state; otherwise connect() returns the live one.
    if (
      !this.ws ||
      (this.ws.readyState !== WebSocket.OPEN &&
        this.ws.readyState !== WebSocket.CONNECTING)
    ) {
      this.closeCurrentConnection();
    }
    const ws = await this.connect();
    return new Promise<TauriSerialPortRow[]>((resolve, reject) => {
      const timeout = setTimeout(() => {
        ws.removeEventListener("message", handler);
        reject(new Error("listPorts timeout"));
      }, 5000);
      const handler = (ev: MessageEvent) => {
        let msg: { type: string; ports?: Array<string | TauriSerialPortRow> };
        try {
          msg = JSON.parse(ev.data as string) as typeof msg;
        } catch {
          return;
        }
        if (msg.type === "ports") {
          clearTimeout(timeout);
          ws.removeEventListener("message", handler);
          resolve(
            (msg.ports ?? []).map((p) => {
              if (typeof p === "string") {
                return { path: p };
              }
              return p;
            }),
          );
        }
      };
      ws.addEventListener("message", handler);
      ws.send(JSON.stringify({ type: "list_ports" }));
    });
  }

  authorizeConfirm(confirmed: boolean): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: "authorize_confirm", confirmed }));
    }
  }

  async runJob(
    job: FlashJobPayload,
    firmwareFiles: Array<File | null>,
    onProgress: (ev: WsProgressEvent) => void,
  ): Promise<void> {
    const ws = await this.connect();

    let fileContent: string | undefined;
    let fileContents: string[] | undefined;

    if (job.mode === "flash" && job.segments && job.segments.length > 0) {
      fileContents = [];
      for (const file of firmwareFiles) {
        if (file) {
          const buf = await file.arrayBuffer();
          fileContents.push(bufferToBase64(buf));
        } else {
          fileContents.push("");
        }
      }
    } else if (firmwareFiles.length > 0 && firmwareFiles[0]) {
      const buf = await firmwareFiles[0].arrayBuffer();
      fileContent = bufferToBase64(buf);
    }

    const wireJob = {
      mode: job.mode,
      chipId: job.chipId,
      port: job.port,
      baudRate: job.baudRate,
      segments: job.segments ?? null,
      flashStartHex: job.flashStartHex ?? null,
      flashEndHex: job.flashEndHex ?? null,
      eraseStartHex: job.eraseStartHex ?? null,
      eraseEndHex: job.eraseEndHex ?? null,
      readStartHex: job.readStartHex ?? null,
      readEndHex: job.readEndHex ?? null,
      readFilePath: job.readFilePath ?? null,
      firmwarePath: job.firmwarePath ?? null,
      authorizeUuid: job.authorizeUuid ?? null,
      authorizeKey: job.authorizeKey ?? null,
    };

    return new Promise<void>((resolve, reject) => {
      let pendingFileContent: { name: string; content: string } | null = null;

      const handler = (ev: MessageEvent) => {
        let msg: {
          type: string;
          payload?: Record<string, unknown>;
          message?: string;
        };
        try {
          msg = JSON.parse(ev.data as string) as {
            type: string;
            payload?: Record<string, unknown>;
            message?: string;
          };
        } catch {
          return;
        }

        if (msg.type === "error") {
          ws.removeEventListener("message", handler);
          reject(new Error(msg.message ?? "unknown error"));
          return;
        }

        if (msg.type === "progress" && msg.payload) {
          const p = msg.payload;
          const kind = p["kind"] as string;

          if (kind === "file_content") {
            pendingFileContent = {
              name: (p["name"] as string) ?? "read.bin",
              content: (p["content"] as string) ?? "",
            };
            return;
          }

          onProgress({
            payload: p as unknown as FlashProgressPayload,
            fileContent: pendingFileContent,
          });
          pendingFileContent = null;

          if (kind === "done") {
            ws.removeEventListener("message", handler);
            const result = p["result"] as Record<string, unknown>;
            if ("ok" in result) {
              resolve();
            } else if ("cancelled" in result) {
              reject(new Error("Cancelled"));
            } else {
              const errMsg = (result["err"] as Record<string, unknown>)?.[
                "message"
              ] as string | undefined;
              reject(new Error(errMsg ?? "operation failed"));
            }
          }
        }
      };

      ws.addEventListener("message", handler);
      ws.send(
        JSON.stringify({
          type: "run_job",
          job: wireJob,
          ...(fileContent ? { file_content: fileContent } : {}),
          ...(fileContents ? { file_contents: fileContents } : {}),
        }),
      );
    });
  }

  cancelJob(): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: "cancel" }));
    }
  }

  async serialDebugOpen(
    cfg: import("@/features/serial-debug/types").DebugConfig,
    onChunk: (
      chunk: import("@/features/serial-debug/types").DebugChunk,
    ) => void,
    onChunkBatch: (
      chunks: import("@/features/serial-debug/types").DebugChunk[],
    ) => void,
    onDisconnect: (reason: string) => void,
    onFilterUpdated: (payload: SerialDebugFilterUpdatePayload) => void,
  ): Promise<void> {
    const ws = await this.connect();
    return new Promise((resolve, reject) => {
      const handler = (ev: MessageEvent) => {
        let msg: {
          type: string;
          chunk?: { direction: "tx" | "rx"; tsMs: number; bytes: number[] };
          reason?: string;
          message?: string;
        };
        try {
          msg = JSON.parse(ev.data as string);
        } catch {
          return;
        }
        switch (msg.type) {
          case "serial_debug_opened":
            ws.addEventListener("message", chunkHandler);
            ws.removeEventListener("message", handler);
            resolve();
            break;
          case "error":
            ws.removeEventListener("message", handler);
            reject(new Error(msg.message ?? "ws error"));
            break;
        }
      };
      const chunkHandler = (ev: MessageEvent) => {
        try {
          const m = JSON.parse(ev.data as string) as {
            type: string;
            chunk?: import("@/features/serial-debug/types").DebugChunk;
            chunks?: import("@/features/serial-debug/types").DebugChunk[];
            reason?: string;
            def?: SerialDebugFilterUpdatePayload["def"];
            stats?: SerialDebugFilterUpdatePayload["stats"];
          };
          if (m.type === "serial_debug_chunk" && m.chunk) onChunk(m.chunk);
          else if (m.type === "serial_debug_chunk_batch" && m.chunks)
            onChunkBatch(m.chunks);
          else if (m.type === "serial_debug_disconnected")
            onDisconnect(m.reason ?? "");
          else if (m.type === "serial_debug_filter_updated" && m.def && m.stats)
            onFilterUpdated({ def: m.def, stats: m.stats });
        } catch {
          /* ignore */
        }
      };
      this.activeSerialDebugChunkHandler = chunkHandler;
      ws.addEventListener("message", handler);
      ws.send(JSON.stringify({ type: "serial_debug_open", cfg }));
    });
  }

  async serialDebugClose(): Promise<void> {
    if (this.ws && this.activeSerialDebugChunkHandler) {
      this.ws.removeEventListener(
        "message",
        this.activeSerialDebugChunkHandler,
      );
      this.activeSerialDebugChunkHandler = null;
    }
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: "serial_debug_close" }));
    }
  }

  async serialDebugSend(bytes: Uint8Array): Promise<void> {
    const ws = await this.connect();
    ws.send(
      JSON.stringify({ type: "serial_debug_send", bytes: Array.from(bytes) }),
    );
  }

  async serialDebugSessionClear(): Promise<void> {
    const ws = await this.connect();
    ws.send(JSON.stringify({ type: "serial_debug_session_clear" }));
  }

  async serialDebugAppendSysLine(tsMs: number, text: string): Promise<void> {
    const ws = await this.connect();
    ws.send(
      JSON.stringify({
        type: "serial_debug_append_sys_line",
        ts_ms: tsMs,
        text,
      }),
    );
  }

  async serialDebugFilterAdd(
    keyword: string,
    useRegex: boolean,
    color: string,
  ): Promise<SerialDebugFilterUpdatePayload> {
    const ws = await this.connect();
    const requestId = this.newSerialDebugRequestId("serial-debug-filter-add");
    return new Promise((resolve, reject) => {
      const handler = (ev: MessageEvent) => {
        let msg: {
          type: string;
          def?: SerialDebugFilterUpdatePayload["def"];
          stats?: SerialDebugFilterUpdatePayload["stats"];
          message?: string;
          request_id?: string;
        };
        try {
          msg = JSON.parse(ev.data as string);
        } catch {
          return;
        }
        if (msg.request_id !== requestId) {
          return;
        }
        if (msg.type === "error") {
          ws.removeEventListener("message", handler);
          reject(new Error(msg.message ?? "ws error"));
          return;
        }
        if (
          msg.type === "serial_debug_filter_updated" &&
          msg.def &&
          msg.stats
        ) {
          ws.removeEventListener("message", handler);
          resolve({ def: msg.def, stats: msg.stats });
        }
      };
      ws.addEventListener("message", handler);
      ws.send(
        JSON.stringify({
          type: "serial_debug_filter_add",
          keyword,
          use_regex: useRegex,
          color,
          request_id: requestId,
        }),
      );
    });
  }

  async serialDebugFilterRemove(filterId: string): Promise<void> {
    const ws = await this.connect();
    ws.send(
      JSON.stringify({
        type: "serial_debug_filter_remove",
        filter_id: filterId,
      }),
    );
  }

  async serialDebugFilterReadMatches(
    filterId: string,
    start: number,
    limit: number,
  ): Promise<SerialDebugFilterPage> {
    const ws = await this.connect();
    const requestId = this.newSerialDebugRequestId("serial-debug-filter-page");
    return new Promise((resolve, reject) => {
      const handler = (ev: MessageEvent) => {
        let msg: {
          type: string;
          page?: SerialDebugFilterPage;
          message?: string;
          request_id?: string;
        };
        try {
          msg = JSON.parse(ev.data as string);
        } catch {
          return;
        }
        if (msg.request_id !== requestId) {
          return;
        }
        if (msg.type === "error") {
          ws.removeEventListener("message", handler);
          reject(new Error(msg.message ?? "ws error"));
          return;
        }
        if (msg.type === "serial_debug_filter_page" && msg.page) {
          ws.removeEventListener("message", handler);
          resolve(msg.page);
        }
      };
      ws.addEventListener("message", handler);
      ws.send(
        JSON.stringify({
          type: "serial_debug_filter_read_matches",
          filter_id: filterId,
          start,
          limit,
          request_id: requestId,
        }),
      );
    });
  }

  async serialDebugSessionReadPage(
    start: number,
    limit: number,
  ): Promise<SerialDebugSessionPage> {
    const ws = await this.connect();
    const requestId = this.newSerialDebugRequestId("serial-debug-session-page");
    return new Promise((resolve, reject) => {
      const handler = (ev: MessageEvent) => {
        let msg: {
          type: string;
          page?: SerialDebugSessionPage;
          message?: string;
          request_id?: string;
        };
        try {
          msg = JSON.parse(ev.data as string);
        } catch {
          return;
        }
        if (msg.request_id !== requestId) {
          return;
        }
        if (msg.type === "error") {
          ws.removeEventListener("message", handler);
          reject(new Error(msg.message ?? "ws error"));
          return;
        }
        if (msg.type === "serial_debug_session_page" && msg.page) {
          ws.removeEventListener("message", handler);
          resolve(msg.page);
        }
      };
      ws.addEventListener("message", handler);
      ws.send(
        JSON.stringify({
          type: "serial_debug_session_read_page",
          start,
          limit,
          request_id: requestId,
        }),
      );
    });
  }
}

function bufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (let i = 0; i < bytes.byteLength; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

/** Singleton for use across flash store lifecycle. */
export const wsTransport = new WsTransport();
