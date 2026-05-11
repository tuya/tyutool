export interface AnsiStyle {
  fg?: string;
  bg?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
}

export interface AnsiSpan {
  text: string;
  style: AnsiStyle;
}

// Standard xterm 16-color palette
const ANSI_COLORS: string[] = [
  '#000000', '#cd0000', '#00cd00', '#cdcd00',
  '#0000ee', '#cd00cd', '#00cdcd', '#e5e5e5',
  '#7f7f7f', '#ff0000', '#00ff00', '#ffff00',
  '#5c5cff', '#ff00ff', '#00ffff', '#ffffff',
];

function color256(n: number): string {
  if (n < 16) return ANSI_COLORS[n];
  if (n >= 232) {
    const v = (n - 232) * 10 + 8;
    return `rgb(${v},${v},${v})`;
  }
  const idx = n - 16;
  const r = Math.floor(idx / 36);
  const g = Math.floor(idx / 6) % 6;
  const b = idx % 6;
  const ch = (x: number): number => (x === 0 ? 0 : x * 40 + 55);
  return `rgb(${ch(r)},${ch(g)},${ch(b)})`;
}

function applyParams(current: AnsiStyle, params: string): AnsiStyle {
  const parts = params ? params.split(';').map(Number) : [0];
  const s: AnsiStyle = { ...current };
  let i = 0;
  while (i < parts.length) {
    const p = parts[i];
    if (p === 0) return {};
    else if (p === 1) s.bold = true;
    else if (p === 3) s.italic = true;
    else if (p === 4) s.underline = true;
    else if (p === 22) delete s.bold;
    else if (p === 23) delete s.italic;
    else if (p === 24) delete s.underline;
    else if (p === 39) delete s.fg;
    else if (p === 49) delete s.bg;
    else if (p >= 30 && p <= 37) s.fg = ANSI_COLORS[p - 30];
    else if (p >= 90 && p <= 97) s.fg = ANSI_COLORS[p - 90 + 8];
    else if (p >= 40 && p <= 47) s.bg = ANSI_COLORS[p - 40];
    else if (p >= 100 && p <= 107) s.bg = ANSI_COLORS[p - 100 + 8];
    else if (p === 38 || p === 48) {
      const isFg = p === 38;
      if (parts[i + 1] === 5 && i + 2 < parts.length) {
        const color = color256(parts[i + 2]);
        if (isFg) s.fg = color; else s.bg = color;
        i += 2;
      } else if (parts[i + 1] === 2 && i + 4 < parts.length) {
        const color = `rgb(${parts[i + 2]},${parts[i + 3]},${parts[i + 4]})`;
        if (isFg) s.fg = color; else s.bg = color;
        i += 4;
      }
    }
    i++;
  }
  return s;
}

const ESC_RE = /\x1b\[[^\x1b]*?[A-Za-z]/g;

export function parseAnsi(text: string): AnsiSpan[] {
  const spans: AnsiSpan[] = [];
  let style: AnsiStyle = {};
  let lastIndex = 0;
  ESC_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = ESC_RE.exec(text)) !== null) {
    if (match.index > lastIndex) {
      const t = text.slice(lastIndex, match.index);
      if (t) spans.push({ text: t, style: { ...style } });
    }
    lastIndex = ESC_RE.lastIndex;
    const seq = match[0];
    const m = seq.match(/\x1b\[([0-9;]*)m/);
    if (m) style = applyParams(style, m[1]);
  }
  if (lastIndex < text.length) {
    const t = text.slice(lastIndex);
    if (t) spans.push({ text: t, style: { ...style } });
  }
  return spans;
}

export function stripAnsi(text: string): string {
  return text.replace(/\x1b\[[^\x1b]*?[A-Za-z]/g, '');
}
