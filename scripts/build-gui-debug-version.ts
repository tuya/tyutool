#!/usr/bin/env tsx
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from 'node:fs';
import { join, relative, resolve } from 'node:path';

import { getRepoRoot } from './lib/repo-root.js';
import { run } from './lib/run.js';

const STRICT_SEMVER_RE = /^\d+\.\d+\.\d+$/;
export const debugBuildRepoRoot = getRepoRoot(import.meta.url);
const ROOT = debugBuildRepoRoot;
const TAURI_CARGO = resolve(ROOT, 'src-tauri', 'Cargo.toml');
const TAURI_CONF = resolve(ROOT, 'src-tauri', 'tauri.conf.json');

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

function collectFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = join(dir, entry.name);
    return entry.isDirectory() ? collectFiles(fullPath) : [fullPath];
  });
}

function runOrThrow(cmd: string, args: string[], env: NodeJS.ProcessEnv): void {
  const originalExit = process.exit;

  process.exit = ((code?: number | string | null | undefined) => {
    throw new Error(`Command failed with exit code ${code ?? 1}: ${cmd} ${args.join(' ')}`);
  }) as typeof process.exit;

  try {
    run(cmd, args, { cwd: ROOT, env });
  } finally {
    process.exit = originalExit;
  }
}

function getPnpmCommand(args: string[]): { cmd: string; args: string[] } {
  const execPath = process.env.npm_execpath;

  if (execPath && execPath.toLowerCase().includes('pnpm')) {
    return {
      cmd: process.execPath,
      args: [execPath, ...args],
    };
  }

  return {
    cmd: 'pnpm',
    args,
  };
}

export function copyRunnableArtifacts(bundleRoot: string, outputDir: string): string[] {
  if (!existsSync(bundleRoot)) {
    throw new Error(`Bundle output not found: ${bundleRoot}`);
  }

  mkdirSync(outputDir, { recursive: true });
  const copied = collectFiles(bundleRoot)
    .filter((file) => isRunnableBundleArtifact(file))
    .map((file) => {
      const fileName = file.split(/[/\\]/).at(-1);

      if (!fileName) {
        throw new Error(`Unable to resolve artifact file name: ${file}`);
      }

      cpSync(file, join(outputDir, fileName), { force: true });
      return fileName;
    })
    .sort();

  if (copied.length === 0) {
    throw new Error(`No runnable bundle artifacts found under ${bundleRoot}`);
  }

  return copied;
}

export function main(args = process.argv.slice(2)): void {
  const { version } = parseDebugVersionArgs(args);
  const stamp = makeDebugBuildStamp(new Date());
  const { cargoTargetDir, outputDir } = getDebugBuildPaths(ROOT, version, stamp);
  const cargoTomlBefore = readFileSync(TAURI_CARGO, 'utf-8');
  const tauriConfBefore = readFileSync(TAURI_CONF, 'utf-8');
  const sharedEnv = { ...process.env, APP_VERSION: version };

  console.log(`==> tyutool debug GUI build: version ${version}`);
  console.log(`==> target dir: ${relative(ROOT, cargoTargetDir)}`);
  console.log(`==> output dir: ${relative(ROOT, outputDir)}`);

  try {
    writeFileSync(
      TAURI_CARGO,
      cargoTomlBefore.replace(/^version\s*=\s*"[^"]*"/m, `version = "${version}"`),
      'utf-8',
    );

    const tauriConf = JSON.parse(tauriConfBefore) as Record<string, unknown>;
    tauriConf.version = version;
    const bundle = tauriConf.bundle as Record<string, unknown> | undefined;
    if (bundle) {
      bundle.createUpdaterArtifacts = false;
    }
    writeFileSync(TAURI_CONF, `${JSON.stringify(tauriConf, null, 2)}\n`, 'utf-8');

    const buildCommand = getPnpmCommand(['run', 'build']);
    runOrThrow(buildCommand.cmd, buildCommand.args, sharedEnv);

    const tauriBuildCommand = getPnpmCommand(['exec', 'tauri', 'build']);
    runOrThrow(tauriBuildCommand.cmd, tauriBuildCommand.args, {
      ...sharedEnv,
      CARGO_TARGET_DIR: cargoTargetDir,
    });

    const copied = copyRunnableArtifacts(join(cargoTargetDir, 'release', 'bundle'), outputDir);
    console.log(`==> copied artifacts: ${copied.join(', ')}`);
  } finally {
    writeFileSync(TAURI_CARGO, cargoTomlBefore, 'utf-8');
    writeFileSync(TAURI_CONF, tauriConfBefore, 'utf-8');
    console.log('==> restored temporary version overrides');
  }
}

const entryScript = process.argv[1] ? resolve(process.argv[1]).replace(/\\/g, '/') : '';

if (
  entryScript.endsWith('/scripts/build-gui-debug-version.ts') ||
  entryScript.endsWith('/scripts/build-gui-debug-version.js')
) {
  main();
}
