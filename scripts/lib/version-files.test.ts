import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
  VERSION_FILES,
  applyCargoVersion,
  applyJsonVersion,
  readCurrentVersion,
} from './version-files.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

describe('VERSION_FILES', () => {
  // The regression this guards: bump-version.mjs (CI) and bump-version.ts
  // (local) used to carry separate hardcoded copies of this list, so adding a
  // crate could leave the release path bumping only some of them.
  it('covers the Cargo.toml of every cargo workspace member', () => {
    const workspace = readFileSync(resolve(ROOT, 'Cargo.toml'), 'utf-8');
    const members = [...workspace.matchAll(/"([^"]+)"/g)]
      .map((m) => m[1])
      .filter((m) => m.includes('/') || m === 'src-tauri');

    expect(members.length).toBeGreaterThan(0);
    const listed = VERSION_FILES.map((f) => f.path);
    for (const member of members) {
      expect(listed).toContain(`${member}/Cargo.toml`);
    }
  });

  it('covers package.json and tauri.conf.json', () => {
    const listed = VERSION_FILES.map((f) => f.path);
    expect(listed).toContain('package.json');
    expect(listed).toContain('src-tauri/tauri.conf.json');
  });

  it('every listed file exists and already carries the current version', () => {
    const current = readCurrentVersion();
    for (const { path, kind } of VERSION_FILES) {
      const content = readFileSync(resolve(ROOT, path), 'utf-8');
      const found =
        kind === 'json'
          ? JSON.parse(content).version
          : /^version\s*=\s*"([^"]*)"/m.exec(content)?.[1];
      expect(found, `${path} is out of sync`).toBe(current);
    }
  });
});

describe('applyCargoVersion', () => {
  it('rewrites the crate version but not dependency versions', () => {
    const toml = [
      '[package]',
      'name = "tyutool-cli"',
      'version = "3.2.8"',
      '',
      '[dependencies]',
      'serde = { version = "1.0", features = ["derive"] }',
      'version = "0.0.1"',
    ].join('\n');

    const out = applyCargoVersion(toml, '9.9.9');

    expect(out).toContain('version = "9.9.9"');
    expect(out).toContain('serde = { version = "1.0", features = ["derive"] }');
    expect(out).toContain('version = "0.0.1"');
    expect(out.match(/^version = "9\.9\.9"$/gm)).toHaveLength(1);
  });

  it('leaves content untouched when there is no version key', () => {
    const toml = '[workspace]\nresolver = "2"\n';
    expect(applyCargoVersion(toml, '9.9.9')).toBe(toml);
  });
});

describe('applyJsonVersion', () => {
  it('sets version, preserves other keys, and ends with a newline', () => {
    const json = JSON.stringify({ name: 'tyutool', version: '3.2.8', private: true }, null, 2);

    const out = applyJsonVersion(json, '9.9.9');

    expect(JSON.parse(out)).toEqual({ name: 'tyutool', version: '9.9.9', private: true });
    expect(out.endsWith('\n')).toBe(true);
    expect(out).toContain('\n  "version": "9.9.9"');
  });
});
