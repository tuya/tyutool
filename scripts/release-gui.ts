/**
 * GUI release: Vite frontend + Tauri bundle only (no tyutool-cli binary).
 * Needs: Node, Rust, platform WebView toolchain (Linux: webkit2gtk, etc.).
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

console.log('==> tyutool GUI: pnpm install');
const lockfile = join(ROOT, 'pnpm-lock.yaml');
run('pnpm', existsSync(lockfile) ? ['install', '--frozen-lockfile'] : ['install'], {
  cwd: ROOT,
});

console.log('==> tyutool GUI: ensure Tauri icons');
const icon32 = join(ROOT, 'src-tauri', 'icons', '32x32.png');
if (!existsSync(icon32)) {
  run('pnpm', ['exec', 'tsx', 'scripts/generate-icon-source.ts'], { cwd: ROOT });
  run(
    'pnpm',
    ['exec', 'tauri', 'icon', join(ROOT, 'scripts', 'icon-source.png'), '-o', join(ROOT, 'src-tauri', 'icons')],
    { cwd: ROOT },
  );
}

console.log('==> tyutool GUI: frontend build (prebuild: clean dist, then vue-tsc + vite)');
run('pnpm', ['run', 'build'], { cwd: ROOT });

console.log('==> tyutool GUI: Rust tests (tyutool-core)');
run('cargo', ['test', '-p', 'tyutool-core'], { cwd: ROOT });

if (process.platform === 'linux') {
  const pkg = spawnSync('pkg-config', ['--version'], { stdio: 'pipe' });
  if (pkg.status !== 0) {
    console.error('ERROR: pkg-config not found. Install it (e.g. Debian/Ubuntu: sudo apt-get install pkg-config).');
    process.exit(1);
  }
  const gdk = spawnSync('pkg-config', ['--exists', 'gdk-3.0'], { stdio: 'pipe' });
  const webkit = spawnSync('pkg-config', ['--exists', 'webkit2gtk-4.1'], { stdio: 'pipe' });
  if (gdk.status !== 0 || webkit.status !== 0) {
    console.error(
      'ERROR: Linux system libraries for Tauri (GTK / WebKit2GTK) are missing or not visible to pkg-config.',
    );
    console.error('Install development packages, then retry. Debian/Ubuntu (same as CI):');
    console.error('  sudo apt-get update && sudo apt-get install -y \\');
    console.error('    libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \\');
    console.error('    build-essential curl wget file libxdo-dev libssl-dev \\');
    console.error('    libayatana-appindicator3-dev librsvg2-dev patchelf');
    console.error('See also: https://v2.tauri.app/start/prerequisites/');
    process.exit(1);
  }
}

console.log('==> tyutool GUI: Tauri bundle (embeds fresh dist via beforeBuildCommand)');
const isCi = process.env.CI === 'true';
if (isCi) {
  run('pnpm', ['exec', 'tauri', 'build', '--bundles', 'deb', '--ci'], { cwd: ROOT });
} else {
  run('pnpm', ['exec', 'tauri', 'build'], { cwd: ROOT });
}

console.log('==> Done (GUI). Bundles under target/release/bundle/ (workspace root)');
