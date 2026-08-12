// ──────────────────────────────────────────────────────────────────────────────
// version-files.mjs — The set of files carrying the project version, and the
//                     logic to rewrite them.
//
// Plain .mjs on purpose: release.yml runs `node scripts/bump-version.mjs`
// straight after checkout, before any pnpm/node setup step (the cli-build
// matrix never installs pnpm at all), so this must load under bare node.
// scripts/bump-version.ts imports it through tsx.
//
// Add a new version-bearing file HERE, not in either entry point — that is the
// whole reason this module exists. Cargo crates need no entry: they inherit
// from [workspace.package] in the root Cargo.toml. version-files.test.ts
// enforces both halves of that.
// ──────────────────────────────────────────────────────────────────────────────

import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

/**
 * Files whose version field is kept in lockstep, relative to the repo root.
 * The root Cargo.toml covers all three crates via [workspace.package].
 */
export const VERSION_FILES = [
  { path: 'package.json', kind: 'json' },
  { path: 'src-tauri/tauri.conf.json', kind: 'json' },
  { path: 'Cargo.toml', kind: 'cargo' },
];

/** Set the top-level "version" key, preserving 2-space indent and trailing newline. */
export function applyJsonVersion(content, version) {
  const parsed = JSON.parse(content);
  parsed.version = version;
  return JSON.stringify(parsed, null, 2) + '\n';
}

/**
 * Set the crate version in a Cargo.toml. Only the FIRST `version = "..."` is
 * rewritten — the key recurs under [dependencies], and those must not move.
 */
export function applyCargoVersion(content, version) {
  let replaced = false;
  return content.replace(/^version\s*=\s*"[^"]*"/m, (match) => {
    if (replaced) return match;
    replaced = true;
    return `version = "${version}"`;
  });
}

/** Current version, read from package.json. */
export function readCurrentVersion() {
  return JSON.parse(readFileSync(resolve(ROOT, 'package.json'), 'utf-8')).version;
}

/**
 * Write `version` into every file in VERSION_FILES.
 * Returns the repo-relative paths that were rewritten, in order.
 */
export function syncVersionFiles(version) {
  for (const { path, kind } of VERSION_FILES) {
    const abs = resolve(ROOT, path);
    const apply = kind === 'json' ? applyJsonVersion : applyCargoVersion;
    writeFileSync(abs, apply(readFileSync(abs, 'utf-8'), version), 'utf-8');
  }
  return VERSION_FILES.map(({ path }) => path);
}
