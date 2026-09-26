// Code shows 4 columns an indentation level everywhere. A file indented with fewer spaces a
// level gets each level turned into a tab (drawn 4 wide) for display, and copies turn the tabs
// back, so what leaves the app is the file's own text. Pure, so it runs under `node --test`.

export const TAB = 4;

/**
 * Spaces per indentation level to widen, from the most common step between indented lines;
 * 0 when it's already a tab stop or more, or when a tab starts a line or ends its leading spaces
 * (either couldn't be turned back exactly, and the file view saves what it shows).
 */
export function indentUnit(...texts: (string | null | undefined)[]) {
  const steps = new Map<number, number>();
  for (const text of texts) {
    if (!text) continue;
    if (/^ *\t/m.test(text)) return 0;
    let prev = 0;
    for (const line of text.split("\n")) {
      if (!line.trim()) continue;
      const spaces = line.length - line.trimStart().length;
      const step = spaces - prev;
      if (step > 0 && step <= 8) steps.set(step, (steps.get(step) ?? 0) + 1);
      prev = spaces;
    }
  }
  let unit = 0;
  for (const [step, n] of steps) if (n > (steps.get(unit) ?? 0) || (n === steps.get(unit) && step < unit)) unit = step;
  // One-space steps are alignment (a JSDoc's " * "), not indentation.
  return unit >= 2 && unit < TAB ? unit : 0;
}

/** `line` with its leading indentation levels as tabs. */
export const widenLine = (line: string, unit: number) => {
  const spaces = line.length - line.replace(/^ +/, "").length;
  return spaces ? "\t".repeat(Math.floor(spaces / unit)) + " ".repeat(spaces % unit) + line.slice(spaces) : line;
};

export const widen = (text: string, unit: number) => (unit ? text.replace(/^ +/gm, (s) => widenLine(s, unit)) : text);

/** Where column `col` (0-based, UTF-16) of `line` lands once the line is widened. */
export function widenColumn(line: string, col: number, unit: number) {
  const spaces = line.length - line.replace(/^ +/, "").length;
  if (!unit || !spaces) return col;
  const indent = Math.floor(spaces / unit) + (spaces % unit);
  return col <= spaces ? Math.min(col, indent) : col - spaces + indent;
}

/** Text copied out of a widened view, as it is in the file. */
export const narrow = (text: string, unit: number) => (unit ? text.replace(/^\t+/gm, (t) => " ".repeat(t.length * unit)) : text);

/** A copy handler for a widened view: the selection leaves with the file's own indentation. */
export const copyNarrowed = (unit: number) => (e: { clipboardData: DataTransfer; preventDefault(): void }) => {
  const text = unit && window.getSelection()?.toString();
  if (!text) return;
  e.clipboardData.setData("text/plain", narrow(text, unit));
  e.preventDefault();
};
