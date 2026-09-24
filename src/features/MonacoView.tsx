import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef } from "react";
import type { Blame, BlameCommit, DiffPair, DiffRow } from "@/lib/api";
import { showLanguage } from "@/lib/highlight";
import { languageFor } from "@/lib/language";
import { narrow, widenColumn } from "@/lib/indent";
import { useFind } from "@/lib/find";
import { findMatches } from "@/lib/findQuery";
import { onReveal, takeReveal } from "@/lib/reveal";
import { codeWantsFocus, setCodeEditor } from "@/lib/panels";
import { followDefinitions } from "@/lib/definitions";
import { followLineActions } from "@/lib/lineActions";
import { followReviewThreads, type Review } from "./ReviewThreads";
import { type LinkSide, onReveal as onLinkReveal, takeReveal as takeLinkReveal } from "@/lib/linkHost";
import { colorThrough, createModels, monaco, prepare, redrawWhenColored } from "@/lib/monaco";
import { codeFontFamily, type Settings, useSettings } from "@/lib/settings";
import { relativeTime } from "@/lib/utils";

export type CodeMode = "unified" | "split" | "file";

export interface CodeViewHandle {
  next(): void;
  prev(): void;
}

interface Props {
  pair: DiffPair;
  path: string;
  mode: CodeMode;
  collapse: boolean;
  wrap: boolean;
  scrollKey: string;
  /** The new side is the file on disk (not the index or a commit), so its lines are the file's. */
  onDisk?: boolean;
  /** The file view's blame column: who last changed each line. */
  blame?: Blame | null;
  /** Room for that column, set aside before `blame` lands so the code doesn't jump. */
  blameColumn?: boolean;
  onBlameClick?: (commit: BlameCommit) => void;
  /** Each side's file and tree, for Go to Definition (the old side only in a diff); none: nowhere to go. */
  links?: { original: LinkSide | null; modified: LinkSide } | null;
  /** A working-tree diff whose changes can be staged, unstaged or discarded from here. */
  staging?: { kind: "unstaged" | "staged"; refresh: () => unknown } | null;
  /** A PR file's line comments, drawn under their lines. */
  review?: Review | null;
}

type Editor = monaco.editor.IStandaloneDiffEditor | monaco.editor.IStandaloneCodeEditor;
const isDiff = (e: Editor): e is monaco.editor.IStandaloneDiffEditor => "getLineChanges" in e;

// Where each file was left, for the life of the app (tab switches included).
const viewStates = new Map<string, monaco.editor.IDiffEditorViewState | monaco.editor.ICodeEditorViewState>();
// How long a new file waits for its diff before showing, so deleted lines are there from the first
// paint instead of pushing the text down a moment later. git's diff is ready at once; this bounds
// the fallback, Monaco computing one itself.
const DIFF_WAIT = 300;
const CONTEXT = 3;
// Lines past where a file opens that are colored before it shows.
const SCREEN = 150;

// The file on show and where it's read, for "Open in" an editor at that line.
let onShow: { path: string; line: () => number } | null = null;

/** The line being read in `path` if the code view shows it: the cursor's if it's on screen, else the top one. */
// The code view's editor, while one is shown.
let live: Editor | null = null;

/** The text selected in the code view, if it's on one line: what Search in Files starts from. */
export function selectedText(): string {
  const code = live && (isDiff(live) && live.getOriginalEditor().hasWidgetFocus() ? live.getOriginalEditor() : codeEditor(live));
  const sel = code?.getSelection();
  if (!code || !sel || sel.isEmpty() || sel.startLineNumber !== sel.endLineNumber) return "";
  return code.getModel()?.getValueInRange(sel) ?? "";
}

export function lineInView(path: string): number | undefined {
  return onShow?.path === path ? onShow.line() : undefined;
}

