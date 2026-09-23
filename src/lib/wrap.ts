// Line wrapping computed rather than left to CSS, the way VS Code's monospace line breaker
// does it: a row's height is then known before it's laid out, so the scroll range, the
// overview ruler and jumps to a change never depend on measuring the DOM.
// Pure (no DOM, no React) so it runs under `node --test`.

export interface Wrap {
  /** Offsets where continuation lines start. */
  at: number[];
  /** Columns continuation lines are indented by: the line's own indentation. */
  indent: number;
}

export const FITS: Wrap = { at: [], indent: 0 };

export const TAB = 4;
// Two columns in a monospace font: East Asian wide/fullwidth, emoji, flags.
const WIDE =
  /[ᄀ-ᅟ⺀-〾ぁ-㏿㐀-䶿一-鿿ꀀ-꓏가-힣豈-﫿︰-﹏＀-｠￠-￦️\u{20000}-\u{3fffd}]|\p{Emoji_Presentation}|\p{Regional_Indicator}/u;
// No columns: marks that sit on the letter before, zero-width characters.
const ZERO = /^[\p{Mn}\p{Me}​-‏⁠﻿]+$/u;
// Anything but printable ASCII, where a character can be other than one column wide.
const NOT_SIMPLE = /[^\x20-\x7e]/;
const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });

function width(ch: string, col: number) {
  if (ch === "\t") return TAB - (col % TAB);
  return ZERO.test(ch) ? 0 : WIDE.test(ch) ? 2 : 1;
}

/** What the eye sees as one character, so an emoji sequence or a flag is never split. */
function clusters(text: string) {
  return NOT_SIMPLE.test(text) ? Array.from(graphemes.segment(text), (s) => s.segment) : text.split("");
}

/** Columns parts[a, b) take, starting at column 0. */
function span(parts: string[], a: number, b: number) {
  let col = 0;
  for (let k = a; k < b; k++) col += width(parts[k], col);
  return col;
}

/** Columns a whole line takes. */
export function lineWidth(text: string) {
  if (!NOT_SIMPLE.test(text)) return text.length;
  const parts = clusters(text);
  return span(parts, 0, parts.length);
}

/**
 * Where `text` wraps at `cols` columns. Breaks after whitespace when it can, else mid-word;
 * whitespace itself never forces a break, it hangs past the edge.
 */
export function wrapLine(text: string, cols: number): Wrap {
  // Nothing is wider than two columns, or a tab.
  if (text.length * (text.includes("\t") ? TAB : 2) <= cols) return FITS;
  if (text.length <= cols && !NOT_SIMPLE.test(text)) return FITS;
  const parts = clusters(text);
  const offset = [0];
  for (const p of parts) offset.push(offset[offset.length - 1] + p.length);
  let indent = 0;
  for (const p of parts) {
    if (p !== " " && p !== "\t") break;
    indent += width(p, indent);
  }
  // Deep indentation would leave continuation lines no room: start those at the edge instead.
  if (indent > cols / 2) indent = 0;

  const at: number[] = [];
  let room = cols;
  let col = 0;
  let start = 0;
  let lastBreak = -1; // just after whitespace that follows something solid on this line
  let lastSolid = -1;
  for (let k = 0; k < parts.length; ) {
    const space = parts[k] === " " || parts[k] === "\t";
    const w = width(parts[k], col);
    if (!space && col + w > room && k > start) {
      let b = lastBreak > start ? lastBreak : k;
      let carried = span(parts, b, k);
      // The carried word must fit the continuation line, or it's cut here instead.
      if (carried >= cols - indent) [b, carried] = [k, 0];
      at.push(offset[b]);
      start = b;
      room = cols - indent;
      lastBreak = -1;
      col = carried;
      continue;
    }
    col += w;
    k++;
    if (!space) lastSolid = k - 1;
    else if (lastSolid >= start) lastBreak = k;
  }
  return at.length ? { at, indent } : FITS;
}
