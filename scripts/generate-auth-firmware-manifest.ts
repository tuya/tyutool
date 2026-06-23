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
 * Unrelated to scripts/generate-manifest.ts (the app self-update latest.json).
 */
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Mirrors AuthFirmwareEntry in src/features/batch-flash-auth/types.ts.
 * `size` is always emitted (cheap and useful) though the frontend type marks
 * it optional; `sha256` is lowercase hex to match the Rust download verifier
 * (src-tauri/src/lib.rs download_auth_firmware).
 */
export interface AuthFirmwareEntry {
  version: string;
  chip: string;
  url: string;
  sha256: string;
  size: number;
  notes?: string;
}

export interface AuthFirmwareManifest {
  firmwares: AuthFirmwareEntry[];
}

const AUTH_ONLY_CHIP = 'other';

/** Chip directory names are the authoritative chip id and must be lowercase
 *  to match the Rust registry / frontend manifest. */
const CHIP_DIR_NAME_RE = /^[a-z0-9][a-z0-9_]*$/;

/**
 * Validate `fileName` against the `auth-firmware-<chip>-<version>.bin` rule for
 * its containing `chip` directory and return the parsed version. Throws on any
 * violation (wrong prefix, embedded chip mismatch, empty version, non-bin).
 */
export function parseEntry(
  chip: string,
  fileName: string,
): { chip: string; version: string } {
  if (!fileName.endsWith('.bin')) {
    throw new Error(`Not a firmware .bin: "${fileName}"`);
  }
  const prefix = `auth-firmware-${chip}-`;
  if (!fileName.startsWith(prefix)) {
    throw new Error(
      `Firmware "${fileName}" in chip dir "${chip}" must be named ` +
        `auth-firmware-${chip}-<version>.bin`,
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
 * sha256/size, read optional notes, and return a manifest sorted by chip
 * ascending then version descending. Throws on an `other/` dir or when no bins
 * are found.
 */
export function buildManifest(
  sourceDir: string,
  baseUrl: string,
): AuthFirmwareManifest {
  const base = baseUrl.replace(/\/$/, '');
  const chipDirs = readdirSync(sourceDir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name);

  const firmwares: AuthFirmwareEntry[] = [];
  // Uniqueness guard. Structurally unreachable given the filesystem layout
  // (filenames are unique per dir, dirs map 1:1 to chips), so it has no unit
  // test — it documents the invariant and guards a future recursive scan.
  const seen = new Set<string>();

  for (const chip of chipDirs) {
    if (chip === AUTH_ONLY_CHIP) {
      throw new Error(
        `chip dir "${AUTH_ONLY_CHIP}" is auth-only and must not carry default firmware`,
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
      const { version } = parseEntry(chip, fileName);
      const key = `${chip}@${version}`;
      if (seen.has(key)) {
        throw new Error(`duplicate firmware (chip, version): ${key}`);
      }
      seen.add(key);

      const buf = readFileSync(join(chipDir, fileName));
      const entry: AuthFirmwareEntry = {
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

      firmwares.push(entry);
    }
  }

  if (firmwares.length === 0) {
    throw new Error(`no firmware .bin files found under "${sourceDir}"`);
  }

  firmwares.sort(
    (a, b) =>
      a.chip.localeCompare(b.chip) || compareVersionDesc(a.version, b.version),
  );

  return { firmwares };
}

function main(): void {
  const baseUrl = process.env.BASE_URL;
  if (!baseUrl) {
    console.error('ERROR: BASE_URL must be set.');
    process.exit(1);
  }
  const sourceDir = process.env.SOURCE_DIR ?? 'auth-firmware';
  const output = process.env.OUTPUT ?? 'auth-firmware.json';

  const manifest = buildManifest(sourceDir, baseUrl);
  writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`, 'utf-8');

  console.log(
    `Generated ${output} with ${manifest.firmwares.length} firmware entr${manifest.firmwares.length === 1 ? 'y' : 'ies'}:`,
  );
  for (const e of manifest.firmwares) {
    console.log(`  ${e.chip} ${e.version} (${e.size} bytes)`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
