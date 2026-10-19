#!/usr/bin/env tsx
// ──────────────────────────────────────────────────────────────────────────────
// check-tauri-version-parity.ts — fail when an npm Tauri package and its Rust
//                                 crate are on different major/minor releases
//
// `tauri build` performs this check itself and aborts, but only at build time:
// a drift introduced by two independent Dependabot PRs therefore costs every GUI
// job in the Release matrix. Run it from CI so the PR that introduces it fails
// instead. See lib/tauri-version-parity.ts for the rule and the incident.
// ──────────────────────────────────────────────────────────────────────────────

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { getRepoRoot } from './lib/repo-root.js';
import {
  checkTauriVersionParity,
  comparedPairs,
  missingNpmPackages,
  parseCargoLockVersions,
  parsePnpmImporterVersions,
} from './lib/tauri-version-parity.js';

const ROOT = getRepoRoot(import.meta.url);

const npmVersions = parsePnpmImporterVersions(
  readFileSync(resolve(ROOT, 'pnpm-lock.yaml'), 'utf-8'),
);
const cargoVersions = parseCargoLockVersions(readFileSync(resolve(ROOT, 'Cargo.lock'), 'utf-8'));

const packageJson = JSON.parse(readFileSync(resolve(ROOT, 'package.json'), 'utf-8')) as {
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
};
const declaredPackages = [
  ...Object.keys(packageJson.dependencies ?? {}),
  ...Object.keys(packageJson.devDependencies ?? {}),
];

const unparsed = missingNpmPackages(declaredPackages, npmVersions);
if (unparsed.length > 0) {
  console.error('Could not read these declared Tauri packages from pnpm-lock.yaml:');
  for (const name of unparsed) console.error(`  ${name}`);
  console.error('');
  console.error('The lockfile layout may have changed — update');
  console.error('scripts/lib/tauri-version-parity.ts instead of letting the check pass.');
  process.exit(1);
}

const errors = checkTauriVersionParity(npmVersions, cargoVersions);
if (errors.length > 0) {
  console.error('Found version mismatched Tauri packages (npm vs Rust crate):');
  for (const error of errors) console.error(`  ${error}`);
  console.error('');
  console.error('Align the Rust side (preferred) with:');
  console.error('  cargo update -p <crate> --precise <npm version>');
  process.exit(1);
}

const compared = comparedPairs(npmVersions, cargoVersions);
console.log(
  `Tauri npm/crate versions agree (${compared.length} pairs checked: ` +
    `${compared.map((p) => p.crateName).sort().join(', ')}).`,
);
