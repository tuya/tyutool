/**
 * Dev-only Vite plugin: open the same log directory as Tauri `app_log_dir` via the OS file manager.
 */
import type { Plugin } from 'vite';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import { DEV_OPEN_APP_LOG_DIR_PATH } from '../src/config/dev-endpoints';
import { resolveAppLogDirAbsolute } from './resolve-app-log-dir';

function openDirInOsFileManager(dir: string): void {
  if (process.platform === 'darwin') {
    spawn('open', [dir], { detached: true, stdio: 'ignore' }).unref();
    return;
  }
  if (process.platform === 'win32') {
    spawn('explorer', [dir], { detached: true, stdio: 'ignore' }).unref();
    return;
  }
  spawn('xdg-open', [dir], { detached: true, stdio: 'ignore' }).unref();
}

export function devOpenAppLogDirPlugin(): Plugin {
  return {
    name: 'dev-open-app-log-dir',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const pathname = req.url?.split('?')[0] ?? '';
        if (pathname !== DEV_OPEN_APP_LOG_DIR_PATH) {
          next();
          return;
        }
        if (req.method !== 'GET' && req.method !== 'POST') {
          res.statusCode = 405;
          res.setHeader('Content-Type', 'application/json');
          res.end(JSON.stringify({ ok: false, error: 'Method not allowed' }));
          return;
        }
        try {
          const dir = resolveAppLogDirAbsolute();
          fs.mkdirSync(dir, { recursive: true });
          openDirInOsFileManager(dir);
          res.setHeader('Content-Type', 'application/json');
          res.end(JSON.stringify({ ok: true, path: dir }));
        } catch (e) {
          res.statusCode = 500;
          res.setHeader('Content-Type', 'application/json');
          res.end(
            JSON.stringify({
              ok: false,
              error: e instanceof Error ? e.message : String(e),
            }),
          );
        }
      });
    },
  };
}
