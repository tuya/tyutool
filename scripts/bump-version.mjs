#!/usr/bin/env node
// ──────────────────────────────────────────────────────────────────────────────
// bump-version.mjs — Synchronize the version across all project files (CI entry)
//
// Usage:
//   node scripts/bump-version.mjs 0.2.0    # Set exact version (release)
//   node scripts/bump-version.mjs beta     # Use base version as-is (beta label added to file names in CI)
//
// Called by release.yml before any pnpm/node setup, so it must run under bare
// node. The file list and rewrite logic live in lib/version-files.mjs, shared
// with bump-version.ts — add new version-bearing files there.
// ──────────────────────────────────────────────────────────────────────────────

import { readCurrentVersion, syncVersionFiles } from './lib/version-files.mjs';

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

const current = readCurrentVersion();
console.log(`Current version: ${current}`);

// ── Compute target version ───────────────────────────────────────────────────

// Beta builds use the base version without prerelease suffix. This keeps the
// version MSI-compatible (no semver prerelease identifier); the "beta" label is
// added to file names in CI instead.
const version = input === 'beta' ? current.replace(/-.*$/, '') : input;

console.log(`Target version:  ${version}`);

// ── Apply updates ────────────────────────────────────────────────────────────

console.log('');
console.log('Updating files:');

for (const path of syncVersionFiles(version)) {
  console.log(`  ✓ ${path}`);
}

console.log('');
console.log(`Done. All files set to: ${version}`);
