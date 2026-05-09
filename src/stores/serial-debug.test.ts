// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { __setSerialDebugTransportForTest, type SerialDebugTransport } from '@/features/serial-debug/transport';
import type { DebugChunk } from '@/features/serial-debug/types';
import { useSerialDebugStore } from './serial-debug';

function fakeTransport(): SerialDebugTransport & {
  emitChunk: (c: DebugChunk) => void;
  emitDisconnect: (reason: string) => void;
  sent: Uint8Array[];
  opened: boolean;
} {
  const chunkListeners = new Set<(c: DebugChunk) => void>();
  const discListeners = new Set<(p: { reason: string }) => void>();
  const sent: Uint8Array[] = [];
  let opened = false;
  return {
    sent,
    get opened() { return opened; },
    async open() { opened = true; },
    async close() { opened = false; },
    async send(b) { sent.push(b); },
    onChunk(cb) { chunkListeners.add(cb); return () => chunkListeners.delete(cb); },
    onDisconnect(cb) { discListeners.add(cb); return () => discListeners.delete(cb); },
    async openFilterWindow() { return 'native'; },
    emitChunk(c) { chunkListeners.forEach((l) => l(c)); },
    emitDisconnect(reason) { discListeners.forEach((l) => l({ reason })); },
  };
}

describe('useSerialDebugStore.appendChunk', () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it('splits bytes by \\n into lines with direction and timestamp', async () => {
    const s = useSerialDebugStore();
    s.appendChunk({ direction: 'rx', tsMs: 1000, bytes: [...Buffer.from('hi\nworld\n')] });
    expect(s.lines.length).toBe(2);
    expect(s.lines[0]).toMatchObject({ direction: 'rx', tsMs: 1000, text: 'hi' });
    expect(s.lines[1]).toMatchObject({ direction: 'rx', text: 'world' });
  });

  it('buffers trailing partial line until the next newline', () => {
    const s = useSerialDebugStore();
    s.appendChunk({ direction: 'rx', tsMs: 1000, bytes: [...Buffer.from('hel')] });
    expect(s.lines.length).toBe(0);
    s.appendChunk({ direction: 'rx', tsMs: 1001, bytes: [...Buffer.from('lo\n')] });
    expect(s.lines.length).toBe(1);
    expect(s.lines[0].text).toBe('hello');
  });

  it('drops oldest entries past MAX_LOG_LINES', () => {
    const s = useSerialDebugStore();
    // Simulate 20005 lines arriving in one batch (each terminated by \n).
    const oneLine = 'x\n';
    const bytes = [...Buffer.from(oneLine.repeat(20005))];
    s.appendChunk({ direction: 'rx', tsMs: 1000, bytes });
    expect(s.lines.length).toBe(20000);
  });
});

describe('useSerialDebugStore.send', () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it('encodes ASCII and appends \\r\\n when sendAppendCrlf is true', async () => {
    const s = useSerialDebugStore();
    (s as unknown as { open: boolean }).open = true;
    s.sendMode = 'ascii';
    s.sendAppendCrlf = true;
    s.sendInput = 'AT';
    await s.send();
    expect(fake.sent.length).toBe(1);
    expect(Array.from(fake.sent[0])).toEqual([0x41, 0x54, 0x0d, 0x0a]);
  });

  it('parses Hex and ignores non-hex characters', async () => {
    const s = useSerialDebugStore();
    (s as unknown as { open: boolean }).open = true;
    s.sendMode = 'hex';
    s.sendAppendCrlf = false;
    s.sendInput = 'AA,BB;CC';
    await s.send();
    expect(Array.from(fake.sent[0])).toEqual([0xaa, 0xbb, 0xcc]);
  });

  it('keeps send history, trimmed to MAX_SEND_HISTORY and deduped', async () => {
    const s = useSerialDebugStore();
    (s as unknown as { open: boolean }).open = true;
    s.sendMode = 'ascii';
    s.sendAppendCrlf = false;
    s.sendInput = 'A'; await s.send();
    s.sendInput = 'B'; await s.send();
    s.sendInput = 'A'; await s.send(); // duplicate should move to front, not add a new entry
    expect(s.sendHistory).toEqual(['A', 'B']);
  });
});

describe('useSerialDebugStore port-manager integration', () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it('openPort acquires the port before calling transport.open', async () => {
    const s = useSerialDebugStore();
    s.port = '/dev/ttyUSB0';
    s.baudRate = 115200;
    await s.openPort();
    expect(fake.opened).toBe(true);
    expect(s.open).toBe(true);
  });

  it('when port-manager denies, openPort does not call transport.open', async () => {
    const { usePortManagerStore } = await import('@/stores/port-manager');
    const pm = usePortManagerStore();
    // Pre-occupy by a different owner that refuses to release.
    await pm.acquire({
      id: 'flash',
      port: '/dev/ttyUSB0',
      onReleaseRequest: async () => false,
      onReleased: () => {},
    });
    const s = useSerialDebugStore();
    s.port = '/dev/ttyUSB0';
    await s.openPort();
    expect(fake.opened).toBe(false);
    expect(s.open).toBe(false);
  });

  it('clear() empties lines and pending buffer', () => {
    const s = useSerialDebugStore();
    s.appendChunk({ direction: 'rx', tsMs: 1000, bytes: [...Buffer.from('ab\ncd')] });
    expect(s.lines.length).toBe(1);
    s.clear();
    expect(s.lines.length).toBe(0);
    // new input after clear should start a fresh line
    s.appendChunk({ direction: 'rx', tsMs: 2000, bytes: [...Buffer.from('xy\n')] });
    expect(s.lines[0].text).toBe('xy');
  });
});
