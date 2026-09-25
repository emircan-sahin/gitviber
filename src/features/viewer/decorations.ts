import type { Blame, BlameCommit, DiffRow } from "@/lib/api";
import { monaco } from "@/lib/editor/monaco";
import { fullDate, relativeTime } from "@/lib/format";

// The color find's decorations ask an editor's own overview ruler for (past 1000 matches, merged ones).
const FIND_MARK = "editorOverviewRuler.findMatchForeground";

/**
 * Find's matches on a diff's overview, which stands in for the editors' own scrollbars (hidden,
 * see diffOptions) where Monaco would mark them: each side's on its half, as the diff's own colors are.
 */
export function markFindMatches(e: monaco.editor.IStandaloneDiffEditor, host: HTMLElement, split: () => boolean) {
  const canvas = document.createElement("canvas");
  canvas.className = "gv-find-marks";
  host.appendChild(canvas);
  const sides = [e.getOriginalEditor(), e.getModifiedEditor()];
  let frame = 0;
  const draw = () => {
    frame = 0;
    const ratio = window.devicePixelRatio;
    canvas.width = Math.round(canvas.clientWidth * ratio);
    canvas.height = Math.round(canvas.clientHeight * ratio);
    const ctx = canvas.getContext("2d")!;
    ctx.fillStyle = getComputedStyle(canvas).color;
    // Both sides scroll as one; the new side's height counts the unified view's deleted lines too.
    const scale = canvas.height / Math.max(1, sides[1].getScrollHeight());
    const half = canvas.width / 2;
    // The unified view hides the old side, which keeps any matches from while it showed.
    sides.slice(split() ? 0 : 1).forEach((side) => {
      const model = side.getModel();
      const x = side === sides[0] ? 0 : half;
      for (const d of (model && side.getDecorationsInRange(model.getFullModelRange())) ?? []) {
        const mark = d.options.overviewRuler?.color;
        if (typeof mark !== "object" || mark.id !== FIND_MARK) continue;
        const top = side.getTopForLineNumber(d.range.startLineNumber);
        const bottom = side.getBottomForLineNumber(d.range.endLineNumber);
        ctx.fillRect(x, Math.floor(top * scale), half, Math.max(3 * ratio, Math.ceil((bottom - top) * scale)));
      }
    });
  };
  const redraw = () => (frame ||= requestAnimationFrame(draw));
  const subs = sides.flatMap((side) => [side.onDidChangeModelDecorations(redraw), side.onDidContentSizeChange(redraw), side.onDidLayoutChange(redraw)]);
  return {
    dispose() {
      cancelAnimationFrame(frame);
      subs.forEach((s) => s.dispose());
      canvas.remove();
      // WebKit frees the backing store only once the canvas is collected, which it puts off.
      canvas.width = canvas.height = 0;
    },
  };
}

const barDecorations = new WeakMap<monaco.editor.ICodeEditor, monaco.editor.IEditorDecorationsCollection>();

/** The file view's VS Code-style change bars, in the gutter and on the scrollbar. */
export function markBars(e: monaco.editor.ICodeEditor, bars: ReturnType<typeof changeBars>) {
  const css = getComputedStyle(document.documentElement);
  const color = { add: css.getPropertyValue("--added"), mod: css.getPropertyValue("--primary"), del: css.getPropertyValue("--removed") };
  let collection = barDecorations.get(e);
  if (!collection) barDecorations.set(e, (collection = e.createDecorationsCollection()));
  collection.set(
    bars.map((b) => ({
      range: new monaco.Range(b.line, 1, b.line, 1),
      options: {
        isWholeLine: true,
        linesDecorationsClassName: `gv-bar gv-bar-${b.kind}`,
        overviewRuler: { color: color[b.kind].trim(), position: monaco.editor.OverviewRulerLane.Left },
      },
    })),
  );
}

