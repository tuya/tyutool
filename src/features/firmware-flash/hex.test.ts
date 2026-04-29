import { describe, expect, it } from 'vitest';
import {
  alignedExclusiveEraseRange4K,
  exclusiveEraseRangeNeeds4KAlignment,
  formatAddrHex,
  formatBigIntAddrHex,
  parseHexAddr,
  validateAddrRange,
} from './hex';

describe('parseHexAddr', () => {
  it('parses 0x-prefixed hex', () => {
    expect(parseHexAddr('0x10')).toBe(16n);
    expect(parseHexAddr('0xFF')).toBe(255n);
  });

  it('parses unprefixed hex', () => {
    expect(parseHexAddr('1000')).toBe(4096n);
  });

  it('accepts uppercase prefix', () => {
    expect(parseHexAddr('0Xa')).toBe(10n);
  });

  it('returns null for empty or invalid', () => {
    expect(parseHexAddr('')).toBeNull();
    expect(parseHexAddr('   ')).toBeNull();
    expect(parseHexAddr('0xGG')).toBeNull();
    expect(parseHexAddr('xyz')).toBeNull();
  });
});

describe('validateAddrRange', () => {
  it('returns null when valid', () => {
    expect(validateAddrRange('0x0', '0x1000')).toBeNull();
    expect(validateAddrRange('100', '200')).toBeNull();
  });

  it('rejects invalid hex', () => {
    expect(validateAddrRange('0x', '0x10')).toBe('invalid');
    expect(validateAddrRange('0x10', '0xZZ')).toBe('invalid');
  });

  it('rejects start after end', () => {
    expect(validateAddrRange('0x2000', '0x1000')).toBe('startAfterEnd');
  });
});

describe('formatAddrHex', () => {
  it('adds 0x when missing', () => {
    expect(formatAddrHex('dead')).toBe('0xdead');
  });

  it('preserves existing prefix', () => {
    expect(formatAddrHex('0xAB')).toBe('0xAB');
  });

  it('returns empty trim as-is', () => {
    expect(formatAddrHex('')).toBe('');
    expect(formatAddrHex('   ')).toBe('');
  });
});

describe('alignedExclusiveEraseRange4K', () => {
  it('leaves already-aligned ranges unchanged', () => {
    const r = alignedExclusiveEraseRange4K(0n, 0x1000n);
    expect(r.alignedStart).toBe(0n);
    expect(r.alignedEndExclusive).toBe(0x1000n);
    expect(exclusiveEraseRangeNeeds4KAlignment(0n, 0x1000n)).toBe(false);
  });

  it('expands misaligned exclusive end (0x129E80 case)', () => {
    const r = alignedExclusiveEraseRange4K(0n, 0x129e80n);
    expect(r.alignedStart).toBe(0n);
    expect(r.alignedEndExclusive).toBe(0x12a000n);
    expect(exclusiveEraseRangeNeeds4KAlignment(0n, 0x129e80n)).toBe(true);
  });

  it('floors misaligned start', () => {
    const r = alignedExclusiveEraseRange4K(0x1004n, 0x3000n);
    expect(r.alignedStart).toBe(0x1000n);
    expect(r.alignedEndExclusive).toBe(0x3000n);
  });
});

describe('formatBigIntAddrHex', () => {
  it('pads to minimum width', () => {
    expect(formatBigIntAddrHex(0n)).toBe('0x00000000');
    expect(formatBigIntAddrHex(0x12a000n)).toBe('0x0012A000');
  });
});
