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
  const dest = await save({
    defaultPath: `tyutool-logs-${timestampForFilename()}.zip`,
    filters: [{ name: "Zip", extensions: ["zip"] }],
  });
  if (!dest) return;

  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("export_logs_zip", { destPath: dest });

  const url = buildIssueUrl({
    version: APP_VERSION,
    os: detectOs(navigator.userAgent),
  });
  // window.open is a no-op inside the Tauri webview, so on failure we surface
  // the URL in the dialog instead of silently doing nothing.
  let opened = false;
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
    opened = true;
  } catch (e) {
    rLog.error(
      `[ReportIssue] openUrl failed: ${e instanceof Error ? e.message : String(e)}`,
    );
  }

  const savedTo = t("settings.reportIssue.savedTo", { path: dest });
  await showConfirmDialog({
    title: t("settings.reportIssue.title"),
    message: opened
      ? `${savedTo}\n\n${t("settings.reportIssue.issueOpened")}`
      : `${savedTo}\n\n${t("settings.reportIssue.openFailed")}\n${url}`,
    kind: "info",
    okLabel: t("common.ok"),
    showCancel: false,
  });
}
