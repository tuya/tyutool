import { describe, expect, it } from 'vitest';
import { parseAnsi, stripAnsi } from './ansi-parse';

describe('parseAnsi', () => {
  it('plain text returns single span with empty style', () => {
    expect(parseAnsi('hello')).toEqual([{ text: 'hello', style: {} }]);
  });

  it('empty string returns empty array', () => {
    expect(parseAnsi('')).toEqual([]);
  });

  it('single foreground color 3-bit red (31)', () => {
    const spans = parseAnsi('\x1b[31mred\x1b[0m');
    expect(spans).toEqual([{ text: 'red', style: { fg: '#cd0000' } }]);
  });

  it('full reset (0) clears all style', () => {
    const spans = parseAnsi('\x1b[1;31mbold-red\x1b[0mnormal');
    expect(spans[0].style).toEqual({ bold: true, fg: '#cd0000' });
    expect(spans[1].style).toEqual({});
  });

  it('reset combined with color in one sequence (e.g. \\x1b[0;34;49m)', () => {
    // Tuya firmware uses \x1b[0;34;49m — reset then blue fg, default bg
    const spans = parseAnsi('\x1b[0;34;49mblue text\x1b[0m');
    expect(spans[0].style).toEqual({ fg: '#0000ee' });
  });

  it('bold + italic + underline combination', () => {
    const spans = parseAnsi('\x1b[1;3;4mbiu\x1b[0m');
    expect(spans[0].style).toEqual({ bold: true, italic: true, underline: true });
  });

  it('individual attribute resets do not affect other attrs', () => {
    const spans = parseAnsi('\x1b[1;4mstart\x1b[22mmid\x1b[24mend');
    expect(spans[0].style).toEqual({ bold: true, underline: true });
    expect(spans[1].style).toEqual({ underline: true });
    expect(spans[2].style).toEqual({});
  });

  it('bright foreground colors (90-97 range)', () => {
    expect(parseAnsi('\x1b[92mgreen')[0].style.fg).toBe('#00ff00');
    expect(parseAnsi('\x1b[91mred')[0].style.fg).toBe('#ff0000');
  });

  it('background color (41 = red bg)', () => {
    expect(parseAnsi('\x1b[41mtext')[0].style.bg).toBe('#cd0000');
  });

  it('bright background (101 = bright red bg)', () => {
    expect(parseAnsi('\x1b[101mtext')[0].style.bg).toBe('#ff0000');
  });

  it('256-color foreground (38;5;196 = pure red)', () => {
    expect(parseAnsi('\x1b[38;5;196mtext')[0].style.fg).toBe('rgb(255,0,0)');
  });

  it('256-color grayscale (38;5;232 = near black)', () => {
    expect(parseAnsi('\x1b[38;5;232mtext')[0].style.fg).toBe('rgb(8,8,8)');
  });

  it('RGB true-color foreground (38;2;255;128;0)', () => {
    expect(parseAnsi('\x1b[38;2;255;128;0mtext')[0].style.fg).toBe('rgb(255,128,0)');
  });

  it('RGB true-color background (48;2;0;0;255)', () => {
    expect(parseAnsi('\x1b[48;2;0;0;255mtext')[0].style.bg).toBe('rgb(0,0,255)');
  });

  it('non-SGR sequences are stripped and not shown in text', () => {
    const spans = parseAnsi('a\x1b[2Jb\x1b[?25lc');
    expect(spans.map((s) => s.text).join('')).toBe('abc');
  });

  it('fg/bg reset (39/49) clears only that channel', () => {
    const spans = parseAnsi('\x1b[31;41mcolored\x1b[39mno-fg\x1b[49mnone');
    expect(spans[0].style).toEqual({ fg: '#cd0000', bg: '#cd0000' });
    expect(spans[1].style).toEqual({ bg: '#cd0000' });
    expect(spans[2].style).toEqual({});
  });
});

describe('stripAnsi', () => {
  it('removes all SGR sequences', () => {
    expect(stripAnsi('\x1b[31mhello\x1b[0m world')).toBe('hello world');
  });
  it('removes non-SGR sequences too', () => {
    expect(stripAnsi('\x1b[2Jhello\x1b[?25l')).toBe('hello');
  });
  it('plain text unchanged', () => {
    expect(stripAnsi('plain')).toBe('plain');
  });
});
