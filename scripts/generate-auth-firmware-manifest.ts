/**
 * Generate auth-firmware.json for batch-flash-auth's "default authorization
 * firmware" list from a directory tree of firmware bins.
 *
 * Layout (one chip per first-level dir; see design spec §3):
 *   <SOURCE_DIR>/<chip>/auth-firmware-<chip>-<version>.bin
 *   <SOURCE_DIR>/<chip>/auth-firmware-<chip>-<version>.txt   (optional notes)
 *
 * Env: BASE_URL (release download base), SOURCE_DIR (default `auth-firmware`),
 *      OUTPUT (default `auth-firmware.json`).
 *
 * The scanning/validation/hashing is shared with the ram-loader family in
 * lib/firmware-asset-manifest.ts. Unrelated to scripts/generate-manifest.ts
 * (the app self-update latest.json).
 */
import { fileURLToPath } from 'node:url';

import {
  buildAssetEntries,
  compareVersionDesc,
  parseAssetEntry,
  runManifestCli,
  type AssetEntry,
  type AssetFamily,
} from './lib/firmware-asset-manifest.js';

export { compareVersionDesc };

/** Mirrors AuthFirmwareEntry in src/features/batch-flash-auth/types.ts. */
export type AuthFirmwareEntry = AssetEntry;

export interface AuthFirmwareManifest {
  firmwares: AuthFirmwareEntry[];
}

/** `other` is the auth-only chip: it runs through FlashMode::Authorize and has
 *  no default firmware, so an `other/` directory is a mistake. */
const FAMILY: AssetFamily = {
  prefix: 'auth-firmware',
  key: 'firmwares',
  forbiddenChips: ['other'],
};

/** Validate one bin filename against this family's naming rule. */
export function parseEntry(
  chip: string,
  fileName: string,
): { chip: string; version: string } {
  return parseAssetEntry(FAMILY, chip, fileName);
}

export function buildManifest(
  sourceDir: string,
  baseUrl: string,
): AuthFirmwareManifest {
  return { firmwares: buildAssetEntries(FAMILY, sourceDir, baseUrl) };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  runManifestCli(FAMILY, {
    sourceDir: 'auth-firmware',
    output: 'auth-firmware.json',
  });
}
