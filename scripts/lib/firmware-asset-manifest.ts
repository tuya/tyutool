/**
 * Shared builder for the repo's published firmware-asset manifests.
 *
 * Two asset families use the identical layout and differ only in their filename
 * prefix and manifest key, so the scanning, naming validation, hashing and
 * sorting live here once:
 *
 *   assets/auth-firmware/<chip>/auth-firmware-<chip>-<version>.bin  → firmwares[]
 *   assets/ram-loader/<chip>/ram-loader-<chip>-<version>.bin        → loaders[]
 *
 * The thin entry points are scripts/generate-auth-firmware-manifest.ts and
 * scripts/generate-ram-loader-manifest.ts; each keeps its own docs, defaults and
 * consumer references. Unrelated to scripts/generate-manifest.ts (the app
 * self-update latest.json).
 */
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * One published asset. `size` is always emitted (cheap and useful) though the
 * frontend types mark it optional; `sha256` is lowercase hex to match the Rust
 * verifiers (src-tauri/src/updater.rs download_auth_firmware and
 * tyutool_core::ram_loader).
 */
export interface AssetEntry {
  version: string;
  chip: string;
  url: string;
  sha256: string;
  size: number;
  notes?: string;
}

/** What distinguishes one asset family from the other. */
export interface AssetFamily {
  /** Filename prefix, e.g. `auth-firmware` — also the assets/ directory name. */
  prefix: string;
  /** Top-level manifest key holding the entry array, e.g. `firmwares`. */
  key: string;
  /**
   * Chip ids that must never carry this asset. `auth-firmware` forbids `other`
   * (the auth-only chip runs through FlashMode::Authorize and has no default
   * firmware); `ram-loader` forbids nothing.
   */
  forbiddenChips?: readonly string[];
}

/** Chip directory names are the authoritative chip id and must be lowercase
 *  to match the Rust registry / frontend manifest. */
const CHIP_DIR_NAME_RE = /^[a-z0-9][a-z0-9_]*$/;

/**
 * Validate `fileName` against the `<prefix>-<chip>-<version>.bin` rule for its
 * containing `chip` directory and return the parsed version. Throws on any
 * violation (wrong prefix, embedded chip mismatch, empty version, non-bin).
 */
export function parseAssetEntry(
  family: AssetFamily,
  chip: string,
  fileName: string,
): { chip: string; version: string } {
  if (!fileName.endsWith('.bin')) {
    throw new Error(`Not a firmware .bin: "${fileName}"`);
  }
  const prefix = `${family.prefix}-${chip}-`;
  if (!fileName.startsWith(prefix)) {
    throw new Error(
      `Firmware "${fileName}" in chip dir "${chip}" must be named ` +
        `${family.prefix}-${chip}-<version>.bin`,
    );
  }
  const version = fileName.slice(prefix.length, -'.bin'.length);
  if (!version) {
    throw new Error(`Firmware "${fileName}" has an empty version`);
  }
  return { chip, version };
}

/** Descending numeric-aware version comparison (tolerates a leading 'v').
 *  Uses Intl numeric collation so all segments and non-numeric suffixes
 *  (e.g. `v1.0.0-rc1`) get a deterministic order — plain `Number()` parsing
 *  produced NaN comparators and silently broke sort for those inputs.
 *  Mirrors compareVersionDesc in src/features/batch-flash-auth/auth-firmware.ts. */
export function compareVersionDesc(a: string, b: string): number {
  const strip = (v: string): string => v.replace(/^v/, '');
  return strip(b).localeCompare(strip(a), 'en', { numeric: true });
}

/**
 * Scan `sourceDir`'s first-level chip directories, validate naming, compute
 * sha256/size, read optional notes, and return the entries sorted by chip
 * ascending then version descending. Throws on a forbidden chip dir or when no
 * bins are found.
 */
export function buildAssetEntries(
  family: AssetFamily,
  sourceDir: string,
  baseUrl: string,
): AssetEntry[] {
  const base = baseUrl.replace(/\/$/, '');
  const chipDirs = readdirSync(sourceDir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name);

  const entries: AssetEntry[] = [];
  // Uniqueness guard. Structurally unreachable given the filesystem layout
  // (filenames are unique per dir, dirs map 1:1 to chips), so it has no unit
  // test — it documents the invariant and guards a future recursive scan.
  const seen = new Set<string>();

  for (const chip of chipDirs) {
    if (family.forbiddenChips?.includes(chip)) {
      throw new Error(
        `chip dir "${chip}" must not carry a ${family.prefix} asset`,
      );
    }
    if (!CHIP_DIR_NAME_RE.test(chip)) {
      throw new Error(
        `Invalid chip directory name "${chip}" — must match ${CHIP_DIR_NAME_RE} (lowercase, alphanumeric, '_')`,
      );
    }
    const chipDir = join(sourceDir, chip);
    const bins = readdirSync(chipDir, { withFileTypes: true })
      .filter((d) => d.isFile() && d.name.endsWith('.bin'))
      .map((d) => d.name);
    for (const fileName of bins) {
      const { version } = parseAssetEntry(family, chip, fileName);
      const key = `${chip}@${version}`;
      if (seen.has(key)) {
        throw new Error(`duplicate asset (chip, version): ${key}`);
      }
      seen.add(key);

      const buf = readFileSync(join(chipDir, fileName));
      const entry: AssetEntry = {
        chip,
        version,
        url: `${base}/${fileName}`,
        sha256: createHash('sha256').update(buf).digest('hex'),
        size: buf.byteLength,
      };

      const notesPath = join(chipDir, fileName.replace(/\.bin$/, '.txt'));
      if (existsSync(notesPath)) {
        entry.notes = readFileSync(notesPath, 'utf-8').trim();
      }

      entries.push(entry);
    }
  }

  if (entries.length === 0) {
    throw new Error(`no asset .bin files found under "${sourceDir}"`);
  }

  entries.sort(
    (a, b) =>
      a.chip.localeCompare(b.chip) || compareVersionDesc(a.version, b.version),
  );

  return entries;
}

/**
 * `main()` for a family's entry point: read BASE_URL / SOURCE_DIR / OUTPUT from
 * the environment, write the manifest, and log what went in. Exits non-zero
 * when BASE_URL is unset.
 */
export function runManifestCli(
  family: AssetFamily,
  defaults: { sourceDir: string; output: string },
): void {
  const baseUrl = process.env.BASE_URL;
  if (!baseUrl) {
    console.error('ERROR: BASE_URL must be set.');
    process.exit(1);
  }
  const sourceDir = process.env.SOURCE_DIR ?? defaults.sourceDir;
  const output = process.env.OUTPUT ?? defaults.output;

  const entries = buildAssetEntries(family, sourceDir, baseUrl);
  const manifest = { [family.key]: entries };
  writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`, 'utf-8');

  console.log(
    `Generated ${output} with ${entries.length} ${family.prefix} entr${entries.length === 1 ? 'y' : 'ies'}:`,
  );
  for (const e of entries) {
    console.log(`  ${e.chip} ${e.version} (${e.size} bytes)`);
  }
}
