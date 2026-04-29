/**
 * CLI release: tyutool_cli binary only — no Node, no Tauri, no WebView.
 * Output: target/release/tyutool_cli (from repository root).
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

import { ensureCargoAvailable } from './lib/cargo.js';
import { getRepoRoot } from './lib/repo-root.js';
import { run } from './lib/run.js';

const ROOT = getRepoRoot(import.meta.url);
process.chdir(ROOT);

ensureCargoAvailable();

console.log('==> tyutool CLI: cargo test (tyutool-core)');
run('cargo', ['test', '-p', 'tyutool-core'], { cwd: ROOT });

console.log('==> tyutool CLI: release build (tyutool-cli)');
run('cargo', ['build', '--release', '-p', 'tyutool-cli'], { cwd: ROOT });

const binName = process.platform === 'win32' ? 'tyutool_cli.exe' : 'tyutool_cli';
const out = join(ROOT, 'target', 'release', binName);
if (!existsSync(out)) {
  console.error(`ERROR: expected binary missing: ${out}`);
  process.exit(1);
}

console.log('==> Done (CLI). Binary:', out);
if (process.platform === 'win32') {
  spawnSync('cmd', ['/c', 'dir', out], { stdio: 'inherit' });
} else {
  spawnSync('ls', ['-la', out], { stdio: 'inherit' });
}
