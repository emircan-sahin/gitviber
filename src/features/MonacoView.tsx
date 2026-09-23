import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef } from "react";
import type { DiffPair, DiffRow } from "@/lib/api";
import { showLanguage } from "@/lib/highlight";
import { languageFor } from "@/lib/language";
import { monaco, monacoLanguage, prepare } from "@/lib/monaco";
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
// How long a new file waits for its diff before showing: long enough that deleted lines are
// there from the first paint instead of pushing the text down a moment later.
const DIFF_WAIT = 300;
const CONTEXT = 3;

/** The code view on Monaco (VS Code's editor): a diff editor for changes, a plain one for files. */
export const MonacoView = forwardRef<CodeViewHandle, Props>(function MonacoView({ pair, path, mode, collapse, wrap, scrollKey }, ref) {
  const s = useSettings();
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<Editor | null>(null);
  /** The scroll key of the file on show, to save where it was left when the editor goes. */
  const shown = useRef<string | null>(null);
  const diff = mode !== "file";
  const lang = useMemo(() => languageFor(path, pair.modified.exists ? pair.modified.text : pair.original.text), [path, pair]);
  const bars = useMemo(() => (diff || !pair.original.exists || !pair.modified.exists ? [] : changeBars(pair.rows)), [diff, pair]);

  useEffect(() => {
    showLanguage(lang);
    return () => showLanguage(null);
  }, [lang]);

  // One editor per kind; the file on show changes by swapping its models.
  useEffect(() => {
    const el = host.current!;
    const e = diff ? monaco.editor.createDiffEditor(el, diffOptions(s, mode, collapse, wrap)) : monaco.editor.create(el, fileOptions(s, wrap));
    editor.current = e;
    return () => {
      if (shown.current) viewStates.set(shown.current, e.saveViewState()!);
      shown.current = null;
      const models = modelsOf(e);
      e.dispose();
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

  // Swap in the file: colored and diffed before it's shown, then back where it was left.
  useEffect(() => {
    const e = editor.current!;
    let stale = false;
    const language = monacoLanguage(lang);
    const original = diff ? monaco.editor.createModel(pair.original.text, language) : null;
    const modified = monaco.editor.createModel(pair.modified.text, language);
    (async () => {
      await prepare(lang, s.codeTheme);
      if (stale) return;
      const old = modelsOf(e);
      if (isDiff(e)) {
        const vm = e.createViewModel({ original: original!, modified });
        await Promise.race([vm.waitForDiff(), new Promise((r) => setTimeout(r, DIFF_WAIT))]);
        if (stale) return vm.dispose();
        e.setModel(vm);
      } else {
        e.setModel(modified);
        markBars(e, bars);
      }
      old.forEach((m) => m.dispose());
      shown.current = scrollKey;
      const saved = viewStates.get(scrollKey);
      if (saved) e.restoreViewState(saved as never);
      else if (isDiff(e)) e.revealFirstDiff();
      else if (bars.length) scrollToLine(e, bars[0].line);
    })();
    return () => {
      stale = true;
      // Gone with the editor already; else a file that never got on screen.
      if (editor.current !== e) return;
      if (modelsOf(e).includes(modified)) viewStates.set(scrollKey, e.saveViewState()!);
      else [original, modified].forEach((m) => m?.dispose());
    };
    // The theme is applied by the effect below, the rest belongs to the file.
  }, [pair, lang, diff, scrollKey]);

  useEffect(() => void prepare(lang, s.codeTheme), [s.codeTheme, s.dark, lang]);

  // Colors follow the theme; a new file gets its bars when its model goes in.
  useEffect(() => {
    const e = editor.current!;
    if (!isDiff(e) && e.getModel()) markBars(e, bars);
  }, [bars, s.dark]);

  useImperativeHandle(
    ref,
    () => {
      const go = (dir: 1 | -1) => {
        const e = editor.current;
        if (!e) return;
        if (isDiff(e)) return e.goToDiff(dir === 1 ? "next" : "previous");
        // File view: from the line CONTEXT below the top, where a jump puts a change.
        const at = (e.getVisibleRanges()[0]?.startLineNumber ?? 1) + CONTEXT;
        const starts = bars.filter((b, i) => i === 0 || bars[i - 1].line !== b.line - 1).map((b) => b.line);
        const to = dir === 1 ? starts.find((l) => l > at) : [...starts].reverse().find((l) => l < at);
        if (to != null) scrollToLine(e, to);
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
    tabSize: 4,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    renderLineHighlight: "none",
    folding: false,
    glyphMargin: false,
    stickyScroll: { enabled: false },
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
