/**
 * Extract one version's CHANGELOG section into release-body.md (used as the GitHub release body).
 * Env: VERSION (no leading v).
 */
import { readFileSync, writeFileSync } from 'node:fs';

import { parseSection } from './lib/changelog.js';

const version = process.env.VERSION;
if (!version) {
  console.error('ERROR: VERSION must be set.');
  process.exit(1);
}

const body = parseSection(readFileSync('CHANGELOG.md', 'utf-8'), version);
if (!body) {
  console.error(`ERROR: CHANGELOG.md 缺少版本 ${version} 的小节。`);
  process.exit(1);
}

writeFileSync('release-body.md', `${body}\n`, 'utf-8');
console.log(`Wrote release-body.md for ${version} (${body.length} chars)`);
