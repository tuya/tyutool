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

describe('useSerialDebugStore watch chip management', () => {
  let fake: ReturnType<typeof fakeTransport>;
  beforeEach(() => {
    setActivePinia(createPinia());
    fake = fakeTransport();
    __setSerialDebugTransportForTest(fake);
  });
  afterEach(() => __setSerialDebugTransportForTest(null));

  it('addChip returns ok and adds chip in highlight mode', () => {
    const s = useSerialDebugStore();
    const result = s.addChip('ERROR', false);
    expect(result).toBe('ok');
    expect(s.watchChips.length).toBe(1);
    expect(s.watchChips[0]).toMatchObject({ keyword: 'ERROR', useRegex: false, mode: 'highlight' });
  });

  it('addChip returns duplicate when same keyword added twice', () => {
    const s = useSerialDebugStore();
    expect(s.addChip('WIFI', false)).toBe('ok');
    expect(s.addChip('WIFI', false)).toBe('duplicate');
    expect(s.watchChips.length).toBe(1);
  });

  it('addChip returns invalid-regex for bad pattern', () => {
    const s = useSerialDebugStore();
    expect(s.addChip('[invalid', true)).toBe('invalid-regex');
    expect(s.watchChips.length).toBe(0);
  });

  it('removeChip removes by id', () => {
    const s = useSerialDebugStore();
    s.addChip('FIRST', false);
    s.addChip('SECOND', false);
    const firstId = s.watchChips[0].id;
    s.removeChip(firstId);
    expect(s.watchChips.length).toBe(1);
    expect(s.watchChips[0].keyword).toBe('SECOND');
  });

  it('cycleChipMode goes highlight → filter → off → highlight', () => {
    const s = useSerialDebugStore();
    s.addChip('LOG', false);
    const id = s.watchChips[0].id;
    expect(s.watchChips[0].mode).toBe('highlight');
    s.cycleChipMode(id);
    expect(s.watchChips[0].mode).toBe('filter');
    s.cycleChipMode(id);
    expect(s.watchChips[0].mode).toBe('off');
    s.cycleChipMode(id);
    expect(s.watchChips[0].mode).toBe('highlight');
  });

  it('matchChipKeyword matches plain text substring (case-sensitive)', () => {
    const s = useSerialDebugStore();
    s.addChip('ERROR', false);
    const chip = s.watchChips[0];
    expect(s.matchChipKeyword({ id: 1, tsMs: 0, direction: 'rx', text: 'ERROR: timeout' }, chip)).toBe(true);
    expect(s.matchChipKeyword({ id: 2, tsMs: 0, direction: 'rx', text: 'error: timeout' }, chip)).toBe(false);
    expect(s.matchChipKeyword({ id: 3, tsMs: 0, direction: 'rx', text: 'no match' }, chip)).toBe(false);
  });

  it('matchChipKeyword matches regex pattern', () => {
    const s = useSerialDebugStore();
    s.addChip('err(or)?', true);
    const chip = s.watchChips[0];
    expect(s.matchChipKeyword({ id: 1, tsMs: 0, direction: 'rx', text: 'err: foo' }, chip)).toBe(true);
    expect(s.matchChipKeyword({ id: 2, tsMs: 0, direction: 'rx', text: 'error: bar' }, chip)).toBe(true);
    expect(s.matchChipKeyword({ id: 3, tsMs: 0, direction: 'rx', text: 'ok' }, chip)).toBe(false);
  });

  it('clear does not affect watchChips', () => {
    const s = useSerialDebugStore();
    s.addChip('LOG', false);
    s.appendChunk({ direction: 'rx', tsMs: 1000, bytes: [...Buffer.from('LOG line\n')] });
    s.clear();
    expect(s.watchChips.length).toBe(1);
    expect(s.lines.length).toBe(0);
  });

  it('chips cycle colors from CHIP_COLORS when added', async () => {
    const { CHIP_COLORS } = await import('@/features/serial-debug/constants');
    const s = useSerialDebugStore();
    for (let i = 0; i < CHIP_COLORS.length + 1; i++) {
      s.addChip(`kw${i}`, false);
    }
    expect(s.watchChips[0].color).toBe(CHIP_COLORS[0]);
    expect(s.watchChips[CHIP_COLORS.length].color).toBe(CHIP_COLORS[0]);
  });
});
