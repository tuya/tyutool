import { afterEach, describe, expect, it, vi } from 'vitest';
import { isNewerVersion, UPDATE_SOURCES } from './update-sources';

describe('isNewerVersion', () => {
  it('returns true for major version upgrade', () => {
    expect(isNewerVersion('2.0.0', '1.0.0')).toBe(true);
  });

  it('returns true for minor version upgrade', () => {
    expect(isNewerVersion('1.2.0', '1.1.0')).toBe(true);
  });

  it('returns true for patch version upgrade', () => {
    expect(isNewerVersion('1.0.2', '1.0.1')).toBe(true);
  });

  it('returns false for same version', () => {
    expect(isNewerVersion('1.0.0', '1.0.0')).toBe(false);
  });

  it('returns false for older remote version', () => {
    expect(isNewerVersion('1.0.0', '2.0.0')).toBe(false);
    expect(isNewerVersion('1.0.0', '1.1.0')).toBe(false);
    expect(isNewerVersion('1.0.0', '1.0.1')).toBe(false);
  });

  it('handles v prefix', () => {
    expect(isNewerVersion('v2.0.0', 'v1.0.0')).toBe(true);
    expect(isNewerVersion('v1.0.0', '1.0.0')).toBe(false);
    expect(isNewerVersion('1.0.1', 'v1.0.0')).toBe(true);
  });

  it('handles missing segments by treating as 0', () => {
    expect(isNewerVersion('1.1', '1.0.0')).toBe(true);
    expect(isNewerVersion('1.0.0', '1.1')).toBe(false);
  });

  it('handles real-world version strings', () => {
    expect(isNewerVersion('0.1.18', '0.1.17')).toBe(true);
    expect(isNewerVersion('0.1.17', '0.1.17')).toBe(false);
    expect(isNewerVersion('0.2.0', '0.1.17')).toBe(true);
  });
});

describe('UPDATE_SOURCES', () => {
  it('has at least one source', () => {
    expect(UPDATE_SOURCES.length).toBeGreaterThan(0);
  });

  it('each source has required fields', () => {
    for (const source of UPDATE_SOURCES) {
      expect(source.id).toEqual(expect.any(String));
      expect(source.labelKey).toEqual(expect.any(String));
      expect(source.url).toMatch(/^https:\/\//);
      expect(source.releasePageUrl).toMatch(/^https:\/\//);
    }
  });

  it('includes github and gitee sources', () => {
    const ids = UPDATE_SOURCES.map(s => s.id);
    expect(ids).toContain('github');
    expect(ids).toContain('gitee');
  });

  it('uses source-specific release pages', () => {
    const github = UPDATE_SOURCES.find(s => s.id === 'github');
    const gitee = UPDATE_SOURCES.find(s => s.id === 'gitee');

    expect(github?.releasePageUrl).toBe('https://github.com/tuya/tyutool/releases/latest');
    expect(gitee?.releasePageUrl).toBe('https://gitee.com/tuya-open/tyutool/releases');
  });
});

describe('fetchLatestJson (browser mode)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('fetches and parses JSON via fetch API', async () => {
    const mockJson = {
      version: '1.0.0',
      notes: 'test',
      pub_date: '2026-01-01',
      platforms: {},
      cli: {},
    };

    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(mockJson),
      })
    );

    // Dynamic import to avoid module-level Tauri detection issues
    const { fetchLatestJson } = await import('./update-sources');
    const result = await fetchLatestJson('https://example.com/latest.json');
    expect(result.version).toBe('1.0.0');
  });

  it('throws on HTTP error', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: false,
        status: 404,
      })
    );

    const { fetchLatestJson } = await import('./update-sources');
    await expect(fetchLatestJson('https://example.com/latest.json')).rejects.toThrow('HTTP 404');
  });
});
