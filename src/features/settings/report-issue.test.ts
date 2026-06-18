import { describe, it, expect } from "vitest";
import { buildIssueUrl } from "./report-issue";

describe("buildIssueUrl", () => {
  it("targets the bug_report template with env in the body", () => {
    const url = buildIssueUrl({
      version: "3.0.11",
      os: "linux",
      install: "AppImage",
    });
    expect(url).toContain("https://github.com/tuya/tyutool/issues/new");
    expect(url).toContain("template=bug_report.yml");
    const decoded = decodeURIComponent(url);
    expect(decoded).toContain("3.0.11");
    expect(decoded).toContain("linux");
    expect(decoded).toContain("AppImage");
  });

  it("omits the install line when not provided", () => {
    const decoded = decodeURIComponent(
      buildIssueUrl({ version: "1.0.0", os: "windows" }),
    );
    expect(decoded).not.toContain("安装方式");
  });
});
