#!/usr/bin/env tsx
// ──────────────────────────────────────────────────────────────────────────────
// bump-version.ts — Synchronize version across all project files and insert a
//                   draft CHANGELOG section (local entry, cross-platform)
//
// Usage:
//   pnpm version:set 3.1.4          # Bump to 3.1.4, insert CHANGELOG draft
//   pnpm version:set 3.1.4 --beta   # Bump to 3.1.4, skip CHANGELOG (beta build)
//
// The file list and rewrite logic live in lib/version-files.mjs, shared with
// bump-version.mjs (the CI entry) — add new version-bearing files there.
// ──────────────────────────────────────────────────────────────────────────────

import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildDraftSection, insertSection } from './lib/changelog.js';
import { readCurrentVersion, syncVersionFiles } from './lib/version-files.mjs';

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

console.log(`Current version: ${readCurrentVersion()}`);
console.log(`Target version:  ${version}`);
console.log('');

// ── Apply version bumps ───────────────────────────────────────────────────────

console.log('Updating version files:');
for (const path of syncVersionFiles(version)) {
  console.log(`  ✓ ${path}`);
}

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
