import { ref } from "vue";
import { i18n } from "@/i18n";
import { showConfirmDialog } from "@/composables/confirmDialog";
import { rLog } from "@/utils/log";
import { PHASE_STYLES, phaseKey } from "@/features/firmware-flash/phase-styles";
import type { FlashPhase, OpKind } from "@/features/firmware-flash/types";
import type { FlashProgressPayload } from "@/features/firmware-flash/flash-ipc-types";

const t = i18n.global.t;

/** Maps a backend error message to a user-facing string. */
function mapBackendUserMessage(raw: string | undefined): string {
  return raw?.trim() ?? "";
}

export interface FlashProgressDeps {
  appendLog: (line: string) => void;
  logOperationDuration: () => void;
  /** Teardown after an operation settles (success/error/cancel): release the
   *  serial port and sync connection state. Lives in the store. */
  onOperationSettled: () => void;
  /** Returns current authorize UUID input value (for conflict dialog). */
  getAuthorizeUuid: () => string;
  /** Returns current authorize AuthKey input value (for conflict dialog). */
  getAuthorizeAuthKey: () => string;
  /** Send user's overwrite confirmation response to backend. */
  sendAuthorizeConfirm: (confirmed: boolean) => Promise<void>;
}

/** Flash progress/phase state + the backend progress-event reducer. Owns its
 *  own refs; the store destructures them back so existing call sites stay
 *  unchanged. Cross-subsystem effects (connection teardown) are injected. */
