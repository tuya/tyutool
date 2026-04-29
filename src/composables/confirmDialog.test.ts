import { describe, expect, it } from 'vitest';
import { confirmDialogState, resolveConfirmDialog, showConfirmDialog } from './confirmDialog';

describe('showConfirmDialog', () => {
  it('sets dialog state and returns a promise', () => {
    const p = showConfirmDialog({
      title: 'Test Title',
      message: 'Test Message',
      kind: 'warning',
      okLabel: 'Yes',
      cancelLabel: 'No',
    });

    expect(confirmDialogState.visible).toBe(true);
    expect(confirmDialogState.title).toBe('Test Title');
    expect(confirmDialogState.message).toBe('Test Message');
    expect(confirmDialogState.kind).toBe('warning');
    expect(confirmDialogState.okLabel).toBe('Yes');
    expect(confirmDialogState.cancelLabel).toBe('No');
    expect(confirmDialogState.showCancel).toBe(true);
    expect(p).toBeInstanceOf(Promise);

    // Clean up: resolve the promise so it doesn't leak
    resolveConfirmDialog(false);
  });

  it('applies default kind, okLabel, cancelLabel', () => {
    showConfirmDialog({ title: 'T', message: 'M' });

    expect(confirmDialogState.kind).toBe('info');
    expect(confirmDialogState.okLabel).toBe('OK');
    expect(confirmDialogState.cancelLabel).toBe('Cancel');
    expect(confirmDialogState.showCancel).toBe(true);

    resolveConfirmDialog(false);
  });

  it('honors showCancel false', () => {
    showConfirmDialog({ title: 'T', message: 'M', showCancel: false });
    expect(confirmDialogState.showCancel).toBe(false);
    resolveConfirmDialog(false);
  });
});

describe('resolveConfirmDialog', () => {
  it('resolves promise with true on confirm', async () => {
    const p = showConfirmDialog({ title: 'T', message: 'M' });
    resolveConfirmDialog(true);
    const result = await p;
    expect(result).toBe(true);
    expect(confirmDialogState.visible).toBe(false);
  });

  it('resolves promise with false on cancel', async () => {
    const p = showConfirmDialog({ title: 'T', message: 'M' });
    resolveConfirmDialog(false);
    const result = await p;
    expect(result).toBe(false);
  });

  it('auto-cancels previous dialog when a new one is shown', async () => {
    const p1 = showConfirmDialog({ title: 'First', message: 'M' });
    const p2 = showConfirmDialog({ title: 'Second', message: 'M' });

    // p1 should have been auto-resolved as false
    const r1 = await p1;
    expect(r1).toBe(false);

    // p2 is still open
    expect(confirmDialogState.title).toBe('Second');
    expect(confirmDialogState.visible).toBe(true);

    resolveConfirmDialog(true);
    const r2 = await p2;
    expect(r2).toBe(true);
  });
});
