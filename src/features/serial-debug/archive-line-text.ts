/**
 * Turning archive-backed lines into the text a user reads or saves.
 *
 * The Rust session archive stores the "archive size cap reached" notice as a
 * machine-readable sentinel (`serial_debug_archive_cap_sentinel` in
 * `crates/tyutool-core/src/serial_debug.rs`), never as a finished sentence:
 * the wording is user-visible and must come from the i18n catalogue, and
 * translating it at read time means an archive re-read after a language switch
 * shows the notice in the *new* language.
 *
 * Every path that surfaces archive text therefore has to translate it, and they
 * all go through `localizeArchiveLineText` here — one helper, so a path added
 * later cannot quietly leak the raw sentinel to the user.
 */
import { i18n } from "@/i18n";
import { stripAnsi } from "./ansi-parse";
import { formatTs } from "./utils";
import type { DebugLineDirection, SerialDebugLine } from "./types";

// Mirrors `serial_debug_archive_cap_sentinel` in
// `crates/tyutool-core/src/serial_debug.rs`. U+0001 (SOH) is a C0 control
// character that never occurs in a translated UI string.
// `String.fromCharCode(1)` rather than an escape so this source file stays
// printable ASCII end to end.
const SOH = String.fromCharCode(1);
const SENTINEL_PREFIX = `${SOH}tyutool:archive-capped:`;
const SENTINEL_SUFFIX = SOH;

/**
 * The MiB limit carried by an archive-cap sentinel, or `null` for anything
 * else. Mirrors `serial_debug_archive_cap_limit_mib`, including its
 * collision-safety rules: whole-string match (not a substring), and `sys` lines
 * only — device output always arrives as `tx`/`rx`, so a device echoing the
 * marker byte-for-byte still cannot forge the notice.
 */
function archiveCapSentinelLimitMib(
  direction: DebugLineDirection,
  text: string,
): number | null {
  if (direction !== "sys") return null;
  if (!text.startsWith(SENTINEL_PREFIX) || !text.endsWith(SENTINEL_SUFFIX)) {
    return null;
  }
  const digits = text.slice(
    SENTINEL_PREFIX.length,
    text.length - SENTINEL_SUFFIX.length,
  );
  if (!/^[0-9]+$/.test(digits)) return null;
  return Number(digits);
}

/** The translated "archive stopped recording" notice. */
export function archiveCapNoticeText(limitMib: number): string {
  return i18n.global.t("serialDebug.log.archiveCapped", { mib: limitMib });
}

/** Replace the archive-cap sentinel with its translation; pass anything else through. */
export function localizeArchiveLineText(
  direction: DebugLineDirection,
  text: string,
): string {
  const limitMib = archiveCapSentinelLimitMib(direction, text);
  return limitMib === null ? text : archiveCapNoticeText(limitMib);
}

/** One line of an exported / saved log file. */
export function formatExportLine(line: SerialDebugLine): string {
  const dir =
    line.direction === "tx" ? "TX " : line.direction === "rx" ? "RX " : "SYS";
  const text = localizeArchiveLineText(line.direction, line.text);
  return `[${formatTs(line.tsMs)}] [${dir}] ${stripAnsi(text)}`;
}
