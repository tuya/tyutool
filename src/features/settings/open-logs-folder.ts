import type { ComposerTranslation } from "vue-i18n";
import { desktopAppLogDirHint } from "@/config/tauri-desktop-paths";
import { DEV_OPEN_APP_LOG_DIR_PATH } from "@/config/dev-endpoints";
import { showConfirmDialog } from "@/composables/confirmDialog";
import { isTauriRuntime } from "@/runtime";

async function showLogsFolderInfo(
  t: ComposerTranslation,
  message: string,
): Promise<void> {
  await showConfirmDialog({
    title: t("settings.logsFolderWebDevTitle"),
    message,
    kind: "info",
    okLabel: t("settings.logsFolderWebDevOk"),
    showCancel: false,
  });
}

export async function openLogsFolder(t: ComposerTranslation): Promise<void> {
  if (!isTauriRuntime() && import.meta.env.DEV) {
    try {
      const res = await fetch(DEV_OPEN_APP_LOG_DIR_PATH, { method: "POST" });
      const text = await res.text();
      let data: { ok?: boolean; error?: string };
      try {
        data = JSON.parse(text) as { ok?: boolean; error?: string };
      } catch {
        await showLogsFolderInfo(
          t,
          t("settings.logsFolderDevOpenFailed", { detail: text.slice(0, 200) }),
        );
        return;
      }
      if (res.ok && data.ok) {
        return;
      }
      await showLogsFolderInfo(
        t,
        t("settings.logsFolderDevOpenFailed", {
          detail: data.error ?? `${res.status}`,
        }),
      );
    } catch (e) {
      await showLogsFolderInfo(
        t,
        t("settings.logsFolderDevOpenFailed", {
          detail: e instanceof Error ? e.message : String(e),
        }),
      );
    }
    return;
  }
  if (!isTauriRuntime()) {
    await showLogsFolderInfo(
      t,
      t("settings.logsFolderWebDevMessage", { path: desktopAppLogDirHint() }),
    );
    return;
  }
  try {
    const { appLogDir } = await import("@tauri-apps/api/path");
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    const dir = await appLogDir();
    const { info } = await import("@tauri-apps/plugin-log");
    await info(`openLogsFolder: dir=${dir}`);
    await revealItemInDir(dir);
    await info("openLogsFolder: revealItemInDir returned OK");
  } catch (e) {
    const { error: logError } = await import("@tauri-apps/plugin-log");
    await logError(`openLogsFolder failed: ${e}`);
  }
}
