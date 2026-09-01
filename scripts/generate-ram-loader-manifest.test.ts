import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { buildManifest } from './generate-ram-loader-manifest.js';

// The scanning, naming validation, hashing and sorting are shared with the
// auth-firmware family and covered by its test; what needs proving here is that
// this family's own prefix and manifest key are wired through, since a mix-up
// would publish a manifest the Rust side cannot read.

const BASE = 'https://example.com/releases/download/ram-loader';

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'ram-loader-'));
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

function writeBin(chip: string, version: string, content: Buffer): void {
  const dir = join(root, chip);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `ram-loader-${chip}-${version}.bin`), content);
}

describe('buildManifest', () => {
  it('emits loaders[] with the ram-loader url, sha256 and size', () => {
    const content = Buffer.from('ram loader payload');
    writeBin('ln882h', '1.0.0', content);

    const manifest = buildManifest(root, BASE);

    expect(manifest.loaders).toHaveLength(1);
    expect(manifest.loaders[0]).toEqual({
      chip: 'ln882h',
      version: '1.0.0',
      url: `${BASE}/ram-loader-ln882h-1.0.0.bin`,
      sha256: createHash('sha256').update(content).digest('hex'),
      size: content.byteLength,
    });
  });

  it('reads trimmed notes from a sibling .txt (where the vendor provenance lives)', () => {
    writeBin('gd32vw553', '1.0.0', Buffer.from('x'));
    writeFileSync(
      join(root, 'gd32vw553', 'ram-loader-gd32vw553-1.0.0.txt'),
      '  SDK build revision: 94fb25571b15fbea \n',
    );

    const manifest = buildManifest(root, BASE);
    expect(manifest.loaders[0].notes).toBe(
      'SDK build revision: 94fb25571b15fbea',
    );
  });

  it('rejects a bin carrying the other family’s prefix', () => {
    const dir = join(root, 'ln882h');
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'auth-firmware-ln882h-1.0.0.bin'), 'x');

    expect(() => buildManifest(root, BASE)).toThrow(
      /ram-loader-ln882h-<version>\.bin/,
    );
  });

  it('sorts by chip ascending then version descending', () => {
    writeBin('ln882h', '1.0.0', Buffer.from('a'));
    writeBin('ln882h', '1.10.0', Buffer.from('b'));
    writeBin('gd32vw553', '1.0.0', Buffer.from('c'));

    const manifest = buildManifest(root, BASE);

    expect(manifest.loaders.map((e) => `${e.chip}@${e.version}`)).toEqual([
      'gd32vw553@1.0.0',
      'ln882h@1.10.0',
      'ln882h@1.0.0',
    ]);
  });
});
