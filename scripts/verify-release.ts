/**
 * Publish gate: verify the draft release's assets + latest.json, then un-draft.
 * On any failure, exit non-zero and LEAVE the release as a draft.
 * Env: TAG (e.g. v3.0.14), VERSION (e.g. 3.0.14). Uses `gh` (GH_TOKEN in CI).
 */
import { execFileSync } from 'node:child_process';

import {
  checkAssetCompleteness,
  toChinaManifest,
  validateManifest,
  type Manifest,
} from './lib/release-check.js';

const TAG = process.env.TAG;
const VERSION = process.env.VERSION;
if (!TAG || !VERSION) {
  console.error('ERROR: TAG and VERSION must be set.');
  process.exit(1);
}

function gh(args: string[]): string {
  return execFileSync('gh', args, { encoding: 'utf-8' });
}

// The tag lookup can return an empty asset list after a draft changes state.
// Resolve the release once, then query its assets by the stable release ID.
const repo =
  process.env.GITHUB_REPOSITORY ||
  gh([
    'repo',
    'view',
    '--json',
    'nameWithOwner',
    '--jq',
    '.nameWithOwner',
  ]).trim();
const releaseId = JSON.parse(
  gh(['release', 'view', TAG, '--json', 'databaseId']),
).databaseId as number;
const releaseAssets = JSON.parse(
  gh(['api', `repos/${repo}/releases/${releaseId}/assets?per_page=100`]),
) as {
  id: number;
  name: string;
}[];
const assets = releaseAssets.map((a) => a.name);
const assetSet = new Set(assets);

function readReleaseJson(name: string): Manifest {
  const asset = releaseAssets.find((a) => a.name === name);
  if (!asset) throw new Error(`Release 缺少 ${name}`);
  return JSON.parse(
    gh([
      'api',
      `repos/${repo}/releases/assets/${asset.id}`,
      '-H',
      'Accept: application/octet-stream',
    ]),
  ) as Manifest;
}

const manifest = readReleaseJson('latest.json');

const errors = [
  ...checkAssetCompleteness(VERSION, assetSet),
  ...validateManifest(manifest, VERSION, assetSet),
];

// release.json must exist and be the mainland-China variant of latest.json
// (every entry's url replaced by its url_tuya).
if (!assetSet.has('release.json')) {
  errors.push('缺少 release.json（latest.json 的大陆版，url 指向 Tuya OSS）');
} else {
  const releaseManifest = readReleaseJson('release.json');
  try {
    const expected = JSON.stringify(toChinaManifest(manifest));
    if (JSON.stringify(releaseManifest) !== expected) {
      errors.push('release.json 与 latest.json 的大陆版变换结果不一致');
    }
  } catch (e) {
    errors.push(e instanceof Error ? e.message : String(e));
  }
}

if (errors.length > 0) {
  console.error(`\n✗ ${TAG} 校验失败，Release 保留为草稿：`);
  for (const e of errors) console.error(`  - ${e}`);
  console.error('\n附：release 实际资源清单：');
  for (const a of assets.sort()) console.error(`  · ${a}`);
  process.exit(1);
}

console.log(`✓ ${TAG} 全部产物与 latest.json 校验通过，转为正式发布。`);
gh(['release', 'edit', TAG, '--draft=false']);
