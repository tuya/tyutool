import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { wsTransport } from '@/features/firmware-flash/ws-transport';
import type { DebugChunk, DebugConfig, DisconnectPayload } from './types';

type ChunkListener = (chunk: DebugChunk) => void;
type DisconnectListener = (p: DisconnectPayload) => void;

export interface SerialDebugTransport {
  open(cfg: DebugConfig): Promise<void>;
  close(): Promise<void>;
  send(bytes: Uint8Array): Promise<void>;
  onChunk(cb: ChunkListener): () => void;      // returns unsubscribe
  onDisconnect(cb: DisconnectListener): () => void;
  openFilterWindow(): Promise<'native' | 'inline'>;
}

/** Lazy Tauri transport — uses @tauri-apps/api. Loads dynamically so web builds stay lean. */
class TauriTransport implements SerialDebugTransport {
  private chunkListeners = new Set<ChunkListener>();
  private disconnectListeners = new Set<DisconnectListener>();
  private unlistenChunk?: () => void;
  private unlistenDisconnect?: () => void;

  private async ensureListeners(): Promise<void> {
    if (this.unlistenChunk && this.unlistenDisconnect) return;
    const { listen } = await import('@tauri-apps/api/event');
    this.unlistenChunk = await listen<DebugChunk>('serial-debug-chunk', (ev) => {
      this.chunkListeners.forEach((l) => l(ev.payload));
    });
    this.unlistenDisconnect = await listen<DisconnectPayload>('serial-debug-disconnected', (ev) => {
      this.disconnectListeners.forEach((l) => l(ev.payload));
    });
  }

  async open(cfg: DebugConfig): Promise<void> {
    await this.ensureListeners();
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('serial_debug_open', { cfg });
  }

  async close(): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('serial_debug_close');
  }

  async send(bytes: Uint8Array): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('serial_debug_send', { bytes: Array.from(bytes) });
  }

  onChunk(cb: ChunkListener): () => void {
    this.chunkListeners.add(cb);
    return () => { this.chunkListeners.delete(cb); };
  }

  onDisconnect(cb: DisconnectListener): () => void {
    this.disconnectListeners.add(cb);
    return () => { this.disconnectListeners.delete(cb); };
  }

  async openFilterWindow(): Promise<'native' | 'inline'> {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('open_serial_debug_filter_window');
    return 'native';
  }
}

/** Web mode transport — talks to `tyutool-cli serve` over WebSocket via wsTransport. */
class WebTransport implements SerialDebugTransport {
  private chunkListeners = new Set<ChunkListener>();
  private disconnectListeners = new Set<DisconnectListener>();
  private isOpen = false;

  async open(cfg: DebugConfig): Promise<void> {
    await wsTransport.serialDebugOpen(
      cfg,
      (chunk) => this.chunkListeners.forEach((l) => l(chunk)),
      (reason) => this.disconnectListeners.forEach((l) => l({ reason })),
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

  onChunk(cb: ChunkListener): () => void {
    this.chunkListeners.add(cb);
    return () => {
      this.chunkListeners.delete(cb);
    };
  }

  onDisconnect(cb: DisconnectListener): () => void {
    this.disconnectListeners.add(cb);
    return () => {
      this.disconnectListeners.delete(cb);
    };
  }

  async openFilterWindow(): Promise<'native' | 'inline'> {
    return 'inline';
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
export function __setSerialDebugTransportForTest(t: SerialDebugTransport | null) {
  singleton = t;
}
