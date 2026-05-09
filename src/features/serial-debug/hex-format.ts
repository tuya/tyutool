/**
 * Format a Uint8Array as a classic `hex | ascii` dump:
 *   41 42 43 44  | ABCD
 * Each row shows `bytesPerRow` bytes; non-printable (<0x20 or >=0x7f) become '.'.
 */
export function formatHexDump(bytes: Uint8Array, bytesPerRow: 8 | 16 | 32): string {
  if (bytes.length === 0) return '';
  const lines: string[] = [];
  for (let i = 0; i < bytes.length; i += bytesPerRow) {
    const row = bytes.slice(i, i + bytesPerRow);
    const hexCells: string[] = [];
    for (let j = 0; j < bytesPerRow; j++) {
      if (j < row.length) {
        hexCells.push(row[j].toString(16).padStart(2, '0'));
      } else {
        hexCells.push('  ');
      }
    }
    const hexPart = hexCells.join(' ');
    let ascii = '';
    for (let j = 0; j < row.length; j++) {
      const b = row[j];
      ascii += b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '.';
    }
    lines.push(`${hexPart} | ${ascii}`);
  }
  return lines.join('\n');
}

export interface ParseHexResult {
  bytes: Uint8Array;
  ignoredCount: number; // number of unmatched hex chars dropped at the end
}

/**
 * Lenient hex parser — accepts whitespace, punctuation, mixed case.
 * Returns bytes successfully parsed; an odd trailing hex char is dropped
 * and counted in `ignoredCount`.
 */
export function parseHexInput(input: string): ParseHexResult {
  const hexOnly = input.replace(/[^0-9a-fA-F]/g, '');
  const fullLen = hexOnly.length - (hexOnly.length % 2);
  const ignoredCount = hexOnly.length - fullLen;
  const out = new Uint8Array(fullLen / 2);
  for (let i = 0; i < fullLen; i += 2) {
    out[i / 2] = parseInt(hexOnly.substring(i, i + 2), 16);
  }
  return { bytes: out, ignoredCount };
}
