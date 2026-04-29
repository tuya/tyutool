/**
 * Generate latest.json update manifest from release artifacts.
 * Env: VERSION, GITHUB_REPO, TAG
 */
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';

import { findFilesUnderArtifacts } from './lib/artifacts-glob.js';

const VERSION = process.env.VERSION;
const GITHUB_REPO = process.env.GITHUB_REPO;
const TAG = process.env.TAG;

if (!VERSION || !GITHUB_REPO || !TAG) {
  console.error('ERROR: VERSION, GITHUB_REPO, and TAG must be set.');
  process.exit(1);
}

const BASE_URL = `https://github.com/${GITHUB_REPO}/releases/download/${TAG}`;

function sha256File(path: string): string {
  const h = createHash('sha256');
  const buf = readFileSync(path);
  h.update(buf);
  return h.digest('hex');
}

function readSig(path: string): string {
  return readFileSync(path, 'utf-8').trim();
}

const GUI_PLATFORM_PATTERNS: Record<string, [string, string][]> = {
  'linux-x86_64': [[`tyutool-gui_linux_x86_64_appimage_${VERSION}.AppImage`, 'linux-x86_64']],
  'linux-aarch64': [[`tyutool-gui_linux_aarch64_appimage_${VERSION}.AppImage`, 'linux-aarch64']],
  'darwin-x86_64': [[`tyutool-gui_macos_universal_update_${VERSION}.app.tar.gz`, 'darwin-x86_64']],
  'darwin-aarch64': [[`tyutool-gui_macos_universal_update_${VERSION}.app.tar.gz`, 'darwin-aarch64']],
  'windows-x86_64': [[`tyutool-gui_windows_x86_64_nsis_${VERSION}.exe`, 'windows-x86_64']],
};

const platforms: Record<string, { url: string; signature: string }> = {};

for (const [platformKey, patterns] of Object.entries(GUI_PLATFORM_PATTERNS)) {
  for (const [filename] of patterns) {
    const sigMatches = findFilesUnderArtifacts(`${filename}.sig`);
    const sigPath = sigMatches[0] ?? `artifacts/${filename}.sig`;
    const assetMatches = findFilesUnderArtifacts(filename);
    if (assetMatches.length > 0 && existsSync(sigPath)) {
      const signature = readSig(sigPath);
      platforms[platformKey] = {
        url: `${BASE_URL}/${filename}`,
        signature,
      };
    } else {
      console.warn(
        `  WARN: ${platformKey}: asset=${assetMatches.length > 0 ? 'found' : 'MISSING'}, sig=${existsSync(sigPath) ? 'found' : 'MISSING'} (${filename})`,
      );
    }
  }
}

const CLI_PATTERNS: Record<string, string> = {
  'linux-x86_64': `tyutool-cli_linux_x86_64_${VERSION}.tar.gz`,
  'linux-aarch64': `tyutool-cli_linux_aarch64_${VERSION}.tar.gz`,
  'darwin-x86_64': `tyutool-cli_macos_x86_64_${VERSION}.tar.gz`,
  'darwin-aarch64': `tyutool-cli_macos_aarch64_${VERSION}.tar.gz`,
  'windows-x86_64': `tyutool-cli_windows_x86_64_${VERSION}.zip`,
};

const cli: Record<string, { url: string; sha256: string }> = {};

for (const [platformKey, filename] of Object.entries(CLI_PATTERNS)) {
  const matches = findFilesUnderArtifacts(filename);
  if (matches.length > 0) {
    const path = matches[0];
    cli[platformKey] = {
      url: `${BASE_URL}/${filename}`,
      sha256: sha256File(path),
    };
  }
}

const PORTABLE_PATTERNS: Record<string, string> = {
  'linux-x86_64': `tyutool-gui_linux_x86_64_portable_${VERSION}.tar.gz`,
  'linux-aarch64': `tyutool-gui_linux_aarch64_portable_${VERSION}.tar.gz`,
  'darwin-x86_64': `tyutool-gui_macos_universal_portable_${VERSION}.tar.gz`,
  'darwin-aarch64': `tyutool-gui_macos_universal_portable_${VERSION}.tar.gz`,
  'windows-x86_64': `tyutool-gui_windows_x86_64_portable_${VERSION}.zip`,
};

const portable: Record<string, { url: string }> = {};

for (const [platformKey, filename] of Object.entries(PORTABLE_PATTERNS)) {
  const matches = findFilesUnderArtifacts(filename);
  if (matches.length > 0) {
    portable[platformKey] = {
      url: `${BASE_URL}/${filename}`,
    };
  }
}

const now = new Date();
const pubDate = `${now.toISOString().slice(0, 19)}Z`;

const manifest = {
  version: VERSION,
  notes: `tyutool ${VERSION}`,
  pub_date: pubDate,
  platforms,
  cli,
  portable,
};

writeFileSync('latest.json', `${JSON.stringify(manifest, null, 2)}\n`, 'utf-8');

console.log(`Generated latest.json for v${VERSION}`);
console.log(`  GUI platforms: ${Object.keys(platforms).join(', ')}`);
console.log(`  CLI platforms: ${Object.keys(cli).join(', ')}`);
console.log(`  Portable:      ${Object.keys(portable).join(', ')}`);
