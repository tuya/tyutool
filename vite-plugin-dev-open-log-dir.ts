/**
 * Dev-only Vite plugin: open the same log directory as Tauri `app_log_dir` via the OS file manager.
 * Must stay in sync with `identifier` in `src-tauri/tauri.conf.json` and path hints in
 * `src/config/tauri-desktop-paths.ts`.
 */
import type { Plugin } from "vite";
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

/** Same as `identifier` in `src-tauri/tauri.conf.json`. */
const TAURI_APP_IDENTIFIER = "com.tyutool.desktop";

function resolveAppLogDirAbsolute(): string {
  const home = os.homedir();
  if (process.platform === "win32") {
    const localAppData =
      process.env.LOCALAPPDATA || path.join(home, "AppData", "Local");
    return path.join(localAppData, TAURI_APP_IDENTIFIER, "logs");
  }
  if (process.platform === "darwin") {
    return path.join(home, "Library", "Logs", TAURI_APP_IDENTIFIER);
  }
  const xdgDataHome =
    process.env.XDG_DATA_HOME || path.join(home, ".local", "share");
  return path.join(xdgDataHome, TAURI_APP_IDENTIFIER, "logs");
}

function openDirInOsFileManager(dir: string): void {
  if (process.platform === "darwin") {
    spawn("open", [dir], { detached: true, stdio: "ignore" }).unref();
    return;
  }
  if (process.platform === "win32") {
    spawn("explorer", [dir], { detached: true, stdio: "ignore" }).unref();
    return;
  }
  spawn("xdg-open", [dir], { detached: true, stdio: "ignore" }).unref();
}

export function devOpenAppLogDirPlugin(): Plugin {
  return {
    name: "dev-open-app-log-dir",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const pathname = req.url?.split("?")[0] ?? "";
        if (pathname !== "/__dev/open-app-log-dir") {
          next();
          return;
        }
        if (req.method !== "GET" && req.method !== "POST") {
          res.statusCode = 405;
          res.setHeader("Content-Type", "application/json");
          res.end(JSON.stringify({ ok: false, error: "Method not allowed" }));
          return;
        }
        try {
          const dir = resolveAppLogDirAbsolute();
          fs.mkdirSync(dir, { recursive: true });
          openDirInOsFileManager(dir);
          res.setHeader("Content-Type", "application/json");
          res.end(JSON.stringify({ ok: true, path: dir }));
        } catch (e) {
          res.statusCode = 500;
          res.setHeader("Content-Type", "application/json");
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
