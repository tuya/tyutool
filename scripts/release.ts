/**
 * Release command: bump → commit → tag → push, gated by preflight + curated changelog.
 * Usage: pnpm run release <X.Y.Z>   (interactive — opens $EDITOR for changelog polish)
 */
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';

import {
  buildDraftSection,
  insertSection,
  parseSection,
  validateCuratedSection,
} from './lib/changelog.js';
import { evaluatePreEditChecks, type PreflightState } from './lib/preflight.js';
import { getRepoRoot } from './lib/repo-root.js';

const EXPECTED_BRANCH = 'refactor/v3';
const ROOT = getRepoRoot(import.meta.url);
process.chdir(ROOT);

const version = process.argv[2];
if (!version) {
  console.error('Usage: pnpm run release <X.Y.Z>');
  process.exit(1);
}
const tag = `v${version}`;

function cap(cmd: string, args: string[]): string {
  return execFileSync(cmd, args, { encoding: 'utf-8' }).trim();
}
function tryCap(cmd: string, args: string[]): string {
  const r = spawnSync(cmd, args, { encoding: 'utf-8' });
  return r.status === 0 ? (r.stdout ?? '').trim() : '';
}
function step(cmd: string, args: string[]): void {
  const r = spawnSync(cmd, args, { stdio: 'inherit' });
  if (r.status !== 0) {
    console.error(`\n命令失败: ${cmd} ${args.join(' ')}`);
    process.exit(r.status ?? 1);
  }
}
function fail(errs: string[]): never {
  console.error('\n预检未通过：');
  for (const e of errs) console.error(`  ✗ ${e}`);
  process.exit(1);
}

// ── Gather git/gh state ──────────────────────────────────────────────────────
console.log(`==> 准备发布 ${tag}`);
step('git', ['fetch', 'origin', EXPECTED_BRANCH, '--tags']);

const branch = cap('git', ['rev-parse', '--abbrev-ref', 'HEAD']);
const isClean = cap('git', ['status', '--porcelain']) === '';
const head = cap('git', ['rev-parse', 'HEAD']);

let ahead = 0;
let behind = 0;
const counts = tryCap('git', ['rev-list', '--left-right', '--count', `origin/${EXPECTED_BRANCH}...HEAD`]);
if (counts) {
  const [b, a] = counts.split(/\s+/).map((n) => Number.parseInt(n, 10));
  behind = Number.isFinite(b) ? b : 0;
  ahead = Number.isFinite(a) ? a : 0;
}

const tagExistsLocal = tryCap('git', ['tag', '-l', tag]) !== '';
const tagExistsRemote = tryCap('git', ['ls-remote', '--tags', 'origin', tag]) !== '';

// CI conclusion for the exact HEAD SHA, from the "CI" workflow.
let ciStatus: string | null = null;
let ciConclusion: string | null = null;
const ciJson = tryCap('gh', [
  'run',
  'list',
  '--branch',
  EXPECTED_BRANCH,
  '--workflow',
  'CI',
  '--json',
  'headSha,status,conclusion',
  '--limit',
  '50',
]);
if (ciJson) {
  const runs = JSON.parse(ciJson) as { headSha: string; status: string; conclusion: string | null }[];
  const run = runs.find((r) => r.headSha === head);
  if (run) {
    ciStatus = run.status;
    ciConclusion = run.conclusion;
  }
}

const state: PreflightState = {
  version,
  branch,
  expectedBranch: EXPECTED_BRANCH,
  isClean,
  ahead,
  behind,
  tagExistsLocal,
  tagExistsRemote,
  ciStatus,
  ciConclusion,
};

const preErrs = evaluatePreEditChecks(state);
if (preErrs.length) fail(preErrs);
console.log('==> 前置检查通过');

// ── Changelog: draft + interactive polish ───────────────────────────────────
const today = cap('date', ['+%Y-%m-%d']);
const cliffBody = tryCap('git', ['cliff', '--unreleased', '--strip', 'all']);
if (!cliffBody) console.warn('警告: git-cliff 未产出内容（可能无新提交）；草稿将留空待手填。');

const changelogPath = 'CHANGELOG.md';
const existing = existsSync(changelogPath) ? readFileSync(changelogPath, 'utf-8') : '';
const draft = buildDraftSection(version, today, cliffBody);
writeFileSync(changelogPath, insertSection(existing, draft), 'utf-8');

const editor = process.env.EDITOR || process.env.VISUAL || 'vi';
console.log(`==> 已生成 ${tag} 草稿，打开 ${editor} 润色（删除标记、补中文）…`);
const ed = spawnSync(editor, [changelogPath], { stdio: 'inherit' });
if (ed.status !== 0) {
  console.error('编辑器异常退出，已中止（CHANGELOG.md 的草稿改动请手动 git checkout 还原）。');
  process.exit(1);
}

// ── Post-edit check (4-B) ────────────────────────────────────────────────────
const polished = parseSection(readFileSync(changelogPath, 'utf-8'), version);
const postErrs = validateCuratedSection(polished);
if (postErrs.length) {
  console.error('\nchangelog 未通过后置检查（CHANGELOG.md 改动保留，请修正后重跑）：');
  for (const e of postErrs) console.error(`  ✗ ${e}`);
  process.exit(1);
}
console.log('==> changelog 后置检查通过');

// ── Bump + lockfile + commit + tag + push ────────────────────────────────────
step('node', ['scripts/bump-version.mjs', version]);
step('cargo', ['update', '--workspace']);
step('git', [
  'add',
  'package.json',
  'src-tauri/tauri.conf.json',
  'src-tauri/Cargo.toml',
  'crates/tyutool-core/Cargo.toml',
  'crates/tyutool-cli/Cargo.toml',
  'Cargo.lock',
  'CHANGELOG.md',
]);
step('git', ['commit', '-m', `chore(release): ${tag}`]);
step('git', ['tag', tag]);
step('git', ['push', 'origin', EXPECTED_BRANCH]);
step('git', ['push', 'origin', tag]);

console.log(`\n✓ 已发布 ${tag}。CI 将构建并在校验通过后自动转正式 Release。`);
