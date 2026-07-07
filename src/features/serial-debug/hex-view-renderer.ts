import { formatHexDumpFromChunks } from "./hex-format";
import type { DebugLogLine, HexBytesPerRow } from "./types";

function lineChunk(line: DebugLogLine): Uint8Array {
  const raw = line.rawBytes ?? new Uint8Array();
  const chunk = new Uint8Array(raw.length + 1);
  chunk.set(raw, 0);
  chunk[chunk.length - 1] = 0x0a;
  return chunk;
}

export class SerialDebugHexViewRenderer {
  private chunkByLineId = new Map<number, Uint8Array>();
  private cachedLineIds: number[] = [];
  private cachedBytesPerRow: HexBytesPerRow | null = null;
  private cachedDump = "";

  render(lines: readonly DebugLogLine[], bytesPerRow: HexBytesPerRow): string {
    const nextIds = lines.map((line) => line.id);
    if (
      this.cachedBytesPerRow === bytesPerRow &&
      nextIds.length === this.cachedLineIds.length &&
      nextIds.every((id, index) => id === this.cachedLineIds[index])
    ) {
      return this.cachedDump;
    }

    const visibleIds = new Set(nextIds);
    for (const existingId of this.chunkByLineId.keys()) {
      if (!visibleIds.has(existingId)) {
        this.chunkByLineId.delete(existingId);
      }
    }

    const chunks = lines.map((line) => {
      const cached = this.chunkByLineId.get(line.id);
      if (cached) return cached;
      const chunk = lineChunk(line);
      this.chunkByLineId.set(line.id, chunk);
      return chunk;
    });

    this.cachedLineIds = nextIds;
    this.cachedBytesPerRow = bytesPerRow;
    this.cachedDump = formatHexDumpFromChunks(chunks, bytesPerRow);
    return this.cachedDump;
  }

  cacheSize(): number {
    return this.chunkByLineId.size;
  }
}
