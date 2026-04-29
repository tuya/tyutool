import { describe, expect, it } from 'vitest';
import { addTimestampSuffix, formatDuration } from './utils';

describe('formatDuration', () => {
  it('formats 0ms', () => {
    expect(formatDuration(0)).toBe('0.0s');
  });

  it('formats sub-second durations', () => {
    expect(formatDuration(500)).toBe('0.5s');
    expect(formatDuration(1234)).toBe('1.2s');
  });

  it('formats seconds below 60', () => {
    expect(formatDuration(5000)).toBe('5.0s');
    expect(formatDuration(59900)).toBe('59.9s');
  });

  it('formats exactly 60 seconds as minutes', () => {
    expect(formatDuration(60000)).toBe('1m 0s');
  });

  it('formats minutes and seconds', () => {
    expect(formatDuration(125000)).toBe('2m 5s');
    expect(formatDuration(3599000)).toBe('59m 59s');
  });

  it('formats exactly 1 hour', () => {
    expect(formatDuration(3600000)).toBe('1h 0m 0s');
  });

  it('formats hours, minutes, seconds', () => {
    expect(formatDuration(3661000)).toBe('1h 1m 1s');
    expect(formatDuration(7384000)).toBe('2h 3m 4s');
  });
});

describe('addTimestampSuffix', () => {
  const fixedDate = new Date(2026, 3, 10, 14, 30, 25); // 2026-04-10 14:30:25

  it('inserts timestamp before extension', () => {
    expect(addTimestampSuffix('firmware.bin', fixedDate)).toBe('firmware_20260410_143025.bin');
  });

  it('appends timestamp when no extension', () => {
    expect(addTimestampSuffix('firmware', fixedDate)).toBe('firmware_20260410_143025');
  });

  it('handles multiple dots — inserts before last dot', () => {
    expect(addTimestampSuffix('my.firmware.v2.bin', fixedDate)).toBe('my.firmware.v2_20260410_143025.bin');
  });

  it('handles dot at start of filename', () => {
    expect(addTimestampSuffix('.hidden', fixedDate)).toBe('_20260410_143025.hidden');
  });

  it('pads single-digit month/day/hour/minute/second', () => {
    const jan = new Date(2026, 0, 5, 3, 7, 9); // 2026-01-05 03:07:09
    expect(addTimestampSuffix('f.bin', jan)).toBe('f_20260105_030709.bin');
  });
});
