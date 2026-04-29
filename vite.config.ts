import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { devOpenAppLogDirPlugin } from "./vite-plugin-dev-open-log-dir";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const pkg = JSON.parse(
  readFileSync(path.join(__dirname, "package.json"), "utf-8"),
) as { version: string };

// Allow overriding the displayed version at dev time for update-flow testing:
//   APP_VERSION=0.0.1 pnpm run tauri:dev
// @ts-expect-error process is a nodejs global
const appVersion: string = process.env.APP_VERSION || pkg.version;

// `pnpm run dev:web` sets DEV_WEB_LOOSE_PORT=1 so if 1420 is already in use (e.g. another Vite),
// the dev server tries the next free port instead of exiting.
// @ts-expect-error process is a nodejs global
const devWebLoosePort = process.env.DEV_WEB_LOOSE_PORT === "1";

// https://vite.dev/config/
// Relative base so bundled JS/CSS resolve under Tauri's custom protocol on Linux/AppImage
// (absolute "/assets/..." often loads nothing → blank window).
export default defineConfig(async () => ({
  base: "./",
  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  plugins: [vue(), tailwindcss(), devOpenAppLogDirPlugin()],

  build: {
    rollupOptions: {
      output: {
        manualChunks: {
          vue: ['vue', 'vue-router', 'pinia'],
          i18n: ['vue-i18n'],
          fontawesome: [
            '@fortawesome/fontawesome-svg-core',
            '@fortawesome/vue-fontawesome',
            '@fortawesome/free-solid-svg-icons',
          ],
        },
      },
    },
  },
  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri / normal `pnpm dev` expect 1420; `dev:web` may relax strictPort (see devWebLoosePort)
  server: {
    port: 1420,
    strictPort: !devWebLoosePort,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Ignore Rust tree and Tauri crate — `dev:web` runs `cargo build` first; watching
      // `target/**` (especially incremental/) exhausts Linux inotify (ENOSPC).
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },
}));
