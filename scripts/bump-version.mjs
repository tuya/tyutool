#!/usr/bin/env node
// ──────────────────────────────────────────────────────────────────────────────
// bump-version.mjs — Synchronize version across all project files (cross-platform)
//
// Usage:
//   node scripts/bump-version.mjs 0.2.0    # Set exact version (release)
//   node scripts/bump-version.mjs beta     # Use base version as-is (beta label added to file names in CI)
//
// Files updated:
//   package.json                   "version": "..."
//   src-tauri/tauri.conf.json      "version": "..."
//   src-tauri/Cargo.toml           version = "..."
//   crates/tyutool-core/Cargo.toml version = "..."
//   crates/tyutool-cli/Cargo.toml  version = "..."
// ──────────────────────────────────────────────────────────────────────────────

import { readFileSync, writeFileSync } from 'fs';
import { resolve, relative, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');

// ── Argument handling ────────────────────────────────────────────────────────

const input = process.argv[2];
if (!input) {
  console.log('Usage: node scripts/bump-version.mjs <version|beta>');
  console.log('');
  console.log('Examples:');
  console.log('  node scripts/bump-version.mjs 0.2.0     # Set all files to 0.2.0');
  console.log('  node scripts/bump-version.mjs beta      # Use base version as-is (beta label in CI file names)');
  process.exit(1);
}

// ── Read current version from package.json ───────────────────────────────────

const pkgPath = resolve(ROOT, 'package.json');
const pkg = JSON.parse(readFileSync(pkgPath, 'utf-8'));
const current = pkg.version;
console.log(`Current version: ${current}`);

// ── Compute target version ───────────────────────────────────────────────────

let version;
if (input === 'beta') {
  // Beta builds use the base version without prerelease suffix.
  // This keeps the version MSI-compatible (no semver prerelease identifier).
  // The "beta" label is added to file names in CI instead.
  version = current.replace(/-.*$/, '');
} else {
  version = input;
}

console.log(`Target version:  ${version}`);

// ── Update functions ─────────────────────────────────────────────────────────

function updateJson(filePath) {
  const content = JSON.parse(readFileSync(filePath, 'utf-8'));
  content.version = version;
  writeFileSync(filePath, JSON.stringify(content, null, 2) + '\n', 'utf-8');
  console.log(`  ✓ ${relative(ROOT, filePath)}`);
}

function updateCargoToml(filePath) {
  let content = readFileSync(filePath, 'utf-8');
  let replaced = false;
  content = content.replace(/^version\s*=\s*"[^"]*"/m, (match) => {
    if (replaced) return match;
    replaced = true;
    return `version = "${version}"`;
  });
  writeFileSync(filePath, content, 'utf-8');
  console.log(`  ✓ ${relative(ROOT, filePath)}`);
}

// ── Apply updates ────────────────────────────────────────────────────────────

console.log('');
console.log('Updating files:');

updateJson(resolve(ROOT, 'package.json'));
updateJson(resolve(ROOT, 'src-tauri', 'tauri.conf.json'));
updateCargoToml(resolve(ROOT, 'src-tauri', 'Cargo.toml'));
updateCargoToml(resolve(ROOT, 'crates', 'tyutool-core', 'Cargo.toml'));
updateCargoToml(resolve(ROOT, 'crates', 'tyutool-cli', 'Cargo.toml'));

console.log('');
console.log(`Done. All files set to: ${version}`);
