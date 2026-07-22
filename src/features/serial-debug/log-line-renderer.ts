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

type CachedLineBase = {
  plainText: string;
  lowerPlainText: string;
  ansiSpans: AnsiSpan[];
  plainSpans: AnsiSpan[];
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

  render(
    lines: readonly DebugLogLine[],
    ansiEnabled: boolean,
    searchQuery: string,
  ): RenderedLogLine[] {
    const normalizedQuery = searchQuery.trim().toLowerCase();
    const visibleIds = new Set(lines.map((line) => line.id));
    for (const existingId of this.baseByLineId.keys()) {
      if (!visibleIds.has(existingId)) {
        this.baseByLineId.delete(existingId);
      }
    }

    return lines.map((line) =>
      this.renderLine(line, ansiEnabled, normalizedQuery),
    );
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

    const spans = (ansiEnabled ? base.ansiSpans : base.plainSpans).map(
      (span) => ({
        text: span.text,
        style: span.style,
        segments: splitByKeyword(span.text, searchQuery),
      }),
    );
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

  private getOrCreateBase(line: DebugLogLine): CachedLineBase {
    let existing = this.baseByLineId.get(line.id);
    if (existing) {
      return existing;
    }

    const plainText = stripAnsi(line.text);
    existing = {
      plainText,
      lowerPlainText: plainText.toLowerCase(),
      ansiSpans: applyLevelFallback(line, plainText, parseAnsi(line.text)),
      plainSpans: [{ text: plainText, style: {} }],
    };
    this.baseByLineId.set(line.id, existing);
    return existing;
  }
}
