/**
 * Browser dev: free the WebSocket port, start tyutool-cli serve, then Vite.
 * Cross-platform replacement for scripts/dev-web.sh.
 */
import { type ChildProcess, spawn, spawnSync } from 'node:child_process';
import { join } from 'node:path';

import { ensureCargoAvailable } from './lib/cargo.js';
import { freeTcpPort } from './lib/free-tcp-port.js';
import { getRepoRoot } from './lib/repo-root.js';
import { run } from './lib/run.js';

const ROOT = getRepoRoot(import.meta.url);
process.chdir(ROOT);

const PORT = Number(process.env.TYUTOOL_SERVE_PORT ?? 9527);
if (!Number.isFinite(PORT) || PORT <= 0 || PORT > 65535) {
  console.error(`ERROR: invalid TYUTOOL_SERVE_PORT: ${process.env.TYUTOOL_SERVE_PORT}`);
  process.exit(1);
}

ensureCargoAvailable();
freeTcpPort(PORT);

run('cargo', ['build', '-q', '-p', 'tyutool-cli'], { cwd: ROOT });

let serveProc: ChildProcess | null = null;
let cleaningUp = false;

function killServe(): void {
  if (!serveProc?.pid || serveProc.killed) {
    return;
  }
  const pid = serveProc.pid;
  try {
    if (process.platform === 'win32') {
      spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore' });
    } else {
      serveProc.kill('SIGTERM');
    }
  } catch {
    // best-effort
  }
}

function cleanupAndExit(code: number): void {
  if (cleaningUp) {
    return;
  }
  cleaningUp = true;
  killServe();
  process.exit(code);
}

process.on('SIGINT', () => cleanupAndExit(130));
process.on('SIGTERM', () => cleanupAndExit(143));
process.on('exit', () => {
  killServe();
});

serveProc = spawn(
  'cargo',
  ['run', '-p', 'tyutool-cli', '--', 'serve', '--port', String(PORT)],
  {
    cwd: ROOT,
    stdio: 'inherit',
    env: process.env,
  },
);

serveProc.on('error', (err) => {
  console.error('ERROR: failed to start tyutool-cli serve:', err.message);
  cleanupAndExit(1);
});

serveProc.on('exit', (code, signal) => {
  if (cleaningUp) {
    return;
  }
  if (code !== 0 && code !== null) {
    console.error(`tyutool-cli serve exited with code ${code}`);
    cleanupAndExit(code);
  } else if (signal) {
    console.error(`tyutool-cli serve killed by signal ${signal}`);
    cleanupAndExit(1);
  }
});

process.env.DEV_WEB_LOOSE_PORT = '1';

const viteBin = join(ROOT, 'node_modules', 'vite', 'bin', 'vite.js');
const viteProc = spawn(process.execPath, [viteBin], {
  cwd: ROOT,
  stdio: 'inherit',
  env: process.env,
});

viteProc.on('error', (err) => {
  console.error('ERROR: failed to start Vite:', err.message);
  cleanupAndExit(1);
});

viteProc.on('exit', (code, signal) => {
  if (signal === 'SIGINT') {
    cleanupAndExit(130);
    return;
  }
  cleanupAndExit(code ?? (signal ? 1 : 0));
});
