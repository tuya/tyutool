import {
  parseAnsi,
  stripAnsi,
  type AnsiSpan,
  type AnsiStyle,
} from "./ansi-parse";
import type { DebugLogLine } from "./types";

export interface RenderKeywordSegment {
  text: string;
  isMatch: boolean;
}

export interface RenderLineSpan {
  text: string;
  style: AnsiStyle;
  segments: RenderKeywordSegment[];
}

export interface RenderedLogLine {
  line: DebugLogLine;
  spans: RenderLineSpan[];
  hasMatch: boolean;
}

type CachedRenderedLine = {
  ansiEnabled: boolean;
  searchQuery: string;
  view: RenderedLogLine;
};

// `ansiSpans`/`plainSpans` are built lazily: the log view scans the whole
// buffer for search matches (needs `lowerPlainText` only) but builds spans for
// the visible slice alone, so parsing every line's ANSI eagerly would undo the
// saving.
type CachedLineBase = {
  plainText: string;
  lowerPlainText: string;
  ansiSpans?: AnsiSpan[];
  plainSpans?: AnsiSpan[];
  lastRendered?: CachedRenderedLine;
};

// Theme-aware fallback colors for RX lines whose ESP-IDF/Tuya level prefix
// (`E (...)`, `W (...)`, ...) carries no ANSI foreground color (issue #110).
const LEVEL_PREFIX_RE = /^([EWIDV]) \(/;
const LEVEL_FALLBACK_FG: Record<string, string> = {
  E: "var(--ty-danger)",
  W: "var(--ty-accent)",
  I: "var(--ty-success)",
  D: "var(--ty-primary)",
  V: "var(--ty-text-muted)",
};

function applyLevelFallback(
  line: DebugLogLine,
  plainText: string,
  spans: AnsiSpan[],
): AnsiSpan[] {
  if (line.direction !== "rx") return spans;
  if (spans.some((span) => span.style.fg)) return spans;
  const match = LEVEL_PREFIX_RE.exec(plainText);
  if (!match) return spans;
  const fg = LEVEL_FALLBACK_FG[match[1]];
  return spans.map((span) => ({
    text: span.text,
    style: { ...span.style, fg },
  }));
}

function splitByKeyword(
  text: string,
  searchQuery: string,
): RenderKeywordSegment[] {
  if (!searchQuery) {
    return [{ text, isMatch: false }];
  }

  const parts: RenderKeywordSegment[] = [];
  const lower = text.toLowerCase();
  let pos = 0;
  while (pos < text.length) {
    const idx = lower.indexOf(searchQuery, pos);
    if (idx === -1) {
      parts.push({ text: text.slice(pos), isMatch: false });
      break;
    }
    if (idx > pos) {
      parts.push({ text: text.slice(pos, idx), isMatch: false });
    }
    parts.push({
      text: text.slice(idx, idx + searchQuery.length),
      isMatch: true,
    });
    pos = idx + searchQuery.length;
  }
  return parts;
}

export class SerialDebugLogLineRenderer {
  private baseByLineId = new Map<number, CachedLineBase>();

  /**
   * Build display spans. Callers pass the slice they are about to mount (the
   * visible window), so this must NOT drive cache eviction — call `retain` with
   * the full buffer for that.
   */
  render(
    lines: readonly DebugLogLine[],
    ansiEnabled: boolean,
    searchQuery: string,
  ): RenderedLogLine[] {
    const normalizedQuery = searchQuery.trim().toLowerCase();
    return lines.map((line) =>
      this.renderLine(line, ansiEnabled, normalizedQuery),
    );
  }

  /** Drop cached data for lines that have left the buffer. */
  retain(lines: readonly DebugLogLine[]): void {
    if (this.baseByLineId.size === 0) return;
    const liveIds = new Set(lines.map((line) => line.id));
    for (const existingId of this.baseByLineId.keys()) {
      if (!liveIds.has(existingId)) {
        this.baseByLineId.delete(existingId);
      }
    }
  }

  /**
   * Ids of every line whose text contains `searchQuery`, over the whole buffer.
   * Substring test only — no span building — so it stays affordable for lines
   * that are never mounted.
   */
  matchingLineIds(
    lines: readonly DebugLogLine[],
    searchQuery: string,
  ): number[] {
    const normalizedQuery = searchQuery.trim().toLowerCase();
    if (!normalizedQuery) return [];
    const ids: number[] = [];
    for (const line of lines) {
      if (this.getOrCreateBase(line).lowerPlainText.includes(normalizedQuery)) {
        ids.push(line.id);
      }
    }
    return ids;
  }

  cacheSize(): number {
    return this.baseByLineId.size;
  }

  private renderLine(
    line: DebugLogLine,
    ansiEnabled: boolean,
    searchQuery: string,
  ): RenderedLogLine {
    const base = this.getOrCreateBase(line);
    const cached = base.lastRendered;
    if (
      cached &&
      cached.ansiEnabled === ansiEnabled &&
      cached.searchQuery === searchQuery
    ) {
      return cached.view;
    }

    const spans = this.spansFor(line, base, ansiEnabled).map((span) => ({
      text: span.text,
      style: span.style,
      segments: splitByKeyword(span.text, searchQuery),
    }));
    const view: RenderedLogLine = {
      line,
      spans,
      hasMatch:
        searchQuery.length > 0 && base.lowerPlainText.includes(searchQuery),
    };
    base.lastRendered = {
      ansiEnabled,
      searchQuery,
      view,
    };
    return view;
  }

  private spansFor(
    line: DebugLogLine,
    base: CachedLineBase,
    ansiEnabled: boolean,
  ): AnsiSpan[] {
    if (ansiEnabled) {
      base.ansiSpans ??= applyLevelFallback(
        line,
        base.plainText,
        parseAnsi(line.text),
      );
      return base.ansiSpans;
    }
    base.plainSpans ??= [{ text: base.plainText, style: {} }];
    return base.plainSpans;
  }

  private getOrCreateBase(line: DebugLogLine): CachedLineBase {
    let existing = this.baseByLineId.get(line.id);
    if (existing) {
      return existing;
    }

    const plainText = stripAnsi(line.text);
    existing = {
      plainText,
      lowerPlainText: plainText.toLowerCase(),
    };
    this.baseByLineId.set(line.id, existing);
    return existing;
  }
}
