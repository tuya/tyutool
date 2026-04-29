/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Set when built/served under Tauri (see Tauri Vite plugin). */
  readonly TAURI_ENV_PLATFORM?: string;
}

declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

export {};

declare global {
  /** Injected by Vite `define` in `vite.config.ts` from `package.json` `version`. */
  const __APP_VERSION__: string;
}

declare module 'vue-router' {
  interface RouteMeta {
    /** Window title segment (paired with app name in `router.afterEach`). */
    title?: string;
    /** Shell layout: full-bleed main area for dense tools (e.g. flash). */
    layout?: 'default' | 'fullBleed';
  }
}
