/**
 * Build-time app metadata (see `vite.config.ts` `define.__APP_VERSION__`).
 * Release version for npm / Vite UI; keep `src-tauri/tauri.conf.json` `version` in sync when shipping.
 */
export const APP_VERSION = __APP_VERSION__;

/** Canonical repository — used for issue reporting links. */
export const GITHUB_REPO = "https://github.com/tuya/tyutool";
export const GITHUB_NEW_ISSUE_URL = `${GITHUB_REPO}/issues/new`;