export function useFlashProgress(deps: FlashProgressDeps) {
  const flashProgress = ref(0);
  const flashPhase = ref<FlashPhase>("idle");
  const flashMessage = ref("");
  const runningOp = ref<OpKind | null>(null);

  // Phase-aware progress tracking
  const currentBackendPhase = ref<string | null>(null);
  const phaseProgress = ref(0);
  const phaseIndeterminate = ref(false);
  /** True while a read-only auth operation started by startAuthRead() is in flight. */
  const authOpIsRead = ref(false);

  let indeterminateTimer: ReturnType<typeof setTimeout> | null = null;

  function scheduleIndeterminateCheck(): void {
    cancelIndeterminateCheck();
    indeterminateTimer = setTimeout(() => {
      if (currentBackendPhase.value !== null) {
        phaseIndeterminate.value = true;
      }
    }, 2000);
  }

  function cancelIndeterminateCheck(): void {
    if (indeterminateTimer !== null) {
      clearTimeout(indeterminateTimer);
      indeterminateTimer = null;
    }
  }

  function handleFlashProgressPayload(p: FlashProgressPayload): void {
    if (p.kind === "job_summary") {
      return;
    }

    if (p.kind === "percent") {
      flashProgress.value = Math.min(100, Math.max(0, p.value));
      // Only update phase progress when we're in a registered phase
      if (currentBackendPhase.value !== null) {
        phaseIndeterminate.value = false;
        cancelIndeterminateCheck();
        phaseProgress.value = Math.min(100, Math.max(0, p.value));
      }
      return;
    }

    if (p.kind === "phase") {
      const key = phaseKey(p.phase);
      if (key in PHASE_STYLES) {
        currentBackendPhase.value = key;
        phaseProgress.value = 0;
        phaseIndeterminate.value = false;
        scheduleIndeterminateCheck();
      }
      // Unregistered phases (write_segment, handshake, etc.): ignore, keep current bar state
      return;
    }

    if (p.kind === "milestone") {
      const m = p.milestone;
      if (typeof m === "object" && "auth_read_complete" in m) {
        const { uuid, authkey } = m.auth_read_complete;
        const copyText = `UUID:${uuid}\nAuthKey:${authkey}`;
        void showConfirmDialog({
          title: t("flash.confirm.authReadTitle"),
          message: t("flash.confirm.authReadBody", { uuid, authkey }),
          kind: "info",
          extraActionLabel: t("flash.confirm.authReadCopyCmd"),
          onExtraAction: async () => {
            try {
              await navigator.clipboard.writeText(copyText);
              deps.appendLog(t("flash.log.authReadCopied"));
            } catch {
              deps.appendLog(t("flash.log.copyFailed"));
            }
          },
          okLabel: t("flash.confirm.authReadOk"),
          showCancel: false,
        });
        deps.appendLog(t("flash.log.authReadShown"));
        return;
      }
      if (m === "auth_read_empty") {
        void showConfirmDialog({
          title: t("flash.confirm.authReadEmptyTitle"),
          message: t("flash.confirm.authReadEmptyBody"),
          kind: "warning",
          okLabel: t("flash.confirm.authReadEmptyOk"),
          showCancel: false,
        });
        deps.appendLog(t("flash.log.authReadEmpty"));
        return;
      }
      if (typeof m === "object" && "auth_conflict" in m) {
        const {
          existing_uuid: existingUuid,
          existing_authkey: existingAuthkey,
        } = m.auth_conflict;
        const nu = deps.getAuthorizeUuid();
        const nk = deps.getAuthorizeAuthKey();
        void (async () => {
          const confirmed = await showConfirmDialog({
            title: t("flash.confirm.authOverwriteTitle"),
            message: t("flash.confirm.authOverwriteBody", {
              existingUuid,
              existingAuthkey,
              newUuid: nu,
              newAuthkey: nk,
            }),
            kind: "warning",
            okLabel: t("flash.confirm.authOverwriteOk"),
            cancelLabel: t("flash.confirm.authOverwriteCancel"),
          });
          deps.appendLog(
            confirmed
              ? t("flash.log.authOverwritePrompt")
              : t("flash.log.authOverwriteCancelled"),
          );
          await deps.sendAuthorizeConfirm(confirmed);
        })();
        return;
      }
      const milestoneKey = typeof m === "string" ? m : Object.keys(m)[0];
      const i18nKey = `flash.log.milestone.${milestoneKey}`;
      deps.appendLog(
        i18n.global.te(i18nKey) ? t(i18nKey) : `[${milestoneKey}]`,
      );
      return;
    }

    if (p.kind === "warning") {
      deps.appendLog(`⚠ ${p.message}`);
      return;
    }

    if (p.kind === "done") {
      cancelIndeterminateCheck();
      phaseIndeterminate.value = false;
      currentBackendPhase.value = null;
      const op = runningOp.value;
      runningOp.value = null;
      authOpIsRead.value = false;

      const result = p.result;
      if ("ok" in result) {
        flashPhase.value = "success";
        flashProgress.value = 100;
        if (op === "flash") {
          flashMessage.value = t("flash.msg.flashDone");
          deps.appendLog(t("flash.log.verifyOk"));
        } else if (op === "erase") {
          flashMessage.value = t("flash.msg.eraseDone");
          deps.appendLog(t("flash.log.eraseDoneLog"));
        } else if (op === "read") {
          flashMessage.value = t("flash.msg.readDone");
          deps.appendLog(t("flash.log.readDoneLog"));
        } else if (op === "authorize") {
          flashMessage.value = t("flash.msg.authDone");
          deps.appendLog(t("flash.log.authOkLog"));
        }
        rLog.info(
          `[Flash] Operation '${op}' completed in ${result.ok.elapsed_secs.toFixed(1)}s`,
        );
      } else if ("cancelled" in result) {
        flashPhase.value = "error";
        flashMessage.value = t("flash.msg.cancelled");
        rLog.info(`[Flash] Operation '${op}' cancelled`);
      } else {
        flashPhase.value = "error";
        const displayMsg = result.err.message
          ? mapBackendUserMessage(result.err.message)
          : t("flash.err.withMsg", { msg: "unknown" });
        flashMessage.value = displayMsg;
        deps.appendLog(t("flash.err.withMsg", { msg: displayMsg }));
        rLog.error(`[Flash] Operation '${op}' failed: ${flashMessage.value}`);
      }
      deps.logOperationDuration();
      deps.onOperationSettled();
    }
  }

  return {
    flashProgress,
    flashPhase,
    flashMessage,
    runningOp,
    currentBackendPhase,
    phaseProgress,
    phaseIndeterminate,
    authOpIsRead,
    cancelIndeterminateCheck,
    handleFlashProgressPayload,
  };
}
