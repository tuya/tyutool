/**
 * WebSocket transport for browser-mode flashing.
 *
 * Connects to the local tyutool-cli serve process on the current page host.
 * Used automatically when isTauriRuntime() === false.
 */

import type { FlashJobPayload, FlashProgressPayload } from './flash-tauri';
import type { TauriSerialPortRow } from './serial-port-label';

const WS_PORT = '9527';

function wsUrl(): string {
  const host = typeof window !== 'undefined' && window.location.hostname ? window.location.hostname : '127.0.0.1';
  return `ws://${host}:${WS_PORT}`;
}

export interface WsProgressEvent {
  payload: FlashProgressPayload;
  fileContent?: { name: string; content: string } | null;
}

export class WsTransport {
  private ws: WebSocket | null = null;
  private connectPromise: Promise<WebSocket> | null = null;
  private activeSerialDebugChunkHandler: ((ev: MessageEvent) => void) | null = null;

  private closeCurrentConnection(): void {
    const ws = this.ws;
    this.ws = null;
    this.connectPromise = null;
    if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) {
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
      ws.onerror = () => reject(new Error(`Cannot connect to tyutool-cli serve at ${url}`));
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
        ws.removeEventListener('message', handler);
        fn();
      };

      const timeout = setTimeout(() => {
        ws.removeEventListener('message', handler);
        reject(
          new Error(
            'deviceReset timeout — 请重新编译并启动 tyutool-cli serve（需支持 device_reset），并确认 ws://127.0.0.1:9527 可达'
          )
        );
      }, 15000);

