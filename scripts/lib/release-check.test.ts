import { describe, expect, it } from 'vitest';
import {
  assertManifestComplete,
  checkAssetCompleteness,
  expectedAssetNames,
  toChinaManifest,
  validateManifest,
  type Manifest,
} from './release-check.js';

const V = '3.0.14';
const BASE = `https://github.com/x/y/releases/download/v${V}`;

function fullManifest(): Manifest {
  const gui = (file: string) => ({ url: `${BASE}/${file}`, signature: 'sig' });
  const cli = (file: string) => ({ url: `${BASE}/${file}`, sha256: 'abc' });
  const port = (file: string) => ({ url: `${BASE}/${file}` });
  return {
    version: V,
    platforms: {
      'linux-x86_64': gui(`tyutool-gui_linux_x86_64_appimage_${V}.AppImage`),
      'linux-aarch64': gui(`tyutool-gui_linux_aarch64_appimage_${V}.AppImage`),
      'darwin-x86_64': gui(`tyutool-gui_macos_universal_update_${V}.app.tar.gz`),
      'darwin-aarch64': gui(`tyutool-gui_macos_universal_update_${V}.app.tar.gz`),
      'windows-x86_64': gui(`tyutool-gui_windows_x86_64_nsis_${V}.exe`),
    },
    cli: {
      'linux-x86_64': cli(`tyutool-cli_linux_x86_64_${V}.tar.gz`),
      'linux-aarch64': cli(`tyutool-cli_linux_aarch64_${V}.tar.gz`),
      'darwin-x86_64': cli(`tyutool-cli_macos_x86_64_${V}.tar.gz`),
      'darwin-aarch64': cli(`tyutool-cli_macos_aarch64_${V}.tar.gz`),
      'windows-x86_64': cli(`tyutool-cli_windows_x86_64_${V}.zip`),
    },
    portable: {
      'linux-x86_64': port(`tyutool-gui_linux_x86_64_portable_${V}.tar.gz`),
      'linux-aarch64': port(`tyutool-gui_linux_aarch64_portable_${V}.tar.gz`),
      'darwin-x86_64': port(`tyutool-gui_macos_universal_portable_${V}.tar.gz`),
      'darwin-aarch64': port(`tyutool-gui_macos_universal_portable_${V}.tar.gz`),
      'windows-x86_64': port(`tyutool-gui_windows_x86_64_portable_${V}.zip`),
    },
  };
}

describe('expectedAssetNames', () => {
  it('lists CLI, installers, portables, updater tarball, and sigs (21 names)', () => {
    const names = expectedAssetNames(V);
    expect(names.length).toBe(21);
    expect(names).toContain(`tyutool-gui_linux_x86_64_appimage_${V}.AppImage.sig`);
    expect(names).toContain(`tyutool-gui_macos_universal_dmg_${V}.dmg`);
  });
});

describe('checkAssetCompleteness', () => {
  it('returns [] when every expected asset is present', () => {
    const assets = new Set(expectedAssetNames(V));
    expect(checkAssetCompleteness(V, assets)).toEqual([]);
  });
  it('flags a missing asset', () => {
    const assets = new Set(expectedAssetNames(V));
    assets.delete(`tyutool-gui_windows_x86_64_nsis_${V}.exe.sig`);
    const errs = checkAssetCompleteness(V, assets);
    expect(errs.some((e) => e.includes('nsis') && e.includes('.sig'))).toBe(true);
  });
});

describe('validateManifest', () => {
  const assets = () => new Set(expectedAssetNames(V));
  it('passes a full manifest (shared darwin basename resolves fine)', () => {
    expect(validateManifest(fullManifest(), V, assets())).toEqual([]);
  });
  it('flags a version mismatch', () => {
    expect(validateManifest({ ...fullManifest(), version: '9.9.9' }, V, assets()).length).toBeGreaterThan(0);
  });
  it('flags a missing platform key', () => {
    const m = fullManifest();
    delete (m.platforms as Record<string, unknown>)['windows-x86_64'];
    expect(validateManifest(m, V, assets()).some((e) => e.includes('windows-x86_64'))).toBe(true);
  });
  it('flags an empty signature', () => {
    const m = fullManifest();
    m.platforms['linux-x86_64'].signature = '';
    expect(validateManifest(m, V, assets()).some((e) => e.includes('signature'))).toBe(true);
  });
  it('flags a url whose basename is not a release asset', () => {
    const m = fullManifest();
    m.cli['linux-x86_64'].url = `${BASE}/ghost-file.tar.gz`;
    expect(validateManifest(m, V, assets()).some((e) => e.includes('ghost-file'))).toBe(true);
  });
});

describe('toChinaManifest', () => {
  const TUYA = 'https://oss.example.com/tyutool/v3.0.14';
  function mirroredManifest(): Manifest {
    const m = fullManifest();
    for (const grp of [m.platforms, m.cli, m.portable]) {
      for (const entry of Object.values(grp)) {
        const mirrors = entry as { url_github?: string; url_tuya?: string };
        mirrors.url_github = entry.url;
        mirrors.url_tuya = `${TUYA}/${entry.url.split('/').pop()}`;
      }
    }
    return m;
  }
  it('replaces every url with its url_tuya and keeps mirror/other fields', () => {
    const china = toChinaManifest(mirroredManifest());
    expect(china.version).toBe(V);
    for (const grp of [china.platforms, china.cli, china.portable]) {
      for (const entry of Object.values(grp)) {
        const mirrors = entry as { url_github?: string; url_tuya?: string };
        expect(entry.url.startsWith(TUYA)).toBe(true);
        expect(entry.url).toBe(mirrors.url_tuya);
        expect(mirrors.url_github?.startsWith('https://github.com/')).toBe(true);
      }
    }
    expect(china.platforms['linux-x86_64'].signature).toBe('sig');
    expect(china.cli['linux-x86_64'].sha256).toBe('abc');
  });
  it('does not mutate the input manifest', () => {
    const m = mirroredManifest();
    toChinaManifest(m);
    expect(m.platforms['linux-x86_64'].url.startsWith('https://github.com/')).toBe(true);
  });
  it('throws when an entry lacks url_tuya', () => {
    const m = mirroredManifest();
    delete (m.cli['windows-x86_64'] as { url_tuya?: string }).url_tuya;
    expect(() => toChinaManifest(m)).toThrow(/url_tuya/);
  });
});

describe('assertManifestComplete', () => {
  it('passes a full manifest and flags a missing portable key', () => {
    expect(assertManifestComplete(fullManifest())).toEqual([]);
    const m = fullManifest();
    delete (m.portable as Record<string, unknown>)['darwin-x86_64'];
    expect(assertManifestComplete(m).length).toBeGreaterThan(0);
  });
});
