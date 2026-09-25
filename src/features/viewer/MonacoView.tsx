import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef } from "react";
import type { Blame, BlameCommit, DiffPair, DiffRow } from "@/lib/api";
import { showLanguage } from "@/lib/editor/shownLanguage";
import { languageFor } from "@/lib/editor/language";
import { narrow, widenColumn } from "@/lib/editor/indent";
import { useFind } from "@/lib/ui/find";
import { findMatches } from "@/lib/ui/findQuery";
import { type CodeReveal, onReveal, takeReveal } from "@/lib/editor/reveal";
import { codeWantsFocus, setCodeEditor } from "@/lib/ui/panels";
import { followDefinitions } from "@/lib/editor/definitions";
import { followLineActions, type LineAction, type LineActions } from "@/lib/editor/lineActions";
import { codeEditor, type Editor, hideEditor, hideFile, isDiff, showEditor, showFile } from "./activeEditor";
import { followReviewThreads, type Review } from "@/features/github/pulls/ReviewThreads";
import type { LinkSide } from "@/lib/links/linkHost";
import { colorThrough, createModels, monaco, prepare, redrawWhenColored, releaseModels } from "@/lib/editor/monaco";
import { useSettings } from "@/lib/settings";
import { blameAt, changeBars, isNew, markBars, markBlame, markFindMatches } from "./decorations";
import { CONTEXT, type CodeMode, diffOptions, fileOptions } from "./editorOptions";

