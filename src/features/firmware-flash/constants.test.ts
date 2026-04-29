import { describe, expect, it } from 'vitest';
import { BAUD_RATE_OPTIONS, CHIP_IDS, DEFAULT_CHIP_ID, SERIAL_PORT_OPTIONS } from './constants';

describe('CHIP_IDS', () => {
  it('is a non-empty array', () => {
    expect(CHIP_IDS.length).toBeGreaterThan(0);
  });

  it('is sorted by ASCII string order', () => {
    const copy = [...CHIP_IDS];
    copy.sort();
    expect([...CHIP_IDS]).toEqual(copy);
  });

  it('uses t5 as default chip for first launch (not necessarily first in list)', () => {
    expect(DEFAULT_CHIP_ID).toBe('t5');
  });

  it('contains all expected chip IDs', () => {
    expect(CHIP_IDS).toContain('esp32');
    expect(CHIP_IDS).toContain('esp32c3');
    expect(CHIP_IDS).toContain('esp32c6');
    expect(CHIP_IDS).toContain('esp32s3');
    expect(CHIP_IDS).toContain('t5');
    expect(CHIP_IDS).toContain('t2');
    expect(CHIP_IDS).toContain('bk7231n');
  });
});

describe('BAUD_RATE_OPTIONS', () => {
  it('is a non-empty array', () => {
    expect(BAUD_RATE_OPTIONS.length).toBeGreaterThan(0);
  });

  it('contains standard baud rates', () => {
    expect(BAUD_RATE_OPTIONS).toContain(115200);
    expect(BAUD_RATE_OPTIONS).toContain(921600);
  });

  it('all values are positive numbers', () => {
    for (const rate of BAUD_RATE_OPTIONS) {
      expect(rate).toBeGreaterThan(0);
    }
  });

  it('values are sorted ascending', () => {
    for (let i = 1; i < BAUD_RATE_OPTIONS.length; i++) {
      expect(BAUD_RATE_OPTIONS[i]).toBeGreaterThan(BAUD_RATE_OPTIONS[i - 1]);
    }
  });
});

describe('SERIAL_PORT_OPTIONS', () => {
  it('starts as an empty array (populated at runtime)', () => {
    expect(SERIAL_PORT_OPTIONS).toEqual([]);
  });
});
