import { formatHexDumpFromChunks } from "./hex-format";
import type { DebugLogLine, HexBytesPerRow } from "./types";

const textEncoder = new TextEncoder();

function lineChunk(line: DebugLogLine): Uint8Array {
  // Live lines carry rawBytes (the store splits DebugChunk.bytes itself).
  // Lines paged back from the Rust session archive do not: the archive stores
  // text only, so a `number[]` of the same bytes does not cost 3x the IPC
  // payload. Re-encoding text is byte-identical for any valid UTF-8 log, which
  // is all the archive can round-trip anyway (it decodes lossily on write).
  const raw = line.rawBytes ?? textEncoder.encode(line.text);
  const chunk = new Uint8Array(raw.length + 1);
  chunk.set(raw, 0);
  chunk[chunk.length - 1] = 0x0a;
  return chunk;
}

/**
 * Windowed renderer for the hex view.
 *
 * The hex view is one continuous dump of the whole visible buffer: every line's
 * bytes plus a trailing 0x0a are concatenated into a single byte stream, and hex
 * row k shows bytes `[k * bytesPerRow, (k + 1) * bytesPerRow)` of it. Rows
 * therefore do not line up with log lines — one row can straddle several lines
 * and a long line spans many rows — so the ASCII view's `lineIndex * rowHeight`
 * row model does not carry over. What replaces it is a byte-offset index:
 *
 * - `starts` is a prefix sum of chunk lengths over the *current* buffer, so
 *   `starts[i]` is line i's byte offset and `starts[n]` is the total byte count.
 *   All offsets are relative to the current head, which is exactly what the
 *   rendered dump is relative to. When the store drops lines off the head the
 *   whole stream shifts; rebuilding the prefix sum re-bases every offset in one
 *   numeric pass, so no absolute-offset bookkeeping (and no per-line id → offset
 *   map that would have to be rebased anyway) is needed.
 * - The rebuild only runs when the visible line list actually changes; a pure
 *   scroll reuses the index.
 * - `bytesPerRow` is deliberately not part of the index: it only divides the
 *   byte axis into rows, so switching it re-derives the row count and the window
 *   without touching the index.
 *
 * Byte chunks are cached per line id (`Uint8Array` views are handed to the
 * formatter with `subarray`, so windowing copies no bytes).
 */
export class SerialDebugHexViewRenderer {
  private chunkByLineId = new Map<number, Uint8Array>();
  private ids: number[] = [];
  // Length is `ids.length + 1`; starts[0] is always 0.
  private starts: number[] = [0];

  /** Total number of hex rows the current buffer occupies. */
  rowCount(
    lines: readonly DebugLogLine[],
    bytesPerRow: HexBytesPerRow,
  ): number {
    this.syncIndex(lines);
    return Math.ceil(this.totalBytes() / bytesPerRow);
  }

  /**
   * Dump text for hex rows `[startRow, endRow)`. Byte-identical to the matching
   * slice of the full-buffer dump: `startRow * bytesPerRow` is row-aligned by
   * construction, and the trailing partial row can only appear when the window
   * reaches the end of the stream.
   */
  renderRows(
    lines: readonly DebugLogLine[],
    bytesPerRow: HexBytesPerRow,
    startRow: number,
    endRow: number,
  ): string {
    this.syncIndex(lines);
    const from = Math.max(0, startRow) * bytesPerRow;
    const to = Math.min(endRow * bytesPerRow, this.totalBytes());
    if (from >= to) return "";
    return formatHexDumpFromChunks(this.sliceChunks(from, to), bytesPerRow);
  }

  /**
   * Hex row that line `lineIndex` of the current buffer starts on — the bridge
   * the search / scroll-to-match path needs to turn a line index into a scroll
   * offset while the hex view is up.
   */
  rowOfLine(
    lines: readonly DebugLogLine[],
    bytesPerRow: HexBytesPerRow,
    lineIndex: number,
  ): number {
    this.syncIndex(lines);
    if (lineIndex <= 0) return 0;
    const clamped = Math.min(lineIndex, this.ids.length);
    return Math.floor(this.starts[clamped] / bytesPerRow);
  }

  cacheSize(): number {
    return this.chunkByLineId.size;
  }

  private totalBytes(): number {
    return this.starts[this.ids.length];
  }

  /** Byte views covering `[from, to)` of the current stream. */
  private sliceChunks(from: number, to: number): Uint8Array[] {
    const out: Uint8Array[] = [];
    for (let i = this.lineAt(from); i < this.ids.length; i += 1) {
      const start = this.starts[i];
      if (start >= to) break;
      const chunk = this.chunkByLineId.get(this.ids[i]);
      if (!chunk) continue;
      const head = Math.max(from - start, 0);
      const tail = Math.min(to - start, chunk.length);
      out.push(
        head === 0 && tail === chunk.length
          ? chunk
          : chunk.subarray(head, tail),
      );
    }
    return out;
  }

  /** First line index whose chunk contains `byteOffset`. */
  private lineAt(byteOffset: number): number {
    let lo = 0;
    let hi = this.ids.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.starts[mid + 1] > byteOffset) hi = mid;
      else lo = mid + 1;
    }
    return Math.max(lo, 0);
  }

  private syncIndex(lines: readonly DebugLogLine[]): void {
    const n = lines.length;
    if (n === this.ids.length) {
      let i = 0;
      while (i < n && lines[i].id === this.ids[i]) i += 1;
      if (i === n) return;
    }

    // Reused in place: at 20000 lines a fresh pair of arrays per publish would
    // be ~320 KB of garbage per flush for no benefit.
    const { ids, starts } = this;
    ids.length = n;
    starts.length = n + 1;
    for (let i = 0; i < n; i += 1) {
      const line = lines[i];
      let chunk = this.chunkByLineId.get(line.id);
      if (chunk === undefined) {
        chunk = lineChunk(line);
        this.chunkByLineId.set(line.id, chunk);
      }
      ids[i] = line.id;
      starts[i + 1] = starts[i] + chunk.length;
    }

    // Every visible id is in the cache by now, so a cache larger than the
    // buffer is the only case that can hold stale entries. Sweeping is O(cache)
    // and a saturated buffer strands `logWindowLines / 1000`-ish entries per
    // flush, so tolerate a fifth of slack and sweep once per few hundred
    // flushes instead of on every one: it is the dominant cost of a publish
    // otherwise, and the cache stays bounded either way.
    if (this.chunkByLineId.size > n * 1.2) {
      const visible = new Set(ids);
      for (const existingId of this.chunkByLineId.keys()) {
        if (!visible.has(existingId)) this.chunkByLineId.delete(existingId);
      }
    }
  }
}
