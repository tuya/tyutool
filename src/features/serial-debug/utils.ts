/**
 * Serial-debug helpers shared across components.
 */
import type { DebugLogLine, SerialDebugLine } from "./types";

/**
 * Maps an archive-backed line onto a display line. The caller supplies `id`
 * because archive `lineNo`s restart at 1 on every session, while display ids
 * must stay unique for the whole store lifetime — the log renderers cache
 * parsed lines by id, so a reused id would render the wrong content.
 */
export function archiveLineToLogLine(
  line: SerialDebugLine,
  id: number,
): DebugLogLine {
  return {
    id,
    tsMs: line.tsMs,
    direction: line.direction,
    text: line.text,
    rawBytes: line.rawBytes ? Uint8Array.from(line.rawBytes) : undefined,
  };
}

export function sanitizePortName(port: string): string {
  const stripped = port.startsWith("/") ? port.slice(1) : port;
  return stripped.replace(/[/\\:*?"<>|.]/g, "_");
}

export function makeStamp(): string {
  const now = new Date();
  return `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}-${String(now.getHours()).padStart(2, "0")}${String(now.getMinutes()).padStart(2, "0")}${String(now.getSeconds()).padStart(2, "0")}`;
}

export function formatTs(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  const mmm = String(d.getMilliseconds()).padStart(3, "0");
  return `${hh}:${mm}:${ss}.${mmm}`;
}
