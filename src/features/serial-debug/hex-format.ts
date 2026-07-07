/**
 * Format a Uint8Array as a classic `hex | ascii` dump:
 *   41 42 43 44  | ABCD
 * Each row shows `bytesPerRow` bytes; non-printable (<0x20 or >=0x7f) become '.'.
 */
export function formatHexDump(
  bytes: Uint8Array,
  bytesPerRow: 8 | 16 | 32,
): string {
  return formatHexDumpFromChunks([bytes], bytesPerRow);
}

function formatRow(
  row: Uint8Array,
  rowLength: number,
  bytesPerRow: 8 | 16 | 32,
): string {
  const hexCells: string[] = [];
  for (let j = 0; j < bytesPerRow; j++) {
    if (j < rowLength) {
      hexCells.push(row[j].toString(16).padStart(2, "0"));
    } else {
      hexCells.push("  ");
    }
  }
  const hexPart = hexCells.join(" ");
  let ascii = "";
  for (let j = 0; j < rowLength; j++) {
    const b = row[j];
    ascii += b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : ".";
  }
  return `${hexPart} | ${ascii}`;
}

export function formatHexDumpFromChunks(
  chunks: readonly Uint8Array[],
  bytesPerRow: 8 | 16 | 32,
): string {
  let totalLength = 0;
  for (const chunk of chunks) totalLength += chunk.length;
  if (totalLength === 0) return "";

  const lines: string[] = [];
  const row = new Uint8Array(bytesPerRow);
  let rowLength = 0;

  for (const chunk of chunks) {
    for (let i = 0; i < chunk.length; i += 1) {
      row[rowLength] = chunk[i];
      rowLength += 1;
      if (rowLength === bytesPerRow) {
        lines.push(formatRow(row, rowLength, bytesPerRow));
        rowLength = 0;
      }
    }
  }

  if (rowLength > 0) {
    lines.push(formatRow(row, rowLength, bytesPerRow));
  }
  return lines.join("\n");
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
  const hexOnly = input.replace(/[^0-9a-fA-F]/g, "");
  const fullLen = hexOnly.length - (hexOnly.length % 2);
  const ignoredCount = hexOnly.length - fullLen;
  const out = new Uint8Array(fullLen / 2);
  for (let i = 0; i < fullLen; i += 2) {
    out[i / 2] = parseInt(hexOnly.substring(i, i + 2), 16);
  }
  return { bytes: out, ignoredCount };
}
