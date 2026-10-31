import { nextTick, ref, watch } from "vue";
import { i18n } from "@/i18n";
import { APP_VERSION } from "@/config/app";
import { useCopyFeedback } from "@/composables/useCopyFeedback";

const t = i18n.global.t;

/** Log buffer + autoscroll for the flash store. Owns its own refs; the store
 *  destructures them back so existing call sites stay unchanged. */
export function useFlashLog() {
  const logLines = ref<string[]>([
    `[${t("flash.log.readyTag")}] ${t("flash.log.appInfo", { version: APP_VERSION })} — ${t("flash.log.waiting")}`,
  ]);
  const logScrollRef = ref<HTMLDivElement | null>(null);
  const lockAutoScroll = ref(false);
  let pendingScroll: Promise<void> | null = null;
  const {
    state: copyState,
    message: copyMessage,
    copy,
    reset: resetCopy,
  } = useCopyFeedback();

  function appendLog(line: string): void {
    const ts = new Date().toLocaleTimeString([], { hour12: false });
    logLines.value.push(`[${ts}] ${line}`);
    if (logLines.value.length > 500) {
      logLines.value.splice(0, logLines.value.length - 500);
    }
    void scrollLogToBottom();
  }

  function scrollLogToBottom(): Promise<void> {
    if (!pendingScroll) {
      pendingScroll = nextTick().then(() => {
        pendingScroll = null;
        const el = logScrollRef.value;
        if (el && !lockAutoScroll.value) el.scrollTop = el.scrollHeight;
      });
    }
    return pendingScroll;
  }

  function clearLogs(): void {
    resetCopy();
    logLines.value = [];
    appendLog(t("flash.log.cleared"));
  }

  async function copyLogs(): Promise<void> {
    if (copyState.value === "copying") return;
    await copy(logLines.value.join("\n"));
    if (copyState.value === "copied") {
      appendLog(t("flash.log.copied"));
    } else if (copyState.value === "failed") {
      appendLog(t("flash.log.copyFailed"));
    }
  }

  watch(
    () => logLines.value.length,
    () => {
      void scrollLogToBottom();
    },
  );

  return {
    logLines,
    logScrollRef,
    lockAutoScroll,
    appendLog,
    clearLogs,
    copyLogs,
    copyState,
    copyMessage,
  };
}
