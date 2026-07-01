#!/usr/bin/env tsx
// ──────────────────────────────────────────────────────────────────────────────
// bump-version.ts — Synchronize version across all project files and insert a
//                   draft CHANGELOG section (cross-platform)
//
// Usage:
//   pnpm version:set 3.1.4          # Bump to 3.1.4, insert CHANGELOG draft
//   pnpm version:set 3.1.4 --beta   # Bump to 3.1.4, skip CHANGELOG (beta build)
// ──────────────────────────────────────────────────────────────────────────────

import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, relative, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildDraftSection, insertSection } from './lib/changelog.js';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');

// ── Argument handling ────────────────────────────────────────────────────────

const args = process.argv.slice(2);
const version = args.find((a) => !a.startsWith('--'));
const isBeta = args.includes('--beta');

if (!version) {
  console.log('Usage: pnpm version:set <version> [--beta]');
  console.log('');
  console.log('Examples:');
  console.log('  pnpm version:set 3.1.4          # Bump + insert CHANGELOG draft');
  console.log('  pnpm version:set 3.1.4 --beta   # Bump only, skip CHANGELOG');
  process.exit(1);
}

if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`Error: "${version}" is not a valid semver (x.y.z)`);
  process.exit(1);
}

// ── Read current version ─────────────────────────────────────────────────────

const pkgPath = resolve(ROOT, 'package.json');
const pkg = JSON.parse(readFileSync(pkgPath, 'utf-8'));
console.log(`Current version: ${pkg.version}`);
console.log(`Target version:  ${version}`);
console.log('');

// ── Update functions ─────────────────────────────────────────────────────────

function updateJson(filePath: string) {
  const content = JSON.parse(readFileSync(filePath, 'utf-8'));
  content.version = version;
  writeFileSync(filePath, JSON.stringify(content, null, 2) + '\n', 'utf-8');
  console.log(`  ✓ ${relative(ROOT, filePath)}`);
}

function updateCargoToml(filePath: string) {
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

// ── Apply version bumps ───────────────────────────────────────────────────────

console.log('Updating version files:');
updateJson(resolve(ROOT, 'package.json'));
updateJson(resolve(ROOT, 'src-tauri', 'tauri.conf.json'));
updateCargoToml(resolve(ROOT, 'src-tauri', 'Cargo.toml'));
updateCargoToml(resolve(ROOT, 'crates', 'tyutool-core', 'Cargo.toml'));
updateCargoToml(resolve(ROOT, 'crates', 'tyutool-cli', 'Cargo.toml'));

// ── Insert CHANGELOG draft ────────────────────────────────────────────────────

if (!isBeta) {
  console.log('');
  console.log('Updating CHANGELOG.md:');
  const changelogPath = resolve(ROOT, 'CHANGELOG.md');
  const changelog = readFileSync(changelogPath, 'utf-8');

  if (changelog.includes(`## [${version}]`)) {
    console.log(`  ⚠ CHANGELOG.md already has a [${version}] section — skipped`);
  } else {
    const today = new Date().toISOString().split('T')[0];
    const draft = buildDraftSection(version, today);
    writeFileSync(changelogPath, insertSection(changelog, draft), 'utf-8');
    console.log(`  ✓ CHANGELOG.md (draft section inserted for ${version})`);
    console.log(`  → Edit the section and remove the draft marker before releasing`);
  }
}

console.log('');
console.log(`Done. All files set to: ${version}`);
