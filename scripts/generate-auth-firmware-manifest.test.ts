import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  buildManifest,
  compareVersionDesc,
  parseEntry,
} from './generate-auth-firmware-manifest.js';

const BASE = 'https://example.com/releases/download/auth-firmware';

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'auth-fw-'));
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

function binName(chip: string, version: string): string {
  return `auth-firmware-${chip}-${version}.bin`;
}

function writeBin(chip: string, version: string, content: Buffer): void {
  const dir = join(root, chip);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, binName(chip, version)), content);
}

function writeNotes(chip: string, version: string, text: string): void {
  mkdirSync(join(root, chip), { recursive: true });
  writeFileSync(join(root, chip, `auth-firmware-${chip}-${version}.txt`), text);
}

describe('parseEntry', () => {
  it('parses a valid bin filename into chip + version', () => {
    expect(parseEntry('esp32', 'auth-firmware-esp32-v1.0.0.bin')).toEqual({
      chip: 'esp32',
      version: 'v1.0.0',
    });
  });

  it('keeps the whole remainder as version (dots, no leading v)', () => {
    expect(parseEntry('bk7231n', 'auth-firmware-bk7231n-1.2.3.bin')).toEqual({
      chip: 'bk7231n',
      version: '1.2.3',
    });
  });

  it('throws when the prefix does not match auth-firmware-<chip>-', () => {
    expect(() => parseEntry('esp32', 'firmware-esp32-v1.0.0.bin')).toThrow();
  });

  it('throws when the embedded chip differs from the directory chip', () => {
    expect(() =>
      parseEntry('esp32', 'auth-firmware-bk7231n-v1.0.0.bin'),
    ).toThrow();
  });

  it('throws when the version part is empty', () => {
    expect(() => parseEntry('esp32', 'auth-firmware-esp32-.bin')).toThrow();
  });

  it('throws when the file is not a .bin', () => {
    expect(() => parseEntry('esp32', 'auth-firmware-esp32-v1.0.0.txt')).toThrow();
  });
});

describe('compareVersionDesc', () => {
  it('orders newer versions first', () => {
    expect(compareVersionDesc('v1.1.0', 'v1.0.0')).toBeLessThan(0);
    expect(compareVersionDesc('v1.0.0', 'v1.1.0')).toBeGreaterThan(0);
  });

  it('treats equal versions as 0', () => {
    expect(compareVersionDesc('v2.0.0', 'v2.0.0')).toBe(0);
  });

  it('tolerates a missing leading v', () => {
    expect(compareVersionDesc('1.2.0', 'v1.1.0')).toBeLessThan(0);
  });

  it('compares segments beyond the first three (numeric-aware)', () => {
    expect(compareVersionDesc('1.2.3.5', '1.2.3.4')).toBeLessThan(0);
    expect(compareVersionDesc('1.2.3.4', '1.2.3.5')).toBeGreaterThan(0);
  });

  it('orders by numeric value of each segment (1.10 > 1.2)', () => {
    expect(compareVersionDesc('v1.10.0', 'v1.2.0')).toBeLessThan(0);
  });

  it('produces a deterministic order for pre-release suffixes', () => {
    // Order doesn't need to match strict semver — only to be deterministic
    // (the prior Number()-based impl returned NaN for 'rc1' and broke sort).
    expect(compareVersionDesc('v1.0.0-rc1', 'v1.0.0-rc1')).toBe(0);
    expect(
      Math.sign(compareVersionDesc('v1.0.0-rc1', 'v1.0.0-rc2')),
    ).toBe(-Math.sign(compareVersionDesc('v1.0.0-rc2', 'v1.0.0-rc1')));
    expect(compareVersionDesc('v1.1.0-rc1', 'v1.0.0')).toBeLessThan(0);
  });
});

describe('buildManifest', () => {
  it('builds one entry with sha256 (lowercase hex of file bytes), size, and url', () => {
    const content = Buffer.from('tyutool firmware payload');
    writeBin('esp32', 'v1.0.0', content);

    const manifest = buildManifest(root, BASE);

    expect(manifest.firmwares).toHaveLength(1);
    const entry = manifest.firmwares[0];
    expect(entry.chip).toBe('esp32');
    expect(entry.version).toBe('v1.0.0');
    expect(entry.url).toBe(`${BASE}/auth-firmware-esp32-v1.0.0.bin`);
    expect(entry.size).toBe(content.byteLength);
    expect(entry.sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(entry.sha256).toBe(createHash('sha256').update(content).digest('hex'));
    expect(entry.notes).toBeUndefined();
  });

  it('strips a trailing slash on the base url', () => {
    writeBin('esp32', 'v1.0.0', Buffer.from('x'));
    const manifest = buildManifest(root, `${BASE}/`);
    expect(manifest.firmwares[0].url).toBe(
      `${BASE}/auth-firmware-esp32-v1.0.0.bin`,
    );
  });

  it('reads trimmed notes from a sibling .txt when present', () => {
    writeBin('esp32', 'v1.0.0', Buffer.from('x'));
    writeNotes('esp32', 'v1.0.0', '  first release \n');
    const manifest = buildManifest(root, BASE);
    expect(manifest.firmwares[0].notes).toBe('first release');
  });

  it('sorts by chip ascending then version descending', () => {
    writeBin('esp32', 'v1.0.0', Buffer.from('a'));
    writeBin('esp32', 'v1.1.0', Buffer.from('b'));
    writeBin('bk7231n', 'v1.0.0', Buffer.from('c'));

    const manifest = buildManifest(root, BASE);

    expect(
      manifest.firmwares.map((e) => `${e.chip}@${e.version}`),
    ).toEqual(['bk7231n@v1.0.0', 'esp32@v1.1.0', 'esp32@v1.0.0']);
  });

  it('ignores stray non-directory entries at the source root', () => {
    writeBin('esp32', 'v1.0.0', Buffer.from('x'));
    writeFileSync(join(root, 'README.md'), 'not a chip');
    const manifest = buildManifest(root, BASE);
    expect(manifest.firmwares).toHaveLength(1);
  });

  it('ignores a subdirectory inside a chip dir whose name ends in .bin', () => {
    writeBin('esp32', 'v1.0.0', Buffer.from('x'));
    mkdirSync(join(root, 'esp32', 'nested.bin'), { recursive: true });
    const manifest = buildManifest(root, BASE);
    expect(manifest.firmwares).toHaveLength(1);
  });

  it('throws when an other/ chip directory is present', () => {
    writeBin('other', 'v1.0.0', Buffer.from('x'));
    expect(() => buildManifest(root, BASE)).toThrow(/other/);
  });

  it('throws when a chip directory name has uppercase letters', () => {
    writeBin('ESP32', 'v1.0.0', Buffer.from('x'));
    expect(() => buildManifest(root, BASE)).toThrow(/ESP32/);
  });

  it('throws when a chip directory name has illegal characters', () => {
    writeBin('esp 32', 'v1.0.0', Buffer.from('x'));
    expect(() => buildManifest(root, BASE)).toThrow();
  });

  it('throws when a bin filename has a mismatched prefix', () => {
    const dir = join(root, 'esp32');
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'firmware-esp32-v1.0.0.bin'), 'x');
    expect(() => buildManifest(root, BASE)).toThrow();
  });

  it('throws when no firmware bins exist at all', () => {
    expect(() => buildManifest(root, BASE)).toThrow();
  });
});
