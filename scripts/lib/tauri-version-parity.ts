/**
 * Tauri keeps two halves of every plugin: an npm package the WebView imports and
 * a Rust crate the app links. `tauri build` refuses to run when a pair is on
 * different major/minor releases (`check_mismatched_packages` in the Tauri CLI),
 * so a mismatch discovered at release time costs a full matrix of GUI jobs.
 *
 * Dependabot cannot see across ecosystems: the cargo group and the npm group are
 * separate PRs, each green on its own. On 2026-10-19 they bumped
 * `@tauri-apps/plugin-dialog` to 2.8.3 and `tauri-plugin-dialog` to 2.7.3, and the
 * next Release run died before compiling anything. This module re-implements that
 * one comparison so CI catches the pair drifting instead of the release build.
 *
 * Comparison rule is deliberately the CLI's: major and minor must be equal, patch
 * may differ.
 */

/** `@tauri-apps/api` is the JS side of the `tauri` crate; plugins follow `plugin-<x>`. */
export function tauriCrateForNpmPackage(npmName: string): string | null {
  if (npmName === '@tauri-apps/api') return 'tauri';
  const plugin = /^@tauri-apps\/plugin-([a-z0-9-]+)$/.exec(npmName);
  return plugin ? `tauri-plugin-${plugin[1]}` : null;
}

function majorMinor(version: string): string | null {
  const m = /^(\d+)\.(\d+)\.\d+/.exec(version.trim());
  return m ? `${m[1]}.${m[2]}` : null;
}

/**
 * Versions the root importer resolves in `pnpm-lock.yaml`.
 *
 * Read as text rather than parsed as YAML: the repo has no YAML dependency and
 * `pnpm install` is not required for this check. The importer block nests
 * sections at 4 spaces, package names at 6, their `version:` field at 8; the
 * `packages:` / `snapshots:` blocks use `name@version` keys and are skipped by
 * requiring the quoted name to be followed directly by a colon.
 */
export function parsePnpmImporterVersions(lockText: string): Record<string, string> {
  const versions: Record<string, string> = {};
  let inImporters = false;
  let packageName: string | null = null;

  for (const line of lockText.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed === '') continue;

    const indent = line.length - line.trimStart().length;
    if (indent === 0) {
      inImporters = trimmed === 'importers:';
      packageName = null;
      continue;
    }
    if (!inImporters) continue;

    if (indent === 4) {
      // `dependencies:` / `devDependencies:` — the next name starts a new group.
      packageName = null;
      continue;
    }
    if (indent === 6) {
      const name = /^'?([^':]+)'?:$/.exec(trimmed);
      packageName = name ? name[1] : null;
      continue;
    }
    if (indent === 8 && packageName) {
      const version = /^version:\s*(\S+)/.exec(trimmed);
      // Peer suffixes look like `3.5.44(typescript@6.0.3)`; keep the version only.
      if (version) versions[packageName] = version[1].split('(')[0];
    }
  }
  return versions;
}

/** Versions resolved in `Cargo.lock` (`[[package]]` entries). */
export function parseCargoLockVersions(lockText: string): Record<string, string> {
  const versions: Record<string, string> = {};
  let name: string | null = null;

  for (const line of lockText.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed === '[[package]]') {
      name = null;
      continue;
    }
    const field = /^(name|version)\s*=\s*"([^"]*)"/.exec(trimmed);
    if (!field) continue;
    if (field[1] === 'name') {
      name = field[2];
    } else if (name) {
      versions[name] = field[2];
    }
  }
  return versions;
}

/**
 * Pairs that `checkTauriVersionParity` will actually compare: an npm Tauri
 * package with its crate present. Reported by the script so a run that parses
 * nothing is visibly different from a run that checked six pairs.
 */
export function comparedPairs(
  npmVersions: Record<string, string>,
  cargoVersions: Record<string, string>,
): Array<{ npmName: string; crateName: string }> {
  const pairs: Array<{ npmName: string; crateName: string }> = [];
  for (const npmName of Object.keys(npmVersions)) {
    const crateName = tauriCrateForNpmPackage(npmName);
    if (crateName && cargoVersions[crateName]) pairs.push({ npmName, crateName });
  }
  return pairs;
}

/**
 * Declared Tauri npm packages the lockfile parse failed to find.
 *
 * This is the gate's own fail-safe. If a future pnpm lockfile layout stops
 * matching the parser's assumptions, both parsers would return `{}`, every
 * comparison would be skipped and the gate would pass vacuously. Comparing the
 * parse against `package.json` makes that breakage loud instead.
 */
export function missingNpmPackages(
  declaredPackages: string[],
  npmVersions: Record<string, string>,
): string[] {
  return declaredPackages.filter(
    (name) => tauriCrateForNpmPackage(name) !== null && !(name in npmVersions),
  );
}

/**
 * Mismatched pairs, one message each. Empty means every npm-side Tauri package
 * agrees with its crate on major.minor.
 *
 * Only the npm direction is checked: a Rust-only plugin is legitimate
 * (`tauri-plugin-updater` is driven from the backend and has no JS API). A pair
 * is compared only when both halves exist, which is what `tauri build` does too.
 */
export function checkTauriVersionParity(
  npmVersions: Record<string, string>,
  cargoVersions: Record<string, string>,
): string[] {
  const errors: string[] = [];

  for (const [npmName, npmVersion] of Object.entries(npmVersions)) {
    const crateName = tauriCrateForNpmPackage(npmName);
    if (!crateName) continue;

    const crateVersion = cargoVersions[crateName];
    if (!crateVersion) continue;

    const npmMajorMinor = majorMinor(npmVersion);
    const crateMajorMinor = majorMinor(crateVersion);
    if (!npmMajorMinor || !crateMajorMinor) {
      errors.push(`cannot compare ${crateName} v${crateVersion} with ${npmName} v${npmVersion}`);
      continue;
    }
    if (npmMajorMinor !== crateMajorMinor) {
      errors.push(
        `${crateName} (Rust v${crateVersion}) : ${npmName} (npm v${npmVersion})` +
          ' — must share major/minor, or `tauri build` refuses to run',
      );
    }
  }
  return errors;
}
