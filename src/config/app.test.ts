import { describe, expect, it } from 'vitest';
import { APP_VERSION } from './app';

describe('APP_VERSION', () => {
  it('is a non-empty string', () => {
    expect(typeof APP_VERSION).toBe('string');
    expect(APP_VERSION.length).toBeGreaterThan(0);
  });

  it('looks like a semver version', () => {
    expect(APP_VERSION).toMatch(/^\d+\.\d+\.\d+/);
  });
});
