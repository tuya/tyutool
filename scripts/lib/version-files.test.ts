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

function workspaceMembers(): string[] {
  const workspace = readFileSync(resolve(ROOT, 'Cargo.toml'), 'utf-8');
  const members = /members\s*=\s*\[([^\]]*)\]/s.exec(workspace)?.[1] ?? '';
  return [...members.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

describe('VERSION_FILES', () => {
  // The regression this guards: bump-version.mjs (CI) and bump-version.ts
  // (local) used to carry separate hardcoded copies of this list, so adding a
  // crate could leave the release path bumping only some of them.
  it('covers package.json, tauri.conf.json, and the workspace Cargo.toml', () => {
    const listed = VERSION_FILES.map((f) => f.path);
    expect(listed).toContain('package.json');
    expect(listed).toContain('src-tauri/tauri.conf.json');
    expect(listed).toContain('Cargo.toml');
  });

  // Members that version independently of the workspace, each with a reason:
  //  · tyutool-bridge — Cobuilder Bridge ships on its own release cadence
  //    (0.x, bumped by hand per release), and bridge.yml labels artifacts by
  //    grepping the literal `version = "…"` line out of its Cargo.toml.
  //    Inheriting the workspace version would break that grep and yoke the
  //    bridge to the upstream release train.
  // Adding an entry here requires the same kind of documented reason.
  const INDEPENDENT_VERSION_MEMBERS = ['crates/tyutool-bridge'];

  // Crates are covered transitively through [workspace.package]. A member that
  // declares its own literal version silently escapes the bump, so require
  // inheritance rather than listing each crate.
  it('every workspace member inherits its version instead of declaring one', () => {
    const members = workspaceMembers();
    expect(members.length).toBeGreaterThan(0);
    // A stale exemption (member renamed or removed) must fail, not silently skip.
    expect(members).toEqual(expect.arrayContaining(INDEPENDENT_VERSION_MEMBERS));

    for (const member of members) {
      const toml = readFileSync(resolve(ROOT, member, 'Cargo.toml'), 'utf-8');
      if (INDEPENDENT_VERSION_MEMBERS.includes(member)) {
        // If an exempted member ever switches to inheritance, its entry above
        // is stale and must be removed.
        expect(toml, `${member} is exempt but declares no literal version`).toMatch(
          /^version\s*=\s*"/m,
        );
        continue;
      }
      expect(toml, `${member} must use version.workspace = true`).toMatch(
        /^version\.workspace\s*=\s*true/m,
      );
      expect(toml, `${member} declares a literal version`).not.toMatch(
        /^version\s*=\s*"/m,
      );
    }
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
