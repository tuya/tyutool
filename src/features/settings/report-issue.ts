import { GITHUB_NEW_ISSUE_URL } from "@/config/app";

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
