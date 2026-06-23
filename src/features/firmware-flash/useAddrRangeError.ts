import { computed, type Ref } from "vue";
import { useI18n } from "vue-i18n";
import { validateAddrRange } from "./hex";

/**
 * Reactive inline validation for a hex address pair.
 *
 * Returns an empty string when either field is empty (no premature error
 * while the user is still typing), the parse error when either is malformed,
 * or the "start > end" message when the range is inverted.
 *
 * The boolean variant of this exists implicitly through canFlash/canErase
 * etc. in the store; this composable produces the user-facing reason.
 */
export function useAddrRangeError(
  start: Ref<string>,
  end: Ref<string>,
): { message: Ref<string> } {
  const { t } = useI18n();

  const message = computed(() => {
    if (!start.value.trim() || !end.value.trim()) return "";
    const err = validateAddrRange(start.value, end.value);
    if (err === "invalid") return t("flash.err.addrInvalid");
    if (err === "startAfterEnd") return t("flash.err.startAfterEnd");
    return "";
  });

  return { message };
}
