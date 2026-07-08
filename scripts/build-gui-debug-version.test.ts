import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  assertSupportedDebugBuildPlatform,
  copyRunnableArtifacts,
  getDebugBuildPaths,
  isRunnableBundleArtifact,
  isStrictSemver,
  makeDebugBuildStamp,
  parseDebugVersionArgs,
  restoreFileSnapshots,
} from './build-gui-debug-version.js';

describe('parseDebugVersionArgs', () => {
  it('accepts a strict semver positional argument', () => {
    expect(parseDebugVersionArgs(['0.0.1'])).toEqual({ version: '0.0.1' });
  });

  it('rejects a missing version', () => {
    expect(() => parseDebugVersionArgs([])).toThrow(
      'Usage: pnpm build:gui:debug-version <version>',
    );
  });

  it('rejects non-semver values', () => {
    expect(() => parseDebugVersionArgs(['0.0'])).toThrow(
      'Error: "0.0" is not a valid semver (x.y.z)',
    );
  });
});

describe('isStrictSemver', () => {
  it('accepts x.y.z only', () => {
    expect(isStrictSemver('1.2.3')).toBe(true);
    expect(isStrictSemver('1.2')).toBe(false);
    expect(isStrictSemver('v1.2.3')).toBe(false);
  });
});

describe('makeDebugBuildStamp', () => {
  it('formats a stable timestamp for directory names', () => {
    expect(makeDebugBuildStamp(new Date('2026-07-08T09:10:11Z'))).toBe(
      '20260708-091011',
    );
  });
});

describe('isRunnableBundleArtifact', () => {
  it('keeps runnable Windows artifacts only', () => {
    expect(isRunnableBundleArtifact('tyutool_0.0.1_x64-setup.exe')).toBe(true);
    expect(isRunnableBundleArtifact('tyutool_0.0.1_x64_en-US.msi')).toBe(true);
    expect(isRunnableBundleArtifact('tyutool_gui.exe')).toBe(false);
    expect(isRunnableBundleArtifact('tyutool_gui.pdb')).toBe(false);
    expect(isRunnableBundleArtifact('latest.json')).toBe(false);
  });
});

describe('getDebugBuildPaths', () => {
  it('isolates target and copied outputs under .tmp', () => {
    expect(
      getDebugBuildPaths('D:/repo', '0.0.1', '20260708-091011'),
    ).toEqual({
      cargoTargetDir: 'D:/repo/.tmp/debug-target/0.0.1-20260708-091011',
      outputDir: 'D:/repo/.tmp/debug-builds/0.0.1-20260708-091011',
    });
  });
});

describe('copyRunnableArtifacts', () => {
  it('copies only runnable bundle outputs into the debug directory', () => {
    const root = mkdtempSync(join(tmpdir(), 'tyutool-debug-build-'));
    const bundleRoot = join(root, 'bundle');
    const outputDir = join(root, 'copied');
    const setupArtifact = join(bundleRoot, 'nsis', 'tyutool_0.0.1_x64-setup.exe');
    const msiArtifact = join(bundleRoot, 'msi', 'tyutool_0.0.1_x64_en-US.msi');
    const helperArtifact = join(bundleRoot, 'nsis', 'tyutool_0.0.1_x64-helper.exe');
    const pdbArtifact = join(bundleRoot, 'nsis', 'tyutool_gui.pdb');
    const copiedSetupArtifact = join(outputDir, 'tyutool_0.0.1_x64-setup.exe');
    const copiedMsiArtifact = join(outputDir, 'tyutool_0.0.1_x64_en-US.msi');

    mkdirSync(join(bundleRoot, 'nsis'), { recursive: true });
    mkdirSync(join(bundleRoot, 'msi'), { recursive: true });

    writeFileSync(setupArtifact, 'setup-ok');
    writeFileSync(msiArtifact, 'msi-ok');
    writeFileSync(helperArtifact, 'skip');
    writeFileSync(pdbArtifact, 'skip');

    expect(copyRunnableArtifacts(bundleRoot, outputDir)).toEqual([
      'tyutool_0.0.1_x64-setup.exe',
      'tyutool_0.0.1_x64_en-US.msi',
    ]);

    expect(existsSync(copiedSetupArtifact)).toBe(true);
    expect(existsSync(copiedMsiArtifact)).toBe(true);
    expect(existsSync(join(outputDir, 'tyutool_0.0.1_x64-helper.exe'))).toBe(false);
    expect(existsSync(join(outputDir, 'tyutool_gui.pdb'))).toBe(false);
    expect(readFileSync(copiedSetupArtifact, 'utf-8')).toBe('setup-ok');
    expect(readFileSync(copiedMsiArtifact, 'utf-8')).toBe('msi-ok');
  });
});

describe('assertSupportedDebugBuildPlatform', () => {
  it('accepts Windows', () => {
    expect(() => assertSupportedDebugBuildPlatform('win32')).not.toThrow();
  });

  it('rejects non-Windows platforms with a clear error', () => {
    expect(() => assertSupportedDebugBuildPlatform('linux')).toThrow(
      'Error: build:gui:debug-version currently supports Windows packaging only.',
    );
  });
});

describe('restoreFileSnapshots', () => {
  it('attempts every restore and reports aggregate cleanup failures afterward', () => {
    const root = mkdtempSync(join(tmpdir(), 'tyutool-debug-restore-'));
    const restoredFile = join(root, 'restored.txt');
    const brokenFile = join(root, 'missing', 'broken.txt');

    writeFileSync(restoredFile, 'mutated');

    expect(() =>
      restoreFileSnapshots([
        { path: brokenFile, contents: 'broken', encoding: 'utf-8' },
        { path: restoredFile, contents: 'original', encoding: 'utf-8' },
      ]),
    ).toThrow('Failed to restore 1 file(s) after debug build cleanup.');

    expect(readFileSync(restoredFile, 'utf-8')).toBe('original');
  });
});