      const handler = (ev: MessageEvent) => {
        let msg: { type: string; ok?: boolean; error?: string; message?: string };
        try {
          msg = JSON.parse(ev.data as string) as typeof msg;
        } catch {
          return;
        }
        // 服务端对未知 type 或 JSON 解析失败时会发 { type: "error", message }
        if (msg.type === 'error') {
          finish(() => reject(new Error(msg.message ?? 'server error')));
          return;
        }
        if (msg.type === 'device_reset_result') {
          finish(() => {
            if (msg.ok) {
              resolve();
            } else {
              reject(new Error(msg.error ?? 'device reset failed'));
            }
          });
        }
      };
      ws.addEventListener('message', handler);
      ws.send(JSON.stringify({ type: 'device_reset', port, chip_id: chipId }));
    });
  }

  async listPorts(): Promise<TauriSerialPortRow[]> {
    // Port refresh must not reuse an old dev-server socket. During dev, `pnpm run dev:web`
    // can restart the listener while an existing browser WebSocket stays connected to
    // an orphaned serve process with stale serial-port state.
    this.closeCurrentConnection();
    const ws = await this.connect();
    return new Promise<TauriSerialPortRow[]>((resolve, reject) => {
      const timeout = setTimeout(() => {
        ws.removeEventListener('message', handler);
        reject(new Error('listPorts timeout'));
      }, 5000);
      const handler = (ev: MessageEvent) => {
        let msg: { type: string; ports?: Array<string | TauriSerialPortRow> };
        try {
          msg = JSON.parse(ev.data as string) as typeof msg;
        } catch {
          return;
        }
        if (msg.type === 'ports') {
          clearTimeout(timeout);
          ws.removeEventListener('message', handler);
          resolve(
            (msg.ports ?? []).map(p => {
              if (typeof p === 'string') {
                return { path: p };
              }
              return p;
            })
          );
        }
      };
      ws.addEventListener('message', handler);
      ws.send(JSON.stringify({ type: 'list_ports' }));
    });
  }

  /** Read-only authorize job; returns device UUID/AuthKey from `flash.log.auth.readResult` if present. */
  async authorizeProbe(
    port: string,
    chipId: string,
    baudRate: number
  ): Promise<{ uuid: string; authkey: string } | null> {
    const job: FlashJobPayload = {
      mode: 'authorize',
      chipId,
      port,
      baudRate,
      segments: null,
      flashStartHex: null,
      flashEndHex: null,
      eraseStartHex: null,
      eraseEndHex: null,
      readStartHex: null,
      readEndHex: null,
      readFilePath: null,
      firmwarePath: null,
      authorizeUuid: null,
      authorizeKey: null,
    };
    let found: { uuid: string; authkey: string } | null = null;
    await this.runJob(job, [], ev => {
      if (ev.payload.kind === 'milestone' && typeof ev.payload.milestone === 'object'
          && 'auth_read_complete' in ev.payload.milestone) {
        const { uuid, authkey } = ev.payload.milestone.auth_read_complete;
        if (uuid && authkey) {
          found = { uuid: uuid.trim(), authkey: authkey.trim() };
        }
      }
    });
    return found;
  }

  async runJob(
    job: FlashJobPayload,
    firmwareFiles: Array<File | null>,
    onProgress: (ev: WsProgressEvent) => void
  ): Promise<void> {
    const ws = await this.connect();

    // Encode firmware files as base64 if provided
    let fileContent: string | undefined;
    let fileContents: string[] | undefined;

    if (job.mode === 'flash' && job.segments && job.segments.length > 0) {
      fileContents = [];
      for (const file of firmwareFiles) {
        if (file) {
          const buf = await file.arrayBuffer();
          fileContents.push(bufferToBase64(buf));
        } else {
          fileContents.push('');
        }
      }
    } else if (firmwareFiles.length > 0 && firmwareFiles[0]) {
      const buf = await firmwareFiles[0].arrayBuffer();
      fileContent = bufferToBase64(buf);
    }

    // Map frontend FlashJobPayload (camelCase) → wire format
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
        const msg = JSON.parse(ev.data as string) as {
          type: string;
          payload?: Record<string, unknown>;
          message?: string;
        };

        if (msg.type === 'error') {
          ws.removeEventListener('message', handler);
          reject(new Error(msg.message ?? 'unknown error'));
          return;
        }

        if (msg.type === 'progress' && msg.payload) {
          const p = msg.payload;
          const kind = p['kind'] as string;

          // Intercept file_content meta-message (read mode result)
          if (kind === 'file_content') {
            pendingFileContent = {
              name: (p['name'] as string) ?? 'read.bin',
              content: (p['content'] as string) ?? '',
            };
            return;
          }

          onProgress({
            payload: p as unknown as FlashProgressPayload,
            fileContent: pendingFileContent,
          });
          pendingFileContent = null;

          if (kind === 'done') {
            ws.removeEventListener('message', handler);
            const result = p['result'] as Record<string, unknown>;
            if ('ok' in result) {
              resolve();
            } else if ('cancelled' in result) {
              reject(new Error('Cancelled'));
            } else {
              const msg = (result['err'] as Record<string, unknown>)?.['message'] as string | undefined;
              reject(new Error(msg ?? 'operation failed'));
            }
          }
        }
      };

      ws.addEventListener('message', handler);
      ws.send(
        JSON.stringify({
          type: 'run_job',
          job: wireJob,
          ...(fileContent ? { file_content: fileContent } : {}),
          ...(fileContents ? { file_contents: fileContents } : {}),
        })
      );
    });
  }

  cancelJob(): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: 'cancel' }));
    }
  }

  async serialDebugOpen(
    cfg: import('../serial-debug/types').DebugConfig,
    onChunk: (chunk: import('../serial-debug/types').DebugChunk) => void,
    onDisconnect: (reason: string) => void,
  ): Promise<void> {
    const ws = await this.connect();
    return new Promise((resolve, reject) => {
      const handler = (ev: MessageEvent) => {
        let msg: { type: string; chunk?: { direction: 'tx'|'rx'; tsMs: number; bytes: number[] }; reason?: string; message?: string };
        try { msg = JSON.parse(ev.data as string); } catch { return; }
        switch (msg.type) {
          case 'serial_debug_opened':
            ws.addEventListener('message', chunkHandler);
            ws.removeEventListener('message', handler);
            resolve();
            break;
          case 'error':
            ws.removeEventListener('message', handler);
            reject(new Error(msg.message ?? 'ws error'));
            break;
          // ignore other message types here
        }
      };
      const chunkHandler = (ev: MessageEvent) => {
        try {
          const m = JSON.parse(ev.data as string) as { type: string; chunk?: import('../serial-debug/types').DebugChunk; reason?: string };
          if (m.type === 'serial_debug_chunk' && m.chunk) onChunk(m.chunk);
          else if (m.type === 'serial_debug_disconnected') onDisconnect(m.reason ?? '');
        } catch { /* ignore */ }
      };
      this.activeSerialDebugChunkHandler = chunkHandler;
      ws.addEventListener('message', handler);
      ws.send(JSON.stringify({ type: 'serial_debug_open', cfg }));
    });
  }

  async serialDebugClose(): Promise<void> {
    // Remove the message listener BEFORE sending the close — prevents late-arriving
    // chunks after close from reaching stale handlers.
    if (this.ws && this.activeSerialDebugChunkHandler) {
      this.ws.removeEventListener('message', this.activeSerialDebugChunkHandler);
      this.activeSerialDebugChunkHandler = null;
    }
    // Only send close if the socket is actually open; do not force-reconnect just to close.
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type: 'serial_debug_close' }));
    }
  }

  async serialDebugSend(bytes: Uint8Array): Promise<void> {
    const ws = await this.connect();
    ws.send(JSON.stringify({ type: 'serial_debug_send', bytes: Array.from(bytes) }));
  }
}

function bufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = '';
  for (let i = 0; i < bytes.byteLength; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

/** Singleton for use across flash store lifecycle. */
export const wsTransport = new WsTransport();
