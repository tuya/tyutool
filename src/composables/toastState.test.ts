import { describe, expect, it } from 'vitest';
import { toastState } from './toastState';

describe('toastState', () => {
  it('has visible initialized to false', () => {
    expect(toastState.visible).toBe(false);
  });

  it('has version initialized to empty string', () => {
    expect(toastState.version).toBe('');
  });

  it('is reactive (can be mutated)', () => {
    const prev = toastState.visible;
    toastState.visible = true;
    expect(toastState.visible).toBe(true);
    // Restore
    toastState.visible = prev;
  });
});
