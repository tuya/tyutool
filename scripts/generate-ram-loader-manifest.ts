/**
 * Generate ram-loader.json — the manifest tyutool_core::ram_loader downloads
 * from when a chip's RAM loader is not in the local cache.
 *
 * Layout (one chip per first-level dir; see assets/ram-loader/README.md):
 *   <SOURCE_DIR>/<chip>/ram-loader-<chip>-<version>.bin
 *   <SOURCE_DIR>/<chip>/ram-loader-<chip>-<version>.txt   (optional notes)
 *
 * Env: BASE_URL (release download base), SOURCE_DIR (default `ram-loader`),
 *      OUTPUT (default `ram-loader.json`).
 *
 * The scanning/validation/hashing is shared with the auth-firmware family in
 * lib/firmware-asset-manifest.ts. The consuming plugin pins the one (chip,
 * version, sha256) it was written against, so this manifest is a lookup table,
 * not a source of "what's newest" — see the README.
 */
import { fileURLToPath } from 'node:url';

import {
  buildAssetEntries,
  runManifestCli,
  type AssetEntry,
  type AssetFamily,
} from './lib/firmware-asset-manifest.js';

/** Mirrors RamLoaderEntry in crates/tyutool-core/src/ram_loader.rs. */
export type RamLoaderEntry = AssetEntry;

export interface RamLoaderManifest {
  loaders: RamLoaderEntry[];
}

/** No forbidden chips: unlike auth firmware, a RAM loader exists for exactly
 *  the chips whose ROM cannot flash on its own, and never for `other`, which
 *  has no plugin at all. */
const FAMILY: AssetFamily = { prefix: 'ram-loader', key: 'loaders' };

export function buildManifest(
  sourceDir: string,
  baseUrl: string,
): RamLoaderManifest {
  return { loaders: buildAssetEntries(FAMILY, sourceDir, baseUrl) };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  runManifestCli(FAMILY, {
    sourceDir: 'ram-loader',
    output: 'ram-loader.json',
  });
}
