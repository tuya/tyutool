import { describe, it, expect } from "vitest";
import { buildIssueUrl, detectOs } from "./report-issue";

describe("buildIssueUrl", () => {
  it("prefills the bug_report form's version and os fields", () => {
    const u = new URL(buildIssueUrl({ version: "3.0.11", os: "linux" }));
    expect(u.pathname).toContain("/tuya/tyutool/issues/new");
    expect(u.searchParams.get("template")).toBe("bug_report.yml");
    expect(u.searchParams.get("version")).toBe("3.0.11");
    expect(u.searchParams.get("os")).toBe("linux");
  });

  it("folds the install type into the os field when provided", () => {
    const u = new URL(
      buildIssueUrl({ version: "1.0.0", os: "linux", install: "AppImage" }),
    );
    expect(u.searchParams.get("os")).toBe("linux (AppImage)");
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