/** Lines to mark in the file view: added, modified, and where lines were deleted. */
export function changeBars(rows: DiffRow[]) {
  const out: { line: number; kind: "add" | "mod" | "del" }[] = [];
  let last = 0;
  for (let i = 0; i < rows.length; ) {
    if (rows[i].k === 0) {
      last = rows[i++].n;
      continue;
    }
    let j = i;
    let dels = 0;
    while (j < rows.length && rows[j].k !== 0) if (rows[j++].k === 2) dels++;
    const adds = rows.slice(i, j).filter((r) => r.k === 1);
    for (const r of adds) out.push({ line: r.n, kind: dels ? "mod" : "add" });
    // Deleted lines: marked on the line after them (the last line at the end of the file).
    if (dels && !adds.length) out.push({ line: j < rows.length ? rows[j].n : Math.max(1, last), kind: "del" });
    if (adds.length) last = adds[adds.length - 1].n;
    i = j;
  }
  return out;
}

const blameDecorations = new WeakMap<monaco.editor.ICodeEditor, monaco.editor.IEditorDecorationsCollection>();

/** A line's blame entry; lines git has none for (past its end, not in HEAD) are new. */
export function blameAt(blame: Blame | null, line: number): BlameCommit | null {
  if (!blame) return null;
  return blame.commits[blame.lines[line - 1]] ?? NEW_LINE;
}
const NEW_LINE: BlameCommit = { sha: "0".repeat(40), authorName: "", authorEmail: "", timestamp: 0, message: "", path: "" };
export const isNew = (c: BlameCommit) => /^0+$/.test(c.sha);

/**
 * The blame column: one entry per run of lines from the same commit, labeled on its first
 * line (a class per label, whose ::after holds the text), with the whole message on hover.
 */
export function markBlame(e: monaco.editor.ICodeEditor, blame: Blame | null) {
  let collection = blameDecorations.get(e);
  if (!collection) blameDecorations.set(e, (collection = e.createDecorationsCollection()));
  const model = e.getModel();
  if (!blame || !model) return collection.clear();
  // A final newline leaves an empty last line in the editor that isn't a line to git: no entry.
  const count = model.getLineCount();
  const n = count > 1 && count > blame.lines.length && model.getLineContent(count) === "" ? count - 1 : count;
  const out: monaco.editor.IModelDeltaDecoration[] = [];
  for (let line = 1; line <= n; ) {
    const c = blameAt(blame, line)!;
    let end = line;
    while (end < n && blameAt(blame, end + 1)!.sha === c.sha) end++;
    const kind = isNew(c) ? "gv-blame gv-blame-new" : "gv-blame";
    out.push({
      range: new monaco.Range(line, 1, end, 1),
      options: {
        linesDecorationsClassName: kind,
        firstLineDecorationClassName: `${kind} gv-blame-first ${blameLabel(c)}`,
        linesDecorationsTooltip: blameTooltip(c),
      },
    });
    line = end + 1;
  }
  collection.set(out);
}

/** Characters in a blame label: short SHA, author, age. */
export const BLAME_CHARS = 33;
const AUTHOR_CHARS = 16;

const labelClasses = new Map<string, string>();
let labelSheet: CSSStyleSheet | null = null;

/** The class that shows `c`'s label, made the first time it's needed. */
function blameLabel(c: BlameCommit) {
  const author = c.authorName.length > AUTHOR_CHARS ? `${c.authorName.slice(0, AUTHOR_CHARS - 1)}…` : c.authorName.padEnd(AUTHOR_CHARS);
  const text = isNew(c) ? "Uncommitted" : `${c.sha.slice(0, 7)} ${author} ${relativeTime(c.timestamp)}`;
  let cls = labelClasses.get(text);
  if (cls) return cls;
  cls = `gvb-${labelClasses.size}`;
  labelClasses.set(text, cls);
  if (!labelSheet) {
    const style = document.createElement("style");
    document.head.appendChild(style);
    labelSheet = style.sheet!;
  }
  const content = text.replace(/[\\"]/g, "\\$&").replace(/\n/g, " ");
  labelSheet.insertRule(`.monaco-editor .${cls}::after { content: "${content}"; }`, labelSheet.cssRules.length);
  return cls;
}

function blameTooltip(c: BlameCommit) {
  if (isNew(c)) return "Not committed yet";
  const when = fullDate(c.timestamp);
  return `${c.sha.slice(0, 10)} · ${c.authorName} <${c.authorEmail}> · ${when}\n\n${c.message}\n\nClick to show it in History`;
}
