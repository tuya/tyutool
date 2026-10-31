import { computed, onScopeDispose, ref } from "vue";
import { i18n } from "@/i18n";

/** Short local feedback, without writing copied content to diagnostic logs. */
export function useCopyFeedback() {
  const { t } = i18n.global;
  const state = ref<"idle" | "copying" | "copied" | "failed">("idle");
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const message = computed(() =>
    state.value === "copied"
      ? t("common.copied")
      : state.value === "failed"
        ? t("common.copyFailed")
        : "",
  );
  async function copy(text: string): Promise<void> {
    if (state.value === "copying") return;
    const request = ++generation;
    clearTimeout(timer);
    state.value = "copying";
    try {
      await navigator.clipboard.writeText(text);
      if (request !== generation) return;
      state.value = "copied";
      timer = setTimeout(() => {
        state.value = "idle";
      }, 1800);
    } catch {
      if (request === generation) state.value = "failed";
    }
  }
  function reset(): void {
    generation++;
    clearTimeout(timer);
    state.value = "idle";
  }
  onScopeDispose(reset);
  return { state, message, copy, reset };
}
