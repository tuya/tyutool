// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import { i18n } from './index';

describe('i18n instance', () => {
  it('has fallbackLocale set to en', () => {
    expect(i18n.global.fallbackLocale.value).toBe('en');
  });

  it('has zh-CN and en messages loaded', () => {
    const messages = i18n.global.messages.value;
    expect(messages).toHaveProperty('zh-CN');
    expect(messages).toHaveProperty('en');
  });

  it('zh-CN messages contain app namespace', () => {
    const zhCN = i18n.global.messages.value['zh-CN'] as Record<string, unknown>;
    expect(zhCN).toHaveProperty('app');
    expect(zhCN).toHaveProperty('flash');
    expect(zhCN).toHaveProperty('settings');
  });

  it('en messages contain app namespace', () => {
    const en = i18n.global.messages.value['en'] as Record<string, unknown>;
    expect(en).toHaveProperty('app');
    expect(en).toHaveProperty('flash');
    expect(en).toHaveProperty('settings');
  });

  it('can translate a known key', () => {
    const result = i18n.global.t('app.tagline');
    expect(typeof result).toBe('string');
    expect(result.length).toBeGreaterThan(0);
  });
});
