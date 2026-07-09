import { describe, expect, it } from "vitest";
import { splitNotes } from "./split-notes";

describe("splitNotes", () => {
  const notes = [
    "### 新功能",
    "",
    "- `settings`：自动检查更新现在可配置间隔（关闭 / 1 小时），手动「检查更新」始终立即触发",
    "- `serial-debug`：规范并校验芯片目标",
    "",
    "### 问题修复",
    "",
    "- `serial-debug`：批次完成后重新刷新统计（已用 / 剩余）",
    "",
    "---",
    "",
    "### Features",
    "",
    "- `settings`: Auto update checks are interval-based (off / 1h)",
    "- `serial-debug`: Normalize and validate chip IDs",
    "",
    "### Bug Fixes",
    "",
    "- `serial-debug`: Refresh stats after batch completes (used / remaining)",
    "",
  ].join("\n");

  it("returns the Chinese block for zh-CN", () => {
    expect(splitNotes(notes, "zh-CN")).toBe(
      [
        "### 新功能",
        "",
        "- `settings`：自动检查更新现在可配置间隔（关闭 / 1 小时），手动「检查更新」始终立即触发",
        "- `serial-debug`：规范并校验芯片目标",
        "",
        "### 问题修复",
        "",
        "- `serial-debug`：批次完成后重新刷新统计（已用 / 剩余）",
      ].join("\n"),
    );
  });

  it("returns the English block for en", () => {
    expect(splitNotes(notes, "en")).toBe(
      [
        "### Features",
        "",
        "- `settings`: Auto update checks are interval-based (off / 1h)",
        "- `serial-debug`: Normalize and validate chip IDs",
        "",
        "### Bug Fixes",
        "",
        "- `serial-debug`: Refresh stats after batch completes (used / remaining)",
      ].join("\n"),
    );
  });

  it("returns the input unchanged when there is no separator (old inline format)", () => {
    const oldFormat = "### 新功能 / Features\n\n- `x`：中文 / English";
    expect(splitNotes(oldFormat, "zh-CN")).toBe(oldFormat.trim());
    expect(splitNotes(oldFormat, "en")).toBe(oldFormat.trim());
  });

  it("returns '' for empty input", () => {
    expect(splitNotes("", "zh-CN")).toBe("");
    expect(splitNotes("", "en")).toBe("");
  });

  it("returns the only block when the English block is absent (mono-lingual)", () => {
    const zhOnly = "### 新功能\n\n- `x`：仅中文条目";
    expect(splitNotes(zhOnly, "zh-CN")).toBe(zhOnly);
    expect(splitNotes(zhOnly, "en")).toBe(zhOnly);
  });

  it("splits on the first --- line even if more than one exists", () => {
    const multi = "中文块\n\n---\n\nEnglish block\n\n---\n\nextra";
    expect(splitNotes(multi, "zh-CN")).toBe("中文块");
    expect(splitNotes(multi, "en")).toBe("English block\n\n---\n\nextra");
  });
});