/** The code view on Monaco (VS Code's editor): a diff editor for changes, a plain one for files. */
export const MonacoView = forwardRef<CodeViewHandle, Props>(function MonacoView({ pair, path, mode, collapse, wrap, scrollKey, onDisk = true, blame = null, blameColumn = false, onBlameClick, links = null, staging = null, review = null }, ref) {
  const s = useSettings();
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<Editor | null>(null);
  /** The scroll key of the file on show, to save where it was left when the editor goes. */
  const shown = useRef<string | null>(null);
  /** Spaces per indentation level the file on show had widened into tabs (0: none). */
  const unit = useRef(0);
  const diff = mode !== "file";
  const lang = useMemo(() => languageFor(path, pair.modified.exists ? pair.modified.text : pair.original.text), [path, pair]);
  const bars = useMemo(() => (diff || !pair.original.exists || !pair.modified.exists ? [] : changeBars(pair.rows)), [diff, pair]);
  // Read by the file swap (which lands later) and by clicks.
  const blameRef = useRef(blame);
  blameRef.current = diff ? null : blame;
  const onBlameClickRef = useRef(onBlameClick);
  onBlameClickRef.current = onBlameClick;
  const linksRef = useRef(links);
  const stagingRef = useRef(staging);
  stagingRef.current = staging;
  const reviewRef = useRef(review);
  reviewRef.current = review;
  const threads = useRef<ReturnType<typeof followReviewThreads> | null>(null);
  // The diff on show, which a newer `pair` replaces only once it's ready.
  const shownPair = useRef<{ pair: DiffPair; path: string } | null>(null);
  linksRef.current = links;
  const split = useRef(false);
  split.current = mode === "split";

  useEffect(() => {
    showLanguage(lang);
    return () => showLanguage(null);
  }, [lang]);

  // One editor per kind; the file on show changes by swapping its models.
  const viewModel = useRef<monaco.editor.IDiffEditorViewModel | null>(null);
  useEffect(() => {
    const el = host.current!;
    const e = diff ? monaco.editor.createDiffEditor(el, diffOptions(s, mode, collapse, wrap)) : monaco.editor.create(el, fileOptions(s, wrap, blameColumn));
    editor.current = e;
    live = e;
    // A click on a blame entry shows its commit.
    const click = isDiff(e)
      ? null
      : e.onMouseDown((ev) => {
          const line = ev.target.position?.lineNumber;
          const commit = line && ev.target.element?.classList.contains("gv-blame") ? blameAt(blameRef.current, line) : null;
          if (commit && !isNew(commit)) onBlameClickRef.current?.(commit);
        });
    const follow = (code: monaco.editor.ICodeEditor, side: "original" | "modified") => followDefinitions(code, () => linksRef.current?.[side] ?? null);
    const linked = isDiff(e) ? [follow(e.getOriginalEditor(), "original"), follow(e.getModifiedEditor(), "modified")] : [follow(e, "modified")];
    const marks = isDiff(e) ? markFindMatches(e, el, () => split.current) : null;
    const lines = isDiff(e)
      ? followLineActions(e, () => {
          const [s, on] = [stagingRef.current, shownPair.current];
          return s && on ? { ...s, ...on } : null;
        })
      : null;
    threads.current = isDiff(e)
      ? followReviewThreads(e, () => {
          const [r, on] = [reviewRef.current, shownPair.current];
          return r && on ? { review: r, rows: on.pair.rows, unified: !split.current } : null;
        })
      : null;
    return () => {
      click?.dispose();
      linked.forEach((l) => l.dispose());
      marks?.dispose();
      lines?.dispose();
      threads.current?.dispose();
      threads.current = null;
      if (shown.current) viewStates.set(shown.current, e.saveViewState()!);
      shown.current = null;
      const models = modelsOf(e);
      e.dispose();
      // A view model handed to setModel stays ours to dispose.
      viewModel.current?.dispose();
      viewModel.current = null;
      models.forEach((m) => m.dispose());
      editor.current = null;
      if (live === e) live = null;
    };
    // Options follow below; only the kind of editor needs a new one.
  }, [diff]);

  // A link to a line of the file already on show.
  const revealing = useRef<string | null>(null);
  useEffect(
    () =>
      onLinkReveal(() => {
        const e = editor.current;
        const line = e && revealing.current ? takeLinkReveal(revealing.current, false) : null;
        if (e && line) revealAt(codeEditor(e), line, unit.current);
      }),
    [],
  );

  // Focus Code View, F6 and → from a list land in the editor that scrolls.
  useEffect(() => setCodeEditor(() => editor.current && codeEditor(editor.current).focus()), []);

  // A search result's match (lib/reveal), once this file view shows its file: its line, the match selected.
  const reveal = useRef<() => boolean>(() => false);
  reveal.current = () => {
    const e = editor.current;
    if (!e || isDiff(e) || shown.current !== scrollKey) return false;
    const r = takeReveal(path);
    if (!r) return false;
    const model = e.getModel()!;
    const line = Math.min(r.line, model.getLineCount());
    const found = findMatches(model.getLineContent(line), r.query, r.options, 1);
    const [start, end] = (!(found instanceof Error) && found[0]) || [0, 0];
    const range = new monaco.Range(line, start + 1, line, end + 1);
    e.setSelection(range);
    e.revealRangeInCenter(range);
    return true;
  };
  useEffect(() => onReveal(() => void reveal.current()), []);

  // Find is Monaco's own box: in split view on the side that has focus, else the new side.
  useFind("code", () => {
    const e = editor.current;
    if (e) (isDiff(e) && e.getOriginalEditor().hasWidgetFocus() ? e.getOriginalEditor() : codeEditor(e)).getAction("actions.find")?.run();
  });

  useEffect(() => {
    const e = editor.current!;
    if (isDiff(e)) e.updateOptions(diffOptions(s, mode, collapse, wrap));
    else e.updateOptions(fileOptions(s, wrap, blameColumn));
  }, [s, mode, collapse, wrap, diff, blameColumn]);

  // New comments, or the other layout (unified view puts old-side threads on the new side).
  useEffect(() => threads.current?.update(), [review, mode]);

  // A blame that lands after the file shows; the swap below marks the one it finds.
  useEffect(() => {
    const e = editor.current;
    if (e && !isDiff(e) && shown.current === scrollKey) markBlame(e, blame);
  }, [blame, scrollKey]);

  // Swap in the file: colored and diffed before it's shown, then back where it was left. A new
  // theme swaps it in again: recoloring flushes the tokens the unified view drew deleted lines with.
  useEffect(() => {
    const e = editor.current!;
    let stale = false;
    let onScreen = false;
    let models: monaco.editor.ITextModel[] = [];
    let stopRedraw = () => {};
    const shows = { path, line: () => readingLine(codeEditor(e)) };
    (async () => {
      await prepare(lang, s.codeTheme);
      if (stale) return;
      // Only now: a model made before its language is registered gets retokenized from scratch
      // when it is, and the unified view's deleted lines came out uncolored.
      const oldPath = linksRef.current?.original?.path ?? path;
      const created = createModels(lang, path, pair.modified.text, diff ? { path: oldPath, text: pair.original.text, rows: pair.rows } : null);
      const { modified, original } = created;
      models = original ? [original, modified] : [modified];
      const old = modelsOf(e);
      let diffed: Promise<unknown> = Promise.resolve();
      let ready = true;
      const saved = viewStates.get(scrollKey);
      if (isDiff(e)) {
        const vm = e.createViewModel({ original: original!, modified });
        diffed = vm.waitForDiff();
        const waited = Promise.race([diffed.then(() => true), new Promise<boolean>((r) => setTimeout(() => r(false), DIFF_WAIT))]);
        // The unified view draws deleted lines once, from the old text's tokens: color the old text
        // through the first screen now (the rest follows in the background, redrawn when done).
        const at = originalLineAt(pair.rows, (saved as monaco.editor.IDiffEditorViewState | undefined)?.modified?.viewState.firstPosition.lineNumber);
        [ready] = await Promise.all([waited, mode === "unified" && colorThrough(original!, at + SCREEN, () => !stale)]);
        if (stale) return vm.dispose();
        e.setModel(vm);
        viewModel.current?.dispose();
        viewModel.current = vm;
        stopRedraw = redrawWhenColored(original!);
      } else {
        e.setModel(modified);
        markBars(e, bars);
        markBlame(e, blameRef.current);
      }
      onScreen = true;
      // A staged diff or a commit shows another version: its line numbers aren't the file's.
      if (onDisk) onShow = shows;
      unit.current = created.unit;
      old.forEach((m) => m.dispose());
      shown.current = scrollKey;
      shownPair.current = { pair, path };
      threads.current?.update();
      const code = codeEditor(e);
      // Opened from the code view (J/K, a tab switch) or sent here before it was ready: take the keys.
      if (codeWantsFocus()) code.focus();
      // Opened by a link that names a line (path:12, #L12): there, wherever it was left.
      revealing.current = diff ? null : path;
      const line = diff ? null : takeLinkReveal(path, true);
      if (line) return revealAt(code, line, created.unit);
      if (reveal.current()) return;
      if (saved) return e.restoreViewState(saved as never);
      // Near the first change. A diff still computing takes it there when it lands, unless you
      // have scrolled since.
      const toFirst = () => {
        const [line] = changeStarts(e, bars);
        if (line) scrollToLine(code, line);
      };
      if (ready) return toFirst();
      const top = code.getScrollTop();
      void diffed.then(() => !stale && code.getScrollTop() === top && toFirst());
    })();
    return () => {
      stale = true;
      revealing.current = null;
      stopRedraw();
      if (onShow === shows) onShow = null;
      if (!onScreen) return models.forEach((m) => m.dispose());
      // Still on the editor (not disposed with it): remember where it was left.
      if (editor.current === e) viewStates.set(scrollKey, e.saveViewState()!);
    };
    // The file, and the colors it's drawn in (the app's palette too: dark and dimmed share a syntax theme).
  }, [pair, lang, diff, scrollKey, onDisk, s.codeTheme, s.theme]);

  // Copies carry the file's own indentation, not the tabs it's shown with. Monaco has filled the
  // clipboard by the time this bubbles up from its text area.
  useEffect(() => {
    const el = host.current!;
    const onCopy = (ev: ClipboardEvent) => {
      const text = unit.current && ev.clipboardData?.getData("text/plain");
      if (text) ev.clipboardData!.setData("text/plain", narrow(text, unit.current));
    };
    el.addEventListener("copy", onCopy);
    el.addEventListener("cut", onCopy);
    return () => {
      el.removeEventListener("copy", onCopy);
      el.removeEventListener("cut", onCopy);
    };
  }, []);

  // A viewer's keys: they scroll, as they did before Monaco. Moving its cursor instead would jump
  // back to wherever that was (the top, while the view opens at the first change), and Space would
  // only say the file is read-only. ⇧-arrows still select; Esc goes back to the list (Workspace).
  useEffect(() => {
    const el = host.current!;
    const onKey = (ev: KeyboardEvent) => {
      const e = editor.current;
      if (!e || ev.isComposing || !(ev.target instanceof HTMLElement)) return;
      if (ev.altKey || ev.ctrlKey || !ev.target.matches("textarea.inputarea")) return;
      // The peek's own editor moves its cursor, as an editor does.
      if (ev.target.closest(".peekview-widget")) return;
      // Split view: the side you clicked into; the other one follows.
      const code = isDiff(e) && e.getOriginalEditor().hasTextFocus() ? e.getOriginalEditor() : codeEditor(e);
      const line = code.getOption(monaco.editor.EditorOption.lineHeight);
      const page = code.getLayoutInfo().height - line;
      const [top, left] = [code.getScrollTop(), code.getScrollLeft()];
      const key = (ev.metaKey ? "cmd+" : "") + (ev.shiftKey ? "shift+" : "") + ev.key;
      const to: Record<string, number> = {
        ArrowDown: top + line,
        ArrowUp: top - line,
        PageDown: top + page,
        PageUp: top - page,
        " ": top + page,
        "shift+ ": top - page,
        Home: 0,
        End: code.getScrollHeight(),
        "cmd+ArrowUp": 0,
        "cmd+ArrowDown": code.getScrollHeight(),
      };
      if (key in to) code.setScrollTop(to[key]);
      else if (key === "ArrowLeft" || key === "ArrowRight") code.setScrollLeft(left + (key === "ArrowLeft" ? -40 : 40));
      else return;
      ev.preventDefault();
      ev.stopPropagation();
    };
    // Capturing: ahead of Monaco's own handling on its text area.
    el.addEventListener("keydown", onKey, true);
    return () => el.removeEventListener("keydown", onKey, true);
  }, []);

  useImperativeHandle(
    ref,
    () => {
      // From the line CONTEXT below the top, where a jump puts a change, to the next one; no wrapping.
      const go = (dir: 1 | -1) => {
        const e = editor.current;
        if (!e) return;
        const code = codeEditor(e);
        const at = (code.getVisibleRanges()[0]?.startLineNumber ?? 1) + CONTEXT;
        const starts = changeStarts(e, bars);
        const to = dir === 1 ? starts.find((l) => l > at) : [...starts].reverse().find((l) => l < at);
        if (to != null) scrollToLine(code, to);
      };
      return { next: () => go(1), prev: () => go(-1) };
    },
    [bars],
  );

  return <div ref={host} data-scrollbar="none" className="relative h-full" />;
});

