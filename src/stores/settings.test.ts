import { afterEach, describe, expect, it, vi } from 'vitest';
import { resolveLocale } from './settings';

describe('resolveLocale', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns zh-CN when preference is auto and browser language starts with zh', () => {
    vi.stubGlobal('navigator', { language: 'zh-CN' });
    expect(resolveLocale('auto')).toBe('zh-CN');
  });

  it('returns zh-CN for zh-TW browser language', () => {
    vi.stubGlobal('navigator', { language: 'zh-TW' });
    expect(resolveLocale('auto')).toBe('zh-CN');
  });

  it('returns en when preference is auto and browser language is en-US', () => {
    vi.stubGlobal('navigator', { language: 'en-US' });
    expect(resolveLocale('auto')).toBe('en');
  });

  it('returns en when preference is auto and browser language is ja', () => {
    vi.stubGlobal('navigator', { language: 'ja' });
    expect(resolveLocale('auto')).toBe('en');
  });

  it('returns en when preference is auto and navigator.language is empty', () => {
    vi.stubGlobal('navigator', { language: '' });
    expect(resolveLocale('auto')).toBe('en');
  });

  it('returns zh-CN when preference is zh-CN', () => {
    expect(resolveLocale('zh-CN')).toBe('zh-CN');
  });

  it('returns en when preference is en', () => {
    expect(resolveLocale('en')).toBe('en');
  });
});
