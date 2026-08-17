import { isTauriRuntime } from "@/runtime";
import { wsTransport } from "@/transport/ws-transport";
import type {
  ArchiveCappedPayload,
  DebugChunk,
  DebugConfig,
  DisconnectPayload,
  SerialDebugFilterPage,
  SerialDebugSessionPage,
  SerialDebugFilterUpdatePayload,
} from "./types";

type ChunkListener = (chunk: DebugChunk) => void;
type ChunkBatchListener = (chunks: DebugChunk[]) => void;
type DisconnectListener = (p: DisconnectPayload) => void;
type FilterListener = (payload: SerialDebugFilterUpdatePayload) => void;
type ArchiveCappedListener = (p: ArchiveCappedPayload) => void;

export interface SerialDebugTransport {
  open(cfg: DebugConfig): Promise<void>;
  close(): Promise<void>;
  send(bytes: Uint8Array): Promise<void>;
  clearSession(): Promise<void>;
  appendSysLine(tsMs: number, text: string): Promise<void>;
  addFilter(
    keyword: string,
    useRegex: boolean,
    color: string,
  ): Promise<SerialDebugFilterUpdatePayload>;
  removeFilter(filterId: string): Promise<void>;
  readFilterMatches(
    filterId: string,
    start: number,
    limit: number,
  ): Promise<SerialDebugFilterPage>;
  readSessionPage(
    start: number,
    limit: number,
  ): Promise<SerialDebugSessionPage>;
  /** Push the session-archive byte cap down to Rust (0 = unlimited). */
  setArchiveLimit(maxBytes: number): Promise<void>;
  onChunk(cb: ChunkListener): () => void; // returns unsubscribe
  onChunkBatch(cb: ChunkBatchListener): () => void;
  onDisconnect(cb: DisconnectListener): () => void;
  onFilterUpdated(cb: FilterListener): () => void;
  /**
   * The session archive stopped recording. Its own event because the live view
   * is fed by the raw chunk stream and never sees archived lines — the notice
   * the archive wrote into itself would otherwise be invisible to the user.
   */
  onArchiveCapped(cb: ArchiveCappedListener): () => void;
}

/** Lazy Tauri transport — uses @tauri-apps/api. Loads dynamically so web builds stay lean. */
class TauriTransport implements SerialDebugTransport {
  private chunkListeners = new Set<ChunkListener>();
  private chunkBatchListeners = new Set<ChunkBatchListener>();
  private disconnectListeners = new Set<DisconnectListener>();
  private filterListeners = new Set<FilterListener>();
  private archiveCappedListeners = new Set<ArchiveCappedListener>();
  private unlistenChunk?: () => void;
  private unlistenChunkBatch?: () => void;
  private unlistenDisconnect?: () => void;
  private unlistenFilterUpdated?: () => void;
  private unlistenArchiveCapped?: () => void;
  private listenersReady: Promise<void>;

  constructor() {
    this.listenersReady = this.ensureListeners();
  }

  private async ensureListeners(): Promise<void> {
    if (
      this.unlistenChunk &&
      this.unlistenChunkBatch &&
      this.unlistenDisconnect &&
      this.unlistenFilterUpdated &&
      this.unlistenArchiveCapped
    ) {
      return;
    }
    const { listen } = await import("@tauri-apps/api/event");
    this.unlistenChunk = await listen<DebugChunk>(
      "serial-debug-chunk",
      (ev) => {
        this.chunkListeners.forEach((l) => l(ev.payload));
      },
    );
    this.unlistenChunkBatch = await listen<DebugChunk[]>(
      "serial-debug-chunk-batch",
      (ev) => {
        if (this.chunkBatchListeners.size > 0) {
          this.chunkBatchListeners.forEach((l) => l(ev.payload));
          return;
        }
        for (const chunk of ev.payload) {
          this.chunkListeners.forEach((l) => l(chunk));
        }
      },
    );
    this.unlistenDisconnect = await listen<DisconnectPayload>(
      "serial-debug-disconnected",
      (ev) => {
        this.disconnectListeners.forEach((l) => l(ev.payload));
      },
    );
    this.unlistenFilterUpdated = await listen<SerialDebugFilterUpdatePayload>(
      "serial-debug-filter-updated",
      (ev) => {
        this.filterListeners.forEach((l) => l(ev.payload));
      },
    );
    this.unlistenArchiveCapped = await listen<ArchiveCappedPayload>(
      "serial-debug-archive-capped",
      (ev) => {
        this.archiveCappedListeners.forEach((l) => l(ev.payload));
      },
    );
  }

  async open(cfg: DebugConfig): Promise<void> {
    await this.listenersReady;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("serial_debug_open", { cfg });
  }

  async close(): Promise<void> {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("serial_debug_close");
  }

  async send(bytes: Uint8Array): Promise<void> {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("serial_debug_send", { bytes: Array.from(bytes) });
  }

  async clearSession(): Promise<void> {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("serial_debug_session_clear");
  }

  async appendSysLine(tsMs: number, text: string): Promise<void> {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("serial_debug_append_sys_line", { tsMs, text });
  }

  async addFilter(
    keyword: string,
    useRegex: boolean,
    color: string,
  ): Promise<SerialDebugFilterUpdatePayload> {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke("serial_debug_filter_add", {
      args: { keyword, useRegex, color },
    });
  }

  async removeFilter(filterId: string): Promise<void> {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("serial_debug_filter_remove", { filterId });
  }

