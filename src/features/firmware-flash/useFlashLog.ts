import { nextTick, ref, watch } from "vue";
import { i18n } from "@/i18n";
import { APP_VERSION } from "@/config/app";

const t = i18n.global.t;

/** Log buffer + autoscroll for the flash store. Owns its own refs; the store
 *  destructures them back so existing call sites stay unchanged. */
export function useFlashLog() {
  const logLines = ref<string[]>([
    `[${t("flash.log.readyTag")}] ${t("flash.log.appInfo", { version: APP_VERSION })} — ${t("flash.log.waiting")}`,
  ]);
  const logScrollRef = ref<HTMLDivElement | null>(null);
  const lockAutoScroll = ref(false);

  function appendLog(line: string): void {
    const ts = new Date().toLocaleTimeString([], { hour12: false });
    logLines.value.push(`[${ts}] ${line}`);
    if (logLines.value.length > 500) {
      logLines.value.splice(0, logLines.value.length - 500);
    }
  }

  async function scrollLogToBottom(): Promise<void> {
    await nextTick();
    const el = logScrollRef.value;
    if (!el || lockAutoScroll.value) {
      return;
    }
    el.scrollTop = el.scrollHeight;
  }

  function clearLogs(): void {
    logLines.value = [];
    appendLog(t("flash.log.cleared"));
  }

  async function copyLogs(): Promise<void> {
    const text = logLines.value.join("\n");
    try {
      await navigator.clipboard.writeText(text);
      appendLog(t("flash.log.copied"));
    } catch {
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
  };
}
