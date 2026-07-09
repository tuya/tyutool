/**
 * Return the release-notes block for the requested locale.
 *
 * Release notes use a two-block layout: a full Chinese block, a `---` line,
 * then a full English block. This splits on the first `---` line and returns
 * only the matching block. Notes without a `---` (old inline `中文 / English`
 * format, or mono-lingual) are returned unchanged — graceful fallback.
 *
 * Pure function — no Vue/i18n imports.
 */
export type SplitLocale = "zh-CN" | "en";

const SEPARATOR = /^[ \t]*-{3,}[ \t]*$/m;

export function splitNotes(notes: string, locale: SplitLocale): string {
  if (!notes) return "";
  const m = SEPARATOR.exec(notes);
  if (!m || m.index === undefined) return notes.trim();
  const zh = notes.slice(0, m.index).trim();
  const en = notes.slice(m.index + m[0].length).trim();
  return locale === "zh-CN" ? zh : en;
}
