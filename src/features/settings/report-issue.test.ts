import { describe, it, expect } from "vitest";
import { buildIssueUrl, detectOs } from "./report-issue";

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

describe("detectOs", () => {
  it("maps a Windows UA to 'windows'", () => {
    expect(
      detectOs(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
      ),
    ).toBe("windows");
  });

  it("maps a macOS UA to 'macos'", () => {
    expect(
      detectOs(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
      ),
    ).toBe("macos");
  });

  it("maps a Linux/X11 UA to 'linux'", () => {
    expect(
      detectOs(
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
      ),
    ).toBe("linux");
  });

  it("returns 'unknown' for unrecognized strings", () => {
    expect(detectOs("SomeWeirdBot/1.0")).toBe("unknown");
  });
});
