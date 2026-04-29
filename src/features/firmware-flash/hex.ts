/**
 * Pure helpers for hex address parsing and range checks (no UI / i18n).
 */

export function parseHexAddr(s: string): bigint | null {
  const t = s.trim();
  if (!t) {
    return null;
  }
  const raw = t.startsWith('0x') || t.startsWith('0X') ? t.slice(2) : t;
  if (!/^[0-9a-fA-F]+$/.test(raw)) {
    return null;
  }
  try {
    return BigInt(`0x${raw}`);
  } catch {
    return null;
  }
}

/** `null` means valid range; otherwise reason for rejection (caller maps to i18n). */
export type AddrRangeError = 'invalid' | 'startAfterEnd';

export function validateAddrRange(start: string, end: string): AddrRangeError | null {
  const a = parseHexAddr(start);
  const b = parseHexAddr(end);
  if (a === null || b === null) {
    return 'invalid';
  }
  if (a > b) {
    return 'startAfterEnd';
  }
  return null;
}

export function formatAddrHex(s: string): string {
  const t = s.trim();
  if (!t) {
    return t;
  }
  return t.startsWith('0x') || t.startsWith('0X') ? t : `0x${t}`;
}

/** 4 KiB — ESP `espflash` and Beken `ops::erase` both use this sector size for alignment. */
export const FLASH_SECTOR_4K = 0x1000;

/**
 * Aligns half-open erase range `[start, end)` to 4 KiB boundaries (floor start, ceil end).
 * Matches `tyutool-core` ESP and Beken erase plugins.
 */
export function alignedExclusiveEraseRange4K(
  startExclusive: bigint,
  endExclusive: bigint
): { alignedStart: bigint; alignedEndExclusive: bigint } {
  const sector = BigInt(FLASH_SECTOR_4K);
  const mask = sector - 1n;
  const alignedStart = startExclusive & ~mask;
  const alignedEndExclusive = (endExclusive + mask) & ~mask;
  return { alignedStart, alignedEndExclusive };
}

export function exclusiveEraseRangeNeeds4KAlignment(startExclusive: bigint, endExclusive: bigint): boolean {
  const { alignedStart, alignedEndExclusive } = alignedExclusiveEraseRange4K(startExclusive, endExclusive);
  return alignedStart !== startExclusive || alignedEndExclusive !== endExclusive;
}

/** Format a non-negative address for UI / job payload (fixed minimum width, uppercase hex). */
export function formatBigIntAddrHex(value: bigint, minHexDigits = 8): string {
  if (value < 0n) {
    throw new RangeError('formatBigIntAddrHex: negative address');
  }
  let h = value.toString(16).toUpperCase();
  if (h.length < minHexDigits) {
    h = h.padStart(minHexDigits, '0');
  }
  return `0x${h}`;
}
