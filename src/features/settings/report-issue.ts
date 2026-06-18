import { GITHUB_NEW_ISSUE_URL, APP_VERSION } from "@/config/app";
import type { ComposerTranslation } from "vue-i18n";
import { isTauriRuntime } from "@/runtime";
import { showConfirmDialog } from "@/composables/confirmDialog";
import { rLog } from "@/utils/log";

/**
 * Map a navigator.userAgent string to the same coarse OS vocabulary used by
 * std::env::consts::OS on the Rust side ("windows" | "macos" | "linux").
 * Pure helper — takes the UA as an arg so it is unit-testable.
 */
export function detectOs(userAgent: string): string {
  const ua = userAgent.toLowerCase();
  if (ua.includes("windows") || ua.includes("win")) return "windows";
  if (ua.includes("mac") || ua.includes("darwin")) return "macos";
  if (ua.includes("linux") || ua.includes("x11")) return "linux";
  return "unknown";
}

/** Build a pre-filled GitHub "new issue" URL for the bug_report form. */
export function buildIssueUrl(env: {
  version: string;
  os: string;
  install?: string;
}): string {
  const body = [
    "<!-- 请描述问题，并附上导出的日志 zip / Please describe the issue and attach the exported log zip -->",
    "",
    `- tyutool 版本 / version: ${env.version}`,
    `- 系统 / OS: ${env.os}`,
    env.install ? `- 安装方式 / install: ${env.install}` : "",
    "",
    "## 复现步骤 / Steps to reproduce",
    "",
    "## 期望结果 / 实际结果 (Expected / Actual)",
    "",
  ]
    .filter(Boolean)
    .join("\n");
  const params = new URLSearchParams({
    template: "bug_report.yml",
    title: "[Bug] ",
    body,
  });
  return `${GITHUB_NEW_ISSUE_URL}?${params.toString()}`;
}

function timestampForFilename(): string {
  return new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
}

/** Save a logs zip, then open a pre-filled GitHub issue. No-ops with a hint in web mode. */
export async function exportLogsAndReport(
  t: ComposerTranslation,
): Promise<void> {
  if (!isTauriRuntime()) {
    await showConfirmDialog({
      title: t("settings.reportIssue.title"),
      message: t("settings.reportIssue.webHint"),
      kind: "info",
      okLabel: t("common.ok"),
      showCancel: false,
    });
    return;
  }
  const { save } = await import("@tauri-apps/plugin-dialog");
  const { downloadDir, homeDir, join } = await import("@tauri-apps/api/path");
  const filename = `tyutool-logs-${timestampForFilename()}.zip`;
  // An AppImage's working directory is its read-only mount, so a bare filename
  // makes the save dialog default *inside the mount* — where the write fails and
  // aborts the whole flow. Anchor the default to a writable user directory.
  let baseDir = "";
  try {
    baseDir = await downloadDir();
  } catch {
    try {
      baseDir = await homeDir();
    } catch {
      /* fall back to a bare filename */
    }
  }
  const defaultPath = baseDir ? await join(baseDir, filename) : filename;
  const dest = await save({
    defaultPath,
    filters: [{ name: "Zip", extensions: ["zip"] }],
  });
  if (!dest) return;

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("export_logs_zip", { destPath: dest });
  } catch (e) {
    const detail = e instanceof Error ? e.message : String(e);
    rLog.error(`[ReportIssue] export_logs_zip failed: ${detail}`);
    await showConfirmDialog({
      title: t("settings.reportIssue.title"),
      message: t("settings.reportIssue.exportFailed", { error: detail }),
      kind: "warning",
      okLabel: t("common.ok"),
      showCancel: false,
    });
    return;
  }

  const url = buildIssueUrl({
    version: APP_VERSION,
    os: detectOs(navigator.userAgent),
  });
  // The opener plugin spawns the URL handler *detached* and reports success
  // even when the browser never actually appears (e.g. a broken default-handler
  // chain on Linux), so we cannot rely on openUrl alone. Always copy the URL to
  // the clipboard as a reliable fallback the user can paste manually.
  try {
    await navigator.clipboard.writeText(url);
  } catch {
    /* clipboard unavailable — the open attempt below is the only path */
  }
  // Use our own command (not the opener plugin): on Linux/AppImage it strips the
  // AppImage env and uses the system xdg-open, and it reports real failures
  // instead of detaching and silently "succeeding".
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("open_external_url", { url });
  } catch (e) {
    rLog.error(
      `[ReportIssue] open_external_url failed: ${e instanceof Error ? e.message : String(e)}`,
    );
  }

  await showConfirmDialog({
    title: t("settings.reportIssue.title"),
    message: `${t("settings.reportIssue.savedTo", { path: dest })}\n\n${t("settings.reportIssue.linkCopied")}`,
    kind: "info",
    okLabel: t("common.ok"),
    showCancel: false,
  });
}
