/**
 * Generate a square PNG for `pnpm exec tauri icon` (no third-party deps).
 * Output: scripts/icon-source.png (1024×1024, RGB).
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { crc32, deflateSync } from 'node:zlib';

function chunk(chunkType: Buffer, data: Buffer): Buffer {
  const combined = Buffer.concat([chunkType, data]);
  const crc = crc32(combined) >>> 0;
  const len = Buffer.allocUnsafe(4);
  len.writeUInt32BE(data.length, 0);
  const crcBuf = Buffer.allocUnsafe(4);
  crcBuf.writeUInt32BE(crc, 0);
  return Buffer.concat([len, chunkType, data, crcBuf]);
}

function writeRgbPng(
  path: string,
  width: number,
  height: number,
  pixelRgb: [number, number, number],
): void {
  const [r, g, b] = pixelRgb;
  const rowLen = 1 + width * 3;
  const row = Buffer.allocUnsafe(rowLen);
  row[0] = 0;
  for (let x = 0; x < width; x++) {
    const o = 1 + x * 3;
    row[o] = r;
    row[o + 1] = g;
    row[o + 2] = b;
  }
  const raw = Buffer.allocUnsafe(rowLen * height);
  for (let y = 0; y < height; y++) {
    row.copy(raw, y * rowLen);
  }
  const compressed = deflateSync(raw, { level: 9 });
  const ihdr = Buffer.allocUnsafe(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const png = Buffer.concat([
    sig,
    chunk(Buffer.from('IHDR'), ihdr),
    chunk(Buffer.from('IDAT'), compressed),
    chunk(Buffer.from('IEND'), Buffer.alloc(0)),
  ]);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, png);
}

const scriptDir = dirname(fileURLToPath(import.meta.url));
const out = join(scriptDir, 'icon-source.png');
writeRgbPng(out, 1024, 1024, [37, 99, 235]);
console.log(`Wrote ${out}`);