// The color find's decorations ask an editor's own overview ruler for (past 1000 matches, merged ones).
const FIND_MARK = "editorOverviewRuler.findMatchForeground";

/**
 * Find's matches on a diff's overview, which stands in for the editors' own scrollbars (hidden,
 * see diffOptions) where Monaco would mark them: each side's on its half, as the diff's own colors are.
 */
function markFindMatches(e: monaco.editor.IStandaloneDiffEditor, host: HTMLElement, split: () => boolean) {
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
    },
  };
}

const barDecorations = new WeakMap<monaco.editor.ICodeEditor, monaco.editor.IEditorDecorationsCollection>();

/** The file view's VS Code-style change bars, in the gutter and on the scrollbar. */
function markBars(e: monaco.editor.ICodeEditor, bars: ReturnType<typeof changeBars>) {
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

const blameDecorations = new WeakMap<monaco.editor.ICodeEditor, monaco.editor.IEditorDecorationsCollection>();

/** A line's blame entry; lines git has none for (past its end, not in HEAD) are new. */
function blameAt(blame: Blame | null, line: number): BlameCommit | null {
  if (!blame) return null;
  return blame.commits[blame.lines[line - 1]] ?? NEW_LINE;
}
const NEW_LINE: BlameCommit = { sha: "0".repeat(40), authorName: "", authorEmail: "", timestamp: 0, message: "", path: "" };
const isNew = (c: BlameCommit) => /^0+$/.test(c.sha);

/**
 * The blame column: one entry per run of lines from the same commit, labeled on its first
 * line (a class per label, whose ::after holds the text), with the whole message on hover.
 */
function markBlame(e: monaco.editor.ICodeEditor, blame: Blame | null) {
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
const BLAME_CHARS = 33;
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
  const when = new Date(c.timestamp * 1000).toLocaleString();
  return `${c.sha.slice(0, 10)} · ${c.authorName} <${c.authorEmail}> · ${when}\n\n${c.message}\n\nClick to show it in History`;
}

/** The editor that scrolls: the diff's new side, which carries the old one along. */
const codeEditor = (e: Editor) => (isDiff(e) ? e.getModifiedEditor() : e);

/** First line of each change, in the editor that scrolls (a deletion: the line after it). */
function changeStarts(e: Editor, bars: ReturnType<typeof changeBars>) {
  if (isDiff(e)) return (e.getLineChanges() ?? []).map((c) => (c.modifiedEndLineNumber === 0 ? c.modifiedStartLineNumber + 1 : c.modifiedStartLineNumber));
  return bars.filter((b, i) => i === 0 || bars[i - 1].line !== b.line - 1).map((b) => b.line);
}

/** The old text's line level with new line `n`, or with the first change when there's none. */
function originalLineAt(rows: DiffRow[], n?: number) {
  let o = 1;
  for (const r of rows) {
    if (n == null ? r.k !== 0 : r.k !== 2 && r.n >= n) break;
    if (r.o) o = r.o;
  }
  return o;
}

function readingLine(e: monaco.editor.ICodeEditor) {
  const visible = e.getVisibleRanges();
  const cursor = e.getPosition()?.lineNumber;
  if (cursor && visible.some((r) => cursor >= r.startLineNumber && cursor <= r.endLineNumber)) return cursor;
  return visible[0]?.startLineNumber ?? 1;
}

/** Puts the cursor on `pos` (a column as the file on disk has it, before widening) mid-screen. */
function revealAt(e: monaco.editor.ICodeEditor, pos: { lineNumber: number; column: number }, unit: number) {
  const model = e.getModel();
  if (!model) return;
  const lineNumber = Math.min(Math.max(1, pos.lineNumber), model.getLineCount());
  const column = widenColumn(narrow(model.getLineContent(lineNumber), unit), pos.column - 1, unit) + 1;
  e.setPosition({ lineNumber, column });
  e.revealPositionInCenter({ lineNumber, column });
}

/** Puts `line` CONTEXT lines below the top (reveal* only scrolls lines that are off screen). */
function scrollToLine(e: monaco.editor.ICodeEditor, line: number) {
  e.setScrollTop(e.getTopForLineNumber(Math.max(1, line - CONTEXT)));
}

function modelsOf(e: Editor) {
  if (!isDiff(e)) return e.getModel() ? [e.getModel()!] : [];
  const m = e.getModel();
  return m ? [m.original, m.modified] : [];
}

function common(s: Settings, wrap: boolean): monaco.editor.IEditorOptions & monaco.editor.IGlobalEditorOptions {
  return {
    readOnly: true,
    automaticLayout: true,
    fontFamily: codeFontFamily(s),
    fontSize: s.codeFontSize,
    lineHeight: Math.round(s.codeFontSize * s.lineHeight),
    fontLigatures: s.ligatures,
    wordWrap: wrap ? "on" : "off",
    wrappingIndent: "same",
    // Plain text only: the HTML copy would carry the widened indentation.
    copyWithSyntaxHighlighting: false,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    renderLineHighlight: "none",
    folding: false,
    glyphMargin: false,
    stickyScroll: { enabled: false },
    guides: { indentation: false },
    overviewRulerBorder: false,
    scrollbar: { useShadows: false, verticalScrollbarSize: 14, horizontalScrollbarSize: 10 },
    padding: { top: 4 },
  };
}

function diffOptions(s: Settings, mode: CodeMode, collapse: boolean, wrap: boolean): monaco.editor.IDiffEditorConstructionOptions {
  return {
    ...common(s, wrap),
    renderSideBySide: mode === "split",
    // The layout is the user's choice, not the window width's.
    useInlineViewWhenSpaceIsLimited: false,
    hideUnchangedRegions: { enabled: collapse, contextLineCount: CONTEXT, minimumLineCount: 3, revealLineCount: 20 },
    // Whitespace changes are changes, as git counts them.
    ignoreTrimWhitespace: false,
    originalEditable: false,
    renderMarginRevertIcon: false,
    renderGutterMenu: false,
    diffWordWrap: "inherit",
    // A word change on one side only has an empty range on the other (see lib/diffHunks): no marker.
    experimental: { showEmptyDecorations: false },
    // Room around the +/− signs, like the old gutter's sign column.
    lineDecorationsWidth: 20,
    scrollbar: { ...common(s, wrap).scrollbar, vertical: "hidden", verticalScrollbarSize: 0 },
  };
}

function fileOptions(s: Settings, wrap: boolean, blame: boolean): monaco.editor.IStandaloneEditorConstructionOptions {
  // Blame's label goes after the change bars (index.css), in the code font's widths.
  return { ...common(s, wrap), lineDecorationsWidth: blame ? `${BLAME_CHARS + 3}ch` : 12 };
}

/** Lines to mark in the file view: added, modified, and where lines were deleted. */
function changeBars(rows: DiffRow[]) {
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
