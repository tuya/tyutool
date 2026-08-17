// @vitest-environment happy-dom
// happy-dom: @/i18n reads localStorage at import time.
import { afterEach, describe, expect, it } from "vitest";
import { i18n } from "@/i18n";
import {
  archiveCapNoticeText,
  chunksDroppedNoticeText,
  formatExportLine,
  localizeArchiveLineText,
} from "./archive-line-text";
import type { DebugLineDirection, SerialDebugLine } from "./types";

// Pins the wire format produced by `serial_debug_archive_cap_sentinel` /
// `serial_debug_chunk_drop_sentinel` in crates/tyutool-core/src/serial_debug.rs.
// Spelled with fromCharCode so this file stays printable ASCII.
const SOH = String.fromCharCode(1);
const sentinel = (mib: number): string =>
  `${SOH}tyutool:archive-capped:${mib}${SOH}`;
const dropSentinel = (bytes: number): string =>
  `${SOH}tyutool:chunks-dropped:${bytes}${SOH}`;

const originalLocale = i18n.global.locale.value;

afterEach(() => {
  i18n.global.locale.value = originalLocale;
});

function line(direction: DebugLineDirection, text: string): SerialDebugLine {
  return { lineNo: 1, tsMs: 0, direction, text };
}

describe("localizeArchiveLineText", () => {
  it("translates the cap sentinel and never leaks it verbatim", () => {
    i18n.global.locale.value = "en";
    const out = localizeArchiveLineText("sys", sentinel(256));

    expect(out).not.toContain(SOH);
    expect(out).not.toContain("archive-capped");
    expect(out).toContain("256 MiB");
    expect(out).toBe(archiveCapNoticeText(256));
  });

  it("renders in the active locale, so an archive re-read follows the UI language", () => {
    i18n.global.locale.value = "zh-CN";
    const zh = localizeArchiveLineText("sys", sentinel(64));
    i18n.global.locale.value = "en";
    const en = localizeArchiveLineText("sys", sentinel(64));

    expect(zh).toContain("64 MiB");
    expect(zh).toContain("会话归档");
    expect(en).toContain("64 MiB");
    expect(en).not.toBe(zh);
  });

  it("cannot be forged by device output or by a line that merely embeds it", () => {
    // Device bytes always arrive as tx/rx, so an echoed marker stays literal.
    expect(localizeArchiveLineText("rx", sentinel(256))).toBe(sentinel(256));
    expect(localizeArchiveLineText("tx", sentinel(256))).toBe(sentinel(256));

    for (const text of [
      `boot: ${sentinel(256)}`,
      `${sentinel(256)} trailing`,
      `${SOH}tyutool:archive-capped:${SOH}`,
      `${SOH}tyutool:archive-capped:12x${SOH}`,
      "tyutool:archive-capped:256",
    ]) {
      expect(localizeArchiveLineText("sys", text)).toBe(text);
    }
  });

  it("passes ordinary sys lines through untouched", () => {
    expect(localizeArchiveLineText("sys", "Port lost")).toBe("Port lost");
  });

  it("translates the dropped-chunk sentinel and never leaks it verbatim", () => {
    i18n.global.locale.value = "en";
    const out = localizeArchiveLineText("sys", dropSentinel(12288));

    expect(out).not.toContain(SOH);
    expect(out).not.toContain("chunks-dropped");
    expect(out).toContain("12288");
    expect(out).toBe(chunksDroppedNoticeText(12288));
  });

  it("renders the dropped-chunk notice in the active locale", () => {
    i18n.global.locale.value = "zh-CN";
    const zh = localizeArchiveLineText("sys", dropSentinel(4096));
    i18n.global.locale.value = "en";
    const en = localizeArchiveLineText("sys", dropSentinel(4096));

    expect(zh).toContain("4096");
    expect(zh).toContain("设备输出");
    expect(en).toContain("4096");
    expect(en).not.toBe(zh);
  });

  it("cannot have a dropped-chunk notice forged by device output", () => {
    expect(localizeArchiveLineText("rx", dropSentinel(4096))).toBe(
      dropSentinel(4096),
    );
    expect(localizeArchiveLineText("tx", dropSentinel(4096))).toBe(
      dropSentinel(4096),
    );

    for (const text of [
      `boot: ${dropSentinel(4096)}`,
      `${dropSentinel(4096)} trailing`,
      `${SOH}tyutool:chunks-dropped:${SOH}`,
      `${SOH}tyutool:chunks-dropped:12x${SOH}`,
      "tyutool:chunks-dropped:4096",
    ]) {
      expect(localizeArchiveLineText("sys", text)).toBe(text);
    }
  });

  it("keeps the two sentinel families apart", () => {
    i18n.global.locale.value = "en";
    expect(localizeArchiveLineText("sys", dropSentinel(256))).toBe(
      chunksDroppedNoticeText(256),
    );
    expect(localizeArchiveLineText("sys", sentinel(256))).toBe(
      archiveCapNoticeText(256),
    );
    expect(chunksDroppedNoticeText(256)).not.toBe(archiveCapNoticeText(256));
  });
});

describe("formatExportLine", () => {
  it("writes the translated notice into the export, not the sentinel", () => {
    i18n.global.locale.value = "zh-CN";
    const out = formatExportLine(line("sys", sentinel(128)));

    expect(out).not.toContain(SOH);
    expect(out).not.toContain("archive-capped");
    expect(out).toContain("[SYS]");
    expect(out).toContain("128 MiB");
  });

  it("writes the translated data-loss notice into the export", () => {
    i18n.global.locale.value = "en";
    const out = formatExportLine(line("sys", dropSentinel(65536)));

    expect(out).not.toContain(SOH);
    expect(out).not.toContain("chunks-dropped");
    expect(out).toContain("[SYS]");
    expect(out).toContain("65536");
  });

  it("keeps the direction tag and strips ANSI from device lines", () => {
    const esc = String.fromCharCode(27);
    expect(formatExportLine(line("rx", `${esc}[31mboom${esc}[0m`))).toContain(
      "[RX ] boom",
    );
    expect(formatExportLine(line("tx", "ping"))).toContain("[TX ] ping");
  });
});
