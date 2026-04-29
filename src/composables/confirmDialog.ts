import { reactive } from 'vue';

export type ConfirmDialogKind = 'info' | 'warning' | 'danger';

export interface ConfirmDialogOptions {
  title: string;
  message: string;
  kind?: ConfirmDialogKind;
  okLabel?: string;
  cancelLabel?: string;
  /** When false, only the primary button is shown; backdrop / Esc / close still dismiss as cancel. Default true. */
  showCancel?: boolean;
  /** Optional extra action button (does not close dialog), e.g. copy command. */
  extraActionLabel?: string;
  onExtraAction?: (() => void | Promise<void>) | null;
}

interface ConfirmDialogState extends Required<Omit<ConfirmDialogOptions, 'showCancel'>> {
  visible: boolean;
  showCancel: boolean;
}

const defaultState: ConfirmDialogState = {
  visible: false,
  title: '',
  message: '',
  kind: 'info',
  okLabel: 'OK',
  cancelLabel: 'Cancel',
  showCancel: true,
  extraActionLabel: '',
  onExtraAction: null,
};

/** Global reactive state consumed by `<TyConfirmDialog />` in `App.vue`. */
export const confirmDialogState = reactive<ConfirmDialogState>({ ...defaultState });

let _resolve: ((value: boolean) => void) | null = null;

/**
 * Show a confirm dialog and wait for the user's response.
 *
 * Returns `true` if the user clicked OK, `false` if cancelled (button / ESC / backdrop click).
 *
 * Usage:
 * ```ts
 * const ok = await showConfirmDialog({ title: '...', message: '...' });
 * ```
 */
export function showConfirmDialog(opts: ConfirmDialogOptions): Promise<boolean> {
  // If a previous dialog is still pending, resolve it as cancelled.
  if (_resolve) {
    _resolve(false);
    _resolve = null;
  }

  confirmDialogState.title = opts.title;
  confirmDialogState.message = opts.message;
  confirmDialogState.kind = opts.kind ?? 'info';
  confirmDialogState.okLabel = opts.okLabel ?? 'OK';
  confirmDialogState.cancelLabel = opts.cancelLabel ?? 'Cancel';
  confirmDialogState.showCancel = opts.showCancel ?? true;
  confirmDialogState.extraActionLabel = opts.extraActionLabel ?? '';
  confirmDialogState.onExtraAction = opts.onExtraAction ?? null;
  confirmDialogState.visible = true;

  return new Promise<boolean>(resolve => {
    _resolve = resolve;
  });
}

/** Called by the dialog component when the user confirms. */
export function resolveConfirmDialog(result: boolean): void {
  confirmDialogState.visible = false;
  confirmDialogState.onExtraAction = null;
  if (_resolve) {
    _resolve(result);
    _resolve = null;
  }
}
