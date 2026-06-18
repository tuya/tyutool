export const GUI_PLATFORM_KEYS = [
  'linux-x86_64',
  'linux-aarch64',
  'darwin-x86_64',
  'darwin-aarch64',
  'windows-x86_64',
] as const;

export interface Manifest {
  version: string;
  platforms: Record<string, { url: string; signature: string }>;
  cli: Record<string, { url: string; sha256: string }>;
  portable: Record<string, { url: string }>;
}

export function expectedAssetNames(v: string): string[] {
  return [
    // CLI ×5 (per-arch real files)
    `tyutool-cli_linux_x86_64_${v}.tar.gz`,
    `tyutool-cli_linux_aarch64_${v}.tar.gz`,
    `tyutool-cli_macos_x86_64_${v}.tar.gz`,
    `tyutool-cli_macos_aarch64_${v}.tar.gz`,
    `tyutool-cli_windows_x86_64_${v}.zip`,
    // GUI installers ×7
    `tyutool-gui_linux_x86_64_deb_${v}.deb`,
    `tyutool-gui_linux_aarch64_deb_${v}.deb`,
    `tyutool-gui_linux_x86_64_appimage_${v}.AppImage`,
    `tyutool-gui_linux_aarch64_appimage_${v}.AppImage`,
    `tyutool-gui_linux_x86_64_rpm_${v}.rpm`,
    `tyutool-gui_macos_universal_dmg_${v}.dmg`,
    `tyutool-gui_windows_x86_64_nsis_${v}.exe`,
    // Portable ×4
    `tyutool-gui_linux_x86_64_portable_${v}.tar.gz`,
    `tyutool-gui_linux_aarch64_portable_${v}.tar.gz`,
    `tyutool-gui_macos_universal_portable_${v}.tar.gz`,
    `tyutool-gui_windows_x86_64_portable_${v}.zip`,
    // macOS updater tarball ×1 (serves both darwin keys)
    `tyutool-gui_macos_universal_update_${v}.app.tar.gz`,
    // Updater signatures ×4 (latest.json platforms all require a signature)
    `tyutool-gui_linux_x86_64_appimage_${v}.AppImage.sig`,
    `tyutool-gui_linux_aarch64_appimage_${v}.AppImage.sig`,
    `tyutool-gui_windows_x86_64_nsis_${v}.exe.sig`,
    `tyutool-gui_macos_universal_update_${v}.app.tar.gz.sig`,
  ];
}

export function checkAssetCompleteness(version: string, assetBasenames: Set<string>): string[] {
  return expectedAssetNames(version)
    .filter((n) => !assetBasenames.has(n))
    .map((n) => `缺少产物: ${n}`);
}

function basename(url: string): string {
  return url.split('/').pop() ?? '';
}

export function validateManifest(m: Manifest, version: string, assetBasenames: Set<string>): string[] {
  const errs: string[] = [];
  if (m.version !== version) errs.push(`manifest version ${m.version} != ${version}`);

  for (const k of GUI_PLATFORM_KEYS) {
    const p = m.platforms?.[k];
    if (!p) {
      errs.push(`platforms 缺少 key: ${k}`);
      continue;
    }
    if (!assetBasenames.has(basename(p.url))) errs.push(`platforms[${k}] url 不在 release 资源中: ${basename(p.url)}`);
    if (!p.signature || p.signature.trim() === '') errs.push(`platforms[${k}] signature 为空`);
  }
  for (const k of GUI_PLATFORM_KEYS) {
    const c = m.cli?.[k];
    if (!c) {
      errs.push(`cli 缺少 key: ${k}`);
      continue;
    }
    if (!assetBasenames.has(basename(c.url))) errs.push(`cli[${k}] url 不在 release 资源中: ${basename(c.url)}`);
  }
  for (const k of GUI_PLATFORM_KEYS) {
    const pt = m.portable?.[k];
    if (!pt) {
      errs.push(`portable 缺少 key: ${k}`);
      continue;
    }
    if (!assetBasenames.has(basename(pt.url))) errs.push(`portable[${k}] url 不在 release 资源中: ${basename(pt.url)}`);
  }
  return errs;
}

export function assertManifestComplete(m: Pick<Manifest, 'platforms' | 'cli' | 'portable'>): string[] {
  const errs: string[] = [];
  for (const grp of ['platforms', 'cli', 'portable'] as const) {
    for (const k of GUI_PLATFORM_KEYS) {
      if (!m[grp]?.[k]) errs.push(`${grp} 缺少平台: ${k}`);
    }
  }
  return errs;
}
