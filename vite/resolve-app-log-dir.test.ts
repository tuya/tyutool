import { describe, expect, it } from 'vitest';
import { resolveAppLogDirAbsolute } from './resolve-app-log-dir';

describe('resolveAppLogDirAbsolute', () => {
  it('returns a path containing the app identifier', () => {
    const dir = resolveAppLogDirAbsolute();
    expect(dir).toContain('com.tyutool.desktop');
    expect(dir).toContain('logs');
  });
});