  async readFilterMatches(
    filterId: string,
    start: number,
    limit: number,
  ): Promise<SerialDebugFilterPage> {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke("serial_debug_filter_read_matches", {
      filterId,
      start,
      limit,
    });
  }

  async readSessionPage(
    start: number,
    limit: number,
  ): Promise<SerialDebugSessionPage> {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke("serial_debug_session_read_page", { start, limit });
  }

  async setArchiveLimit(maxBytes: number): Promise<void> {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("serial_debug_set_archive_limit", { maxBytes });
  }

  onChunk(cb: ChunkListener): () => void {
    this.chunkListeners.add(cb);
    return () => {
      this.chunkListeners.delete(cb);
    };
  }

  onChunkBatch(cb: ChunkBatchListener): () => void {
    this.chunkBatchListeners.add(cb);
    return () => {
      this.chunkBatchListeners.delete(cb);
    };
  }

  onDisconnect(cb: DisconnectListener): () => void {
    this.disconnectListeners.add(cb);
    return () => {
      this.disconnectListeners.delete(cb);
    };
  }

  onFilterUpdated(cb: FilterListener): () => void {
    this.filterListeners.add(cb);
    return () => {
      this.filterListeners.delete(cb);
    };
  }

  onArchiveCapped(cb: ArchiveCappedListener): () => void {
    this.archiveCappedListeners.add(cb);
    return () => {
      this.archiveCappedListeners.delete(cb);
    };
  }
}

/** Web mode transport — talks to `tyutool-cli serve` over WebSocket via wsTransport. */
class WebTransport implements SerialDebugTransport {
  private chunkListeners = new Set<ChunkListener>();
  private chunkBatchListeners = new Set<ChunkBatchListener>();
  private disconnectListeners = new Set<DisconnectListener>();
  private filterListeners = new Set<FilterListener>();
  private archiveCappedListeners = new Set<ArchiveCappedListener>();
  private isOpen = false;

  async open(cfg: DebugConfig): Promise<void> {
    await wsTransport.serialDebugOpen(
      cfg,
      (chunk) => this.chunkListeners.forEach((l) => l(chunk)),
      (chunks) => {
        if (this.chunkBatchListeners.size > 0) {
          this.chunkBatchListeners.forEach((l) => l(chunks));
          return;
        }
        for (const chunk of chunks) {
          this.chunkListeners.forEach((l) => l(chunk));
        }
      },
      (reason) => this.disconnectListeners.forEach((l) => l({ reason })),
      (payload) => this.filterListeners.forEach((l) => l(payload)),
      (payload) => this.archiveCappedListeners.forEach((l) => l(payload)),
    );
    this.isOpen = true;
  }

  async close(): Promise<void> {
    if (!this.isOpen) return;
    await wsTransport.serialDebugClose();
    this.isOpen = false;
  }

  async send(bytes: Uint8Array): Promise<void> {
    await wsTransport.serialDebugSend(bytes);
  }

  async clearSession(): Promise<void> {
    await wsTransport.serialDebugSessionClear();
  }

  async appendSysLine(tsMs: number, text: string): Promise<void> {
    await wsTransport.serialDebugAppendSysLine(tsMs, text);
  }

  async addFilter(
    keyword: string,
    useRegex: boolean,
    color: string,
  ): Promise<SerialDebugFilterUpdatePayload> {
    return await wsTransport.serialDebugFilterAdd(keyword, useRegex, color);
  }

  async removeFilter(filterId: string): Promise<void> {
    await wsTransport.serialDebugFilterRemove(filterId);
  }

  async readFilterMatches(
    filterId: string,
    start: number,
    limit: number,
  ): Promise<SerialDebugFilterPage> {
    return await wsTransport.serialDebugFilterReadMatches(
      filterId,
      start,
      limit,
    );
  }

  async readSessionPage(
    start: number,
    limit: number,
  ): Promise<SerialDebugSessionPage> {
    return await wsTransport.serialDebugSessionReadPage(start, limit);
  }

  async setArchiveLimit(maxBytes: number): Promise<void> {
    await wsTransport.serialDebugSetArchiveLimit(maxBytes);
  }

  onChunk(cb: ChunkListener): () => void {
    this.chunkListeners.add(cb);
    return () => {
      this.chunkListeners.delete(cb);
    };
  }

  onChunkBatch(cb: ChunkBatchListener): () => void {
    this.chunkBatchListeners.add(cb);
    return () => {
      this.chunkBatchListeners.delete(cb);
    };
  }

  onDisconnect(cb: DisconnectListener): () => void {
    this.disconnectListeners.add(cb);
    return () => {
      this.disconnectListeners.delete(cb);
    };
  }

  onFilterUpdated(cb: FilterListener): () => void {
    this.filterListeners.add(cb);
    return () => {
      this.filterListeners.delete(cb);
    };
  }

  onArchiveCapped(cb: ArchiveCappedListener): () => void {
    this.archiveCappedListeners.add(cb);
    return () => {
      this.archiveCappedListeners.delete(cb);
    };
  }
}

let singleton: SerialDebugTransport | null = null;
export function serialDebugTransport(): SerialDebugTransport {
  if (!singleton) {
    singleton = isTauriRuntime() ? new TauriTransport() : new WebTransport();
  }
  return singleton;
}

// Test helper — tests can inject a fake.
export function __setSerialDebugTransportForTest(
  t: SerialDebugTransport | null,
) {
  singleton = t;
}
