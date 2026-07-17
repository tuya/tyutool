/** localStorage key recording whether the operator dismissed the batch-auth
 *  disclaimer with "don't show again".
 *
 *  - Written by `BatchFlashAuthPage` when the user confirms + checks the box.
 *  - Read by `BatchFlashAuthPage` on mount to decide whether to show the modal.
 *  - Cleared by the Settings → About "show the risk notice again" button.
 *
 *  Shared so the two surfaces can never drift on the key name. */
export const DISCLAIMER_KEY = "tyutool-batch-auth-disclaimer-v1";