export interface CodeViewHandle {
  next(): void;
  prev(): void;
  /** Stage, unstage or discard the selected lines, else the change at the cursor. */
  lineAction(action: LineAction): void;
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

type FindState = { searchString: string; replaceString: string; isReplaceRevealed: boolean; searchScope: null };
type FindController = { closeFindWidget(): void; getState(): { change(state: FindState, moveCursor: boolean, updateHistory: boolean): void } };

// Where each file was left, for the life of the app (tab switches included).
const viewStates = new Map<string, monaco.editor.IDiffEditorViewState | monaco.editor.ICodeEditorViewState>();
// An idle editor of each kind, empty and detached, for the next view to take. Every tab switch
// remounts the view, and each disposed editor left its DOM to a garbage collection WebKit puts off
// until the page nears 1 GB: opening and closing files kept adding 50-100 MB.
const parked: Record<"diff" | "file", { e: Editor; box: HTMLDivElement } | null> = { diff: null, file: null };
// How long a new file waits for its diff before showing, so deleted lines are there from the first
// paint instead of pushing the text down a moment later. git's diff is ready at once; this bounds
// the fallback, Monaco computing one itself.
const DIFF_WAIT = 300;
// Lines past where a file opens that are colored before it shows.
const SCREEN = 150;

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
  const lines = useRef<LineActions | null>(null);
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
    const kind = diff ? "diff" : "file";
    const reused = parked[kind];
    parked[kind] = null;
    const box = reused?.box ?? document.createElement("div");
    box.style.cssText = "width:100%;height:100%";
    el.appendChild(box);
    const e = reused?.e ?? (diff ? monaco.editor.createDiffEditor(box, diffOptions(s, mode, collapse, wrap)) : monaco.editor.create(box, fileOptions(s, wrap, blameColumn)));
    // Detached, it was laid out at 0x0: measure now, before a file is scrolled into place.
    if (reused) e.layout();
    editor.current = e;
    showEditor(e);
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
    lines.current = isDiff(e)
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
      lines.current?.dispose();
      lines.current = null;
      threads.current?.dispose();
      threads.current = null;
      if (shown.current) viewStates.set(shown.current, e.saveViewState()!);
      shown.current = null;
      const models = modelsOf(e);
      // A fresh view opens with Find closed and empty; Monaco would reopen it on the next model.
      for (const code of isDiff(e) ? [e.getOriginalEditor(), e.getModifiedEditor()] : [e]) {
        const find = code.getContribution("editor.contrib.findController") as FindController | null;
        find?.closeFindWidget();
        find?.getState().change({ searchString: "", replaceString: "", isReplaceRevealed: false, searchScope: null }, false, false);
      }
      e.setModel(null);
      // A view model handed to setModel stays ours to dispose.
      viewModel.current?.dispose();
      viewModel.current = null;
      releaseModels(models);
      box.remove();
      if (parked[kind]) e.dispose();
      else parked[kind] = { e, box };
      editor.current = null;
      hideEditor(e);
    };
    // Options follow below; only the kind of editor needs a new one.
  }, [diff]);

  // A line asked for (lib/editor/reveal) in the file already on show.
  const revealing = useRef<string | null>(null);
  useEffect(
    () =>
      onReveal(() => {
        const e = editor.current;
        const r = e && revealing.current ? takeReveal(revealing.current) : null;
        if (e && r) showReveal(codeEditor(e), r, unit.current);
      }),
    [],
  );

  // Focus Code View, F6 and → from a list land in the editor that scrolls.
  useEffect(() => setCodeEditor(() => editor.current && codeEditor(editor.current).focus()), []);

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
      if (onDisk) showFile(shows);
      unit.current = created.unit;
      releaseModels(old);
      shown.current = scrollKey;
      shownPair.current = { pair, path };
      threads.current?.update();
      const code = codeEditor(e);
      // Opened from the code view (J/K, a tab switch) or sent here before it was ready: take the keys.
      if (codeWantsFocus()) code.focus();
      // Opened by a link that names a line (path:12, #L12) or a search result: there, wherever it was left.
      revealing.current = diff ? null : path;
      const r = diff ? null : takeReveal(path, true);
      if (r) return showReveal(code, r, created.unit);
      if (saved) return e.restoreViewState(saved as never);
      // Near the first change. A diff still computing takes it there when it lands, unless you
      // have scrolled since.
      const toFirst = () => {
        const [line] = changeStarts(e, bars);
        if (line) goToLine(code, line);
      };
      if (ready) return toFirst();
      const top = code.getScrollTop();
      void diffed.then(() => !stale && code.getScrollTop() === top && toFirst());
    })();
    return () => {
      stale = true;
      revealing.current = null;
      stopRedraw();
      hideFile(shows);
      if (!onScreen) return releaseModels(models);
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
        if (to != null) goToLine(code, to);
      };
      return { next: () => go(1), prev: () => go(-1), lineAction: (action) => lines.current?.act(action) };
    },
    [bars],
  );

  return <div ref={host} data-scrollbar="none" className="relative h-full" />;
});

/** The editor that scrolls: the diff's new side, which carries the old one along. */

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

/** A link's line at its column, or a search result's with its match selected. */
function showReveal(e: monaco.editor.ICodeEditor, r: CodeReveal, unit: number) {
  const model = e.getModel();
  if (!model) return;
  if (!("query" in r)) return revealAt(e, { lineNumber: r.line, column: r.column }, unit);
  const line = Math.min(r.line, model.getLineCount());
  const found = findMatches(model.getLineContent(line), r.query, r.options, 1);
  const [start, end] = (!(found instanceof Error) && found[0]) || [0, 0];
  const range = new monaco.Range(line, start + 1, line, end + 1);
  e.setSelection(range);
  e.revealRangeInCenter(range);
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

/**
 * Puts `line` CONTEXT lines below the top (reveal* only scrolls lines that are off screen), with
 * the cursor on it: the change the keys stage or discard is the one moved to (lib/editor/lineActions).
 */
function goToLine(e: monaco.editor.ICodeEditor, line: number) {
  e.setPosition({ lineNumber: line, column: 1 });
  e.setScrollTop(e.getTopForLineNumber(Math.max(1, line - CONTEXT)));
}

function modelsOf(e: Editor) {
  if (!isDiff(e)) return e.getModel() ? [e.getModel()!] : [];
  const m = e.getModel();
  return m ? [m.original, m.modified] : [];
}
