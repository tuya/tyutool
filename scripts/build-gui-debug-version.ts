import { getRepoRoot } from './lib/repo-root.js';

const STRICT_SEMVER_RE = /^\d+\.\d+\.\d+$/;
export const debugBuildRepoRoot = getRepoRoot(import.meta.url);

function pad2(value: number): string {
  return String(value).padStart(2, '0');
}

function normalizeRoot(root: string): string {
  return root.replace(/\\/g, '/').replace(/\/+$/, '');
}

export function isStrictSemver(version: string): boolean {
  return STRICT_SEMVER_RE.test(version);
}

export function parseDebugVersionArgs(args: string[]): { version: string } {
  const version = args.find((arg) => !arg.startsWith('-'));

  if (!version) {
    throw new Error('Usage: pnpm build:gui:debug-version <version>');
  }

  if (!isStrictSemver(version)) {
    throw new Error(`Error: "${version}" is not a valid semver (x.y.z)`);
  }

  return { version };
}

export function makeDebugBuildStamp(now: Date): string {
  return [
    now.getUTCFullYear(),
    pad2(now.getUTCMonth() + 1),
    pad2(now.getUTCDate()),
  ].join('') + `-${pad2(now.getUTCHours())}${pad2(now.getUTCMinutes())}${pad2(
    now.getUTCSeconds(),
  )}`;
}

export function isRunnableBundleArtifact(fileName: string): boolean {
  const lower = fileName.toLowerCase();

  if (lower.endsWith('.msi')) {
    return true;
  }

  return lower.endsWith('.exe') && !lower.includes('tyutool_gui');
}

export function getDebugBuildPaths(
  root: string,
  version: string,
  stamp: string,
): { cargoTargetDir: string; outputDir: string } {
  const baseRoot = normalizeRoot(root);

  return {
    cargoTargetDir: `${baseRoot}/.tmp/debug-target/${version}-${stamp}`,
    outputDir: `${baseRoot}/.tmp/debug-builds/${version}-${stamp}`,
  };
}
