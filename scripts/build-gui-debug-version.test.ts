import { describe, expect, it } from 'vitest';

import {
  getDebugBuildPaths,
  isRunnableBundleArtifact,
  isStrictSemver,
  makeDebugBuildStamp,
  parseDebugVersionArgs,
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
