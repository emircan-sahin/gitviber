import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef } from "react";
import type { DiffPair, DiffRow } from "@/lib/api";
import { showLanguage } from "@/lib/highlight";
import { languageFor } from "@/lib/language";
import { narrow } from "@/lib/indent";
import { colorThrough, createModels, monaco, prepare, redrawWhenColored } from "@/lib/monaco";
import { CODE_FONTS, type Settings, useSettings } from "@/lib/settings";

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

/** The code view on Monaco (VS Code's editor): a diff editor for changes, a plain one for files. */
export const MonacoView = forwardRef<CodeViewHandle, Props>(function MonacoView({ pair, path, mode, collapse, wrap, scrollKey }, ref) {
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

  useEffect(() => {
    showLanguage(lang);
    return () => showLanguage(null);
  }, [lang]);

  // One editor per kind; the file on show changes by swapping its models.
  const viewModel = useRef<monaco.editor.IDiffEditorViewModel | null>(null);
  useEffect(() => {
    const el = host.current!;
    const e = diff ? monaco.editor.createDiffEditor(el, diffOptions(s, mode, collapse, wrap)) : monaco.editor.create(el, fileOptions(s, wrap));
    editor.current = e;
    return () => {
      if (shown.current) viewStates.set(shown.current, e.saveViewState()!);
      shown.current = null;
      const models = modelsOf(e);
      e.dispose();
      // A view model handed to setModel stays ours to dispose.
      viewModel.current?.dispose();
      viewModel.current = null;
      models.forEach((m) => m.dispose());
      editor.current = null;
    };
    // Options follow below; only the kind of editor needs a new one.
  }, [diff]);

  useEffect(() => {
    const e = editor.current!;
    if (isDiff(e)) e.updateOptions(diffOptions(s, mode, collapse, wrap));
    else e.updateOptions(fileOptions(s, wrap));
  }, [s, mode, collapse, wrap, diff]);

  // Swap in the file: colored and diffed before it's shown, then back where it was left. A new
  // theme swaps it in again: recoloring flushes the tokens the unified view drew deleted lines with.
  useEffect(() => {
    const e = editor.current!;
    let stale = false;
    let onScreen = false;
    let models: monaco.editor.ITextModel[] = [];
    let stopRedraw = () => {};
    (async () => {
      await prepare(lang, s.codeTheme);
      if (stale) return;
      // Only now: a model made before its language is registered gets retokenized from scratch
      // when it is, and the unified view's deleted lines came out uncolored.
      const created = createModels(lang, pair.modified.text, diff ? { text: pair.original.text, rows: pair.rows } : null);
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
      }
      onScreen = true;
      unit.current = created.unit;
      old.forEach((m) => m.dispose());
      shown.current = scrollKey;
      if (saved) return e.restoreViewState(saved as never);
      // Near the first change. A diff still computing takes it there when it lands, unless you
      // have scrolled since.
      const code = codeEditor(e);
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
      stopRedraw();
      if (!onScreen) return models.forEach((m) => m.dispose());
      // Still on the editor (not disposed with it): remember where it was left.
      if (editor.current === e) viewStates.set(scrollKey, e.saveViewState()!);
    };
    // The file, and the colors it's drawn in.
  }, [pair, lang, diff, scrollKey, s.codeTheme, s.dark]);

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

  return <div ref={host} data-scrollbar="none" className="h-full" />;
});

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
    fontFamily: CODE_FONTS[s.codeFont],
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

function fileOptions(s: Settings, wrap: boolean): monaco.editor.IStandaloneEditorConstructionOptions {
  return { ...common(s, wrap), lineDecorationsWidth: 12 };
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
