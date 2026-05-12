// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { __setSerialDebugTransportForTest, type SerialDebugTransport } from '@/features/serial-debug/transport';
import type { DebugChunk } from '@/features/serial-debug/types';
import { MAX_SUB_WINDOW_LINES } from '@/features/serial-debug/constants';
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

  it('each line owns an independent rawBytes copy', () => {
    const s = useSerialDebugStore();
    s.appendChunk({ direction: 'rx', tsMs: 1000, bytes: [...Buffer.from('ab\ncd\n')] });
    expect(s.lines.length).toBe(2);
    expect(s.lines[0].rawBytes).not.toBe(s.lines[1].rawBytes);
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

  it('auto-resumes when the released port becomes free again', async () => {
    const { usePortManagerStore } = await import('@/stores/port-manager');
    const pm = usePortManagerStore();
    const s = useSerialDebugStore();
    s.port = '/dev/ttyUSB0';
    s.autoRelease = true;
    await s.openPort();
    expect(s.open).toBe(true);

    // Simulate flash preempting the port — port-manager will call serial-debug's
    // onReleaseRequest (returns true because autoRelease) then onReleased('requested').
    await pm.acquire({
      id: 'flash',
      port: '/dev/ttyUSB0',
      onReleaseRequest: async () => false,
      onReleased: () => {},
    });
    // After acquire, the flash now owns; allow microtasks for the release side-effects.
    await new Promise((r) => setTimeout(r, 0));
    expect(s.open).toBe(false);
    expect(s.pendingResume).toBe(true);

    // Flash finishes: releases the port. Our watcher should trigger a re-open.
    pm.release('/dev/ttyUSB0', 'flash');
    // Wait a couple ticks for the watcher + async openPort chain.
    await new Promise((r) => setTimeout(r, 50));
    expect(s.open).toBe(true);
    expect(s.pendingResume).toBe(false);
  });
});

describe('useSerialDebugStore sub-window management', () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  function chunk(text: string): DebugChunk {
    return { direction: 'rx', tsMs: 1000, bytes: [...Buffer.from(text + '\n')] };
  }

  it('fan-out: matching line appears in sub-window', () => {
    const s = useSerialDebugStore();
    s.addSubWindow('ERROR', false);
    s.appendChunk(chunk('ERROR: timeout'));
    expect(s.subWindows[0].lines.length).toBe(1);
    expect(s.subWindows[0].lines[0].text).toBe('ERROR: timeout');
  });

  it('fan-out: non-matching line does not appear in sub-window', () => {
    const s = useSerialDebugStore();
    s.addSubWindow('ERROR', false);
    s.appendChunk(chunk('INFO: ok'));
    expect(s.subWindows[0].lines.length).toBe(0);
  });

  it('addSubWindow returns duplicate when same name added twice', () => {
    const s = useSerialDebugStore();
    expect(s.addSubWindow('WIFI', false)).toBe('ok');
    expect(s.addSubWindow('WIFI', false)).toBe('duplicate');
    expect(s.subWindows.length).toBe(1);
  });

  it('addSubWindow returns invalid-regex for bad regex pattern', () => {
    const s = useSerialDebugStore();
    expect(s.addSubWindow('[invalid', true)).toBe('invalid-regex');
    expect(s.subWindows.length).toBe(0);
  });

  it('removeSubWindow removes by id and future chunks do not fan-out to it', () => {
    const s = useSerialDebugStore();
    s.addSubWindow('FIRST', false);
    s.addSubWindow('SECOND', false);
    const firstId = s.subWindows[0].id;
    s.removeSubWindow(firstId);
    expect(s.subWindows.length).toBe(1);
    expect(s.subWindows[0].name).toBe('SECOND');
    // chunks matching FIRST should not fan-out to the remaining sub-window
    s.appendChunk(chunk('FIRST data'));
    expect(s.subWindows[0].lines.length).toBe(0);
  });

  it('clear() empties sub-window lines but keeps the sub-window', () => {
    const s = useSerialDebugStore();
    s.addSubWindow('LOG', false);
    s.appendChunk(chunk('LOG entry'));
    expect(s.subWindows[0].lines.length).toBe(1);
    s.clear();
    expect(s.subWindows.length).toBe(1);
    expect(s.subWindows[0].lines.length).toBe(0);
  });

  it('FIFO cap: sub-window lines are capped at MAX_SUB_WINDOW_LINES', () => {
    const s = useSerialDebugStore();
    s.addSubWindow('X', false);
    const oneLine = 'X\n';
    const bytes = [...Buffer.from(oneLine.repeat(MAX_SUB_WINDOW_LINES + 5))];
    s.appendChunk({ direction: 'rx', tsMs: 1000, bytes });
    expect(s.subWindows[0].lines.length).toBe(MAX_SUB_WINDOW_LINES);
  });

  it('sys lines go to all sub-windows regardless of filter', () => {
    const s = useSerialDebugStore();
    s.addSubWindow('WIFI', false);
    s.appendSysLine('Connected to /dev/ttyUSB0');
    expect(s.subWindows[0].lines.length).toBe(1);
    expect(s.subWindows[0].lines[0].direction).toBe('sys');
  });
});
