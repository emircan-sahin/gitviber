// Monaco, set up once: only the editor features the app uses, colored by Shiki's TextMate grammars
// and themes (the ones the rest of the app uses), loaded as files open.
import { shikiToMonaco, textmateThemeToMonacoTheme } from "@shikijs/monaco";
import * as monaco from "monaco-editor/editor/editor.api";
import { WorkerBasedDocumentDiffProvider } from "monaco-editor/editor/browser/widget/diffEditor/diffProviderFactoryService";
import { OverviewRulerFeature } from "monaco-editor/editor/browser/widget/diffEditor/features/overviewRulerFeature";
import { diffWholeLineAddDecoration, diffWholeLineDeleteDecoration } from "monaco-editor/editor/browser/widget/diffEditor/registrations.contribution";
import { LineRange } from "monaco-editor/editor/common/core/ranges/lineRange";
import { DetailedLineRangeMapping, RangeMapping } from "monaco-editor/editor/common/diff/rangeMapping";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
import "monaco-editor/features/diffEditor/register";
import "monaco-editor/features/codicon/register";
import "monaco-editor/features/find/register";
import "monaco-editor/features/clipboard/register";
import "monaco-editor/features/contextmenu/register";
// ⌘-click, F12, references and peek for lib/editor/definitions (referenceSearch is the peek and its list);
// ⌘-click on URLs.
import "monaco-editor/features/gotoSymbol/register";
import "monaco-editor/features/referenceSearch/register";
import "monaco-editor/features/links/register";
// Editing the file view as in VS Code: word and subword moves (⌥←, ⌃⌥←, ⌥⌫), line moves and copies
// (⌥↑, ⇧⌥↓, ⇧⌘K, ⌘↵, ⌘]), multiple cursors (⌘D, ⌥⌘↓), ⌘L, ⌘U, ⌃T, expand selection, ⌃G, text dragging.
// Left out: what needs a language's rules or a language server (comments, brackets, suggestions).
import "monaco-editor/features/wordOperations/register";
import "monaco-editor/features/wordPartOperations/register";
import "monaco-editor/features/linesOperations/register";
import "monaco-editor/features/multicursor/register";
import "monaco-editor/features/lineSelection/register";
import "monaco-editor/features/cursorUndo/register";
import "monaco-editor/features/caretOperations/register";
import "monaco-editor/features/smartSelect/register";
import "monaco-editor/features/gotoLine/register";
import "monaco-editor/features/dnd/register";
import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createOnigurumaEngine } from "shiki/engine/oniguruma";
import { bundledLanguages } from "shiki/langs";
import { bundledThemes } from "shiki/themes";
import type { DiffRow } from "../api";
import { hunks, type Pos } from "../git/diffHunks";
import { indentUnit, TAB, widen, widenColumn } from "./indent";
import { IGNORE, ignoreGrammar } from "./language";
import { codeFontFamily, getSettings, subscribeSettings } from "../settings";
import { cssVar, toHex } from "../ui/color";

export { monaco };

// Find opens on editor.find (commands.ts, MonacoView), which the user can rebind; Monaco's own ⌘F would stay behind.
monaco.editor.addKeybindingRule({ keybinding: monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyF, command: "-actions.find" });
// Likewise Go to Definition, Peek and References (editor.goToDefinition, …); "to the side" has no side here.
const { CtrlCmd, Alt, Shift } = monaco.KeyMod;
const { F12, KeyK } = monaco.KeyCode;
monaco.editor.addKeybindingRules([
  { keybinding: F12, command: "-editor.action.revealDefinition" },
  { keybinding: CtrlCmd | F12, command: "-editor.action.revealDefinition" },
  { keybinding: Alt | F12, command: "-editor.action.peekDefinition" },
  { keybinding: Shift | F12, command: "-editor.action.goToReferences" },
  { keybinding: monaco.KeyMod.chord(CtrlCmd | KeyK, F12), command: "-editor.action.revealDefinitionAside" },
  { keybinding: monaco.KeyMod.chord(CtrlCmd | KeyK, CtrlCmd | F12), command: "-editor.action.revealDefinitionAside" },
]);

// Code fonts load lazily (Geist Mono, JetBrains Mono), and Monaco keeps the widths it measured: the
// fallback font's, when it measured first. Every column then drifts (Geist Mono at 13.5px: 8.35px
// measured, 8.1px drawn, so the cursor sat two characters off by a line's end). WebKit never fires
// FontFaceSet's loadingdone, so the font is loaded here and measured again once it's in.
let codeFont = codeFontFamily(getSettings());
const measureLoaded = () => void document.fonts.load(`16px ${codeFont}`).then(() => monaco.editor.remeasureFonts(), () => {});
measureLoaded();
// A new pick measures afresh, a custom font's too (Monaco kept its fallback's widths from before it was installed).
subscribeSettings(() => {
  const next = codeFontFamily(getSettings());
  if (next === codeFont) return;
  codeFont = next;
  monaco.editor.remeasureFonts();
  measureLoaded();
});

// The worker computes diffs only as a fallback (see `createModels`), and finds the URLs links open.
globalThis.MonacoEnvironment = { getWorker: () => new EditorWorker() };
// The diff overview doubles as the scrollbar (the editors' own are hidden): 14px like the app's
// other bars, not 30. No option for it; its layout reads these statics (monaco-editor is pinned).
Object.assign(OverviewRulerFeature, { ONE_OVERVIEW_WIDTH: 7, ENTIRE_DIFF_OVERVIEW_WIDTH: 14 });

// An added or removed block gets its line color alone; Monaco paints it all over with the word
// emphasis color too.
diffWholeLineAddDecoration.className = "char-insert-whole";
diffWholeLineDeleteDecoration.className = "char-delete-whole";

// The diff shown is git's, the one the rest of the app counts, rather than Monaco's own: they
// disagree now and then (a line-ending-only change is none to Monaco), and it's ready at once.
type DocumentDiff = Awaited<ReturnType<WorkerBasedDocumentDiffProvider["computeDiff"]>>;
const gitDiffs = new WeakMap<monaco.editor.ITextModel, DocumentDiff>();
const computeDiff = WorkerBasedDocumentDiffProvider.prototype.computeDiff;
WorkerBasedDocumentDiffProvider.prototype.computeDiff = async function (original, modified, ...rest) {
  return gitDiffs.get(modified) ?? computeDiff.call(this, original, modified, ...rest);
};

function gitDiff(rows: DiffRow[], oldText: string, newText: string, unit: number): DocumentDiff {
  const oldLines = oldText.split("\n");
  const newLines = newText.split("\n");
  // Monaco positions: 1-based columns in the widened text.
  const at = (lines: string[], [line, col]: Pos) => [line, widenColumn(lines[line - 1] ?? "", col, unit) + 1] as const;
  const range = (lines: string[], from: Pos, to: Pos) => new monaco.Range(...at(lines, from), ...at(lines, to));
  const changes = hunks(rows, oldLines, newLines).map(
    (h) =>
      new DetailedLineRangeMapping(
        new LineRange(...h.original),
        new LineRange(...h.modified),
        h.inner.map(([o0, o1, n0, n1]) => new RangeMapping(range(oldLines, o0, o1), range(newLines, n0, n1))),
      ),
  );
  return { changes, identical: !changes.length, quitEarly: false, moves: [] };
}

const highlighter = createHighlighterCore({ themes: [], langs: [], engine: createOnigurumaEngine(import("shiki/wasm")) });
// shikiToMonaco wraps these each time it runs: start every run from the originals.
const { setTheme, create } = monaco.editor;

const tokenization = (model: monaco.editor.ITextModel) =>
  (model as unknown as { tokenization: { backgroundTokenizationState: number; forceTokenization(line: number): void; setSemanticTokens(tokens: null, complete: boolean): void } })
    .tokenization;

/** Colors `model` through `line` now, a slice per task so the UI keeps moving. */
export async function colorThrough(model: monaco.editor.ITextModel, line: number, alive: () => boolean) {
  const SLICE = 400;
  line = Math.min(line, model.getLineCount());
  for (let l = Math.min(line, SLICE); alive(); l = Math.min(line, l + SLICE)) {
    tokenization(model).forceTokenization(l);
    if (l >= line) return;
    await new Promise((r) => setTimeout(r));
  }
}

/**
 * Redraws an inline diff's deleted lines once the old text is fully colored. Monaco draws them with
 * the tokens they have and means to redraw when tokenizing completes, but it watches token changes,
 * whose last one comes before the state flips; an empty semantic-token update brings one after.
 */
export function redrawWhenColored(model: monaco.editor.ITextModel) {
  if (model.getLanguageId() === "plaintext") return () => {};
  const t = tokenization(model);
  // Up to 10s: a model with no tokenizer (a grammar that failed, a file too large) never completes.
  let tries = 200;
  const id = setInterval(() => {
    if (model.isDisposed() || --tries < 0) return clearInterval(id);
    if (t.backgroundTokenizationState !== 2 /* Completed */) return;
    clearInterval(id);
    t.setSemanticTokens(null, true);
  }, 50);
  return () => clearInterval(id);
}

/** Monaco's id for a language: its Shiki id, or plain text. */
const monacoLanguage = (lang: string) => (lang === "text" ? "plaintext" : lang);

/**
 * Models for a file's two versions (the old one only for a diff, which then shows git's `rows`),
 * named by their paths. `unit`: spaces per indentation level turned into tabs for display (see
 * lib/editor/indent), or 0.
 */
export function createModels(lang: string, path: string, modifiedText: string, original: OldSide | null) {
  const i = kept.findIndex(
    (k) =>
      k.lang === lang &&
      k.path === path &&
      k.text === modifiedText &&
      (k.original === original || (!!k.original && !!original && k.original.path === original.path && k.original.text === original.text && sameRows(k.original.rows, original.rows))),
  );
  if (i >= 0) return kept.splice(i, 1)[0].models;
  const unit = indentUnit(modifiedText, original?.text);
  const modified = createModel(modifiedText, lang, unit, viewUri(path));
  const old = original && createModel(original.text, lang, unit, viewUri(original.path));
  if (original) gitDiffs.set(modified, gitDiff(original.rows, original.text, modifiedText, unit));
  const models = { modified, original: old, unit };
  keptBy.set(modified, { lang, path, text: modifiedText, original, models, generation });
  return models;
}

const sameRows = (a: DiffRow[], b: DiffRow[]) =>
  a === b || (a.length === b.length && a.every((r, i) => r.k === b[i].k && r.o === b[i].o && r.n === b[i].n && String(r.e) === String(b[i].e)));

type OldSide = { path: string; text: string; rows: DiffRow[] };
interface Kept {
  lang: string;
  path: string;
  text: string;
  original: OldSide | null;
  models: { modified: monaco.editor.ITextModel; original: monaco.editor.ITextModel | null; unit: number };
  generation: number;
}
// Models of files shown lately, for the same text's next show (as VS Code keeps an open editor's):
// a tab switch remounts the view, and new models each time left hundreds of MB of garbage that
// WebKit collects late. Bounded by count and by text; the newest always stays.
const kept: Kept[] = [];
const keptBy = new WeakMap<monaco.editor.ITextModel, Kept>();
const KEPT = 8;
const KEPT_CHARS = 4_000_000;
const keptChars = (k: Kept) => k.text.length + (k.original?.text.length ?? 0);
// Bumped on a repo switch: the old repo's views release their models only after it.
let generation = 0;

/** For a repo switch: the kept files were the other repo's. */
export function resetModels() {
  generation++;
  for (const k of kept.splice(0)) {
    k.models.modified.dispose();
    k.models.original?.dispose();
  }
}

/** Done showing `models` (a file's, from createModels): kept for its next show, the oldest disposed. */
export function releaseModels(models: monaco.editor.ITextModel[]) {
  const k = models.map((m) => keptBy.get(m)).find((x) => x);
  if (!k || k.generation !== generation) return models.forEach((m) => m.dispose());
  if (kept.includes(k) || k.models.modified.isDisposed()) return;
  kept.push(k);
  let chars = kept.reduce((n, x) => n + keptChars(x), 0);
  while (kept.length > KEPT || (kept.length > 1 && chars > KEPT_CHARS)) {
    const old = kept.shift()!;
    chars -= keptChars(old);
    old.models.modified.dispose();
    old.models.original?.dispose();
  }
}

/** A file Go to Definition shows in its peek or hover, widened as the code view would. */
export const createPeekModel = (text: string, lang: string, uri: monaco.Uri) => createModel(text, lang, indentUnit(text), uri);

/** A file's unsaved edit brought back from storage: never kept for another show (see detachModel). */
export const createEditModel = (text: string, lang: string, path: string) => createModel(text, lang, indentUnit(text), viewUri(path));

/** A model being edited no longer holds the text it's kept under: disposed on release, not kept. */
export function detachModel(model: monaco.editor.ITextModel) {
  keptBy.delete(model);
}

function createModel(text: string, lang: string, unit: number, uri?: monaco.Uri) {
  const wide = widen(text, unit);
  const model = monaco.editor.createModel(wide, monacoLanguage(lang), uri);
  // Tabs are 4 wide everywhere; Monaco guesses per file otherwise (2 in a two-space file). Tab
  // types a tab where indentation is tabs, widened ones included (saved back as the file's spaces).
  model.updateOptions({ tabSize: TAB, indentSize: TAB, insertSpaces: !/^\t/m.test(wide) });
  units.set(model, unit);
  return model;
}
const units = new WeakMap<monaco.editor.ITextModel, number>();

// Go to References lists places by their models' names. Each show makes new models before the
// old ones go, so a name is only unique with a count.
let shows = 0;
const viewUri = (path: string) => monaco.Uri.from({ scheme: "gitviber-view", path: `/${path}`, query: String(++shows) });

/** Spaces per indentation level `model`'s text was widened by (see lib/editor/indent). */
export const unitOf = (model: monaco.editor.ITextModel) => units.get(model) ?? 0;

/**
 * Loads a language and theme into Shiki, hands Monaco every grammar loaded so far, and applies
 * the theme with the app's own backgrounds and diff colors. Await it before showing a file, so
 * it's colored from its first paint. It recolors every open file from scratch whenever it hands
 * Monaco something new (Monaco resets tokens on any theme or tokenizer change): call it for the
 * file being opened, not ahead of time for others.
 */
export function prepare(lang: string, theme: string) {
  // One at a time: two runs of shikiToMonaco at once would each reset the theme under the other.
  return (queue = queue.then(() => load(lang, theme)).catch(() => {}));
}
let queue = Promise.resolve();

async function load(lang: string, theme: string) {
  const h = await highlighter;
  // The theme first: a language that fails to load must not leave the view in Monaco's colors.
  const newTheme = !h.getLoadedThemes().includes(theme);
  if (newTheme) await h.loadTheme(bundledThemes[theme as keyof typeof bundledThemes]);
  if (lang !== "text" && !h.getLoadedLanguages().includes(lang)) {
    try {
      await h.loadLanguage(lang === IGNORE ? ignoreGrammar : bundledLanguages[lang as keyof typeof bundledLanguages]);
    } catch {
      // Not a language Shiki knows: the file shows uncolored, in the right theme.
    }
  }
  if (newTheme || !registered.has(lang)) {
    const known = new Set(monaco.languages.getLanguages().map((l) => l.id));
    for (const id of h.getLoadedLanguages()) if (!known.has(id)) monaco.languages.register({ id });
    Object.assign(monaco.editor, { setTheme, create });
    shikiToMonaco(h, monaco);
    h.getLoadedLanguages().forEach((id) => registered.add(id));
    // It redefined the themes with their own colors.
    applied = "";
  }
  // Even one Shiki couldn't load: there's nothing more to hand over for it.
  registered.add(lang);
  applyTheme(h, theme);
}
/** Languages whose tokenizer Monaco has. */
const registered = new Set<string>(["text"]);

let applied = "";
function applyTheme(h: HighlighterCore, theme: string) {
  const colors = appColors();
  const key = theme + JSON.stringify(colors);
  if (key === applied) return;
  applied = key;
  // Its types are written against monaco-editor-core, the same API under another name.
  const base = textmateThemeToMonacoTheme(h.getTheme(theme)) as monaco.editor.IStandaloneThemeData;
  monaco.editor.defineTheme(theme, { ...base, colors: { ...base.colors, ...colors } });
  monaco.editor.setTheme(theme);
}

// Monaco colors from the app's CSS variables, so the viewer looks like the rest of the app.
const APP_COLORS: Record<string, string> = {
  "editor.background": "--background",
  "editorGutter.background": "--background",
  "editorLineNumber.foreground": "--subtle",
  "editorLineNumber.activeForeground": "--subtle",
  "diffEditor.insertedLineBackground": "--add-bg",
  "diffEditor.removedLineBackground": "--del-bg",
  "diffEditorGutter.insertedLineBackground": "--add-gutter",
  "diffEditorGutter.removedLineBackground": "--del-gutter",
  "diffEditor.insertedTextBackground": "--add-emph",
  "diffEditor.removedTextBackground": "--del-emph",
  "diffEditorOverview.insertedForeground": "--added",
  "diffEditorOverview.removedForeground": "--removed",
  "diffEditor.border": "--border-strong",
  "diffEditor.diagonalFill": "--hatch",
  "diffEditor.unchangedRegionBackground": "--panel",
  "editor.findMatchBackground": "--find-current",
  "editor.findMatchHighlightBackground": "--find-match",
  "editorOverviewRuler.findMatchForeground": "--find-mark",
  "editorLink.activeForeground": "--primary",
  // Go to Definition's peek and its list of places (lib/editor/definitions).
  "peekView.border": "--border-strong",
  "peekViewTitle.background": "--panel",
  "peekViewTitleLabel.foreground": "--foreground",
  "peekViewTitleDescription.foreground": "--subtle",
  "peekViewEditor.background": "--panel",
  "peekViewEditorGutter.background": "--panel",
  "peekViewEditorStickyScroll.background": "--panel",
  "peekViewEditor.matchHighlightBackground": "--find-match",
  "peekViewResult.background": "--sidebar",
  "peekViewResult.fileForeground": "--foreground",
  "peekViewResult.lineForeground": "--muted-foreground",
  "peekViewResult.selectionBackground": "--active",
  "peekViewResult.selectionForeground": "--foreground",
  "peekViewResult.matchHighlightBackground": "--find-match",
  "list.activeSelectionBackground": "--active",
  "list.activeSelectionForeground": "--foreground",
  "list.inactiveSelectionBackground": "--active",
  "list.inactiveSelectionForeground": "--foreground",
  "list.focusBackground": "--active",
  "list.hoverBackground": "--hover",
  "list.focusOutline": "--ring",
  "badge.background": "--elevated",
  "badge.foreground": "--muted-foreground",
  "focusBorder": "--ring",
  // Its context menu, as the app's own.
  "menu.background": "--elevated",
  "menu.foreground": "--foreground",
  "menu.selectionBackground": "--primary",
  "menu.selectionForeground": "--primary-foreground",
  "menu.separatorBackground": "--border",
  "menu.border": "--border-strong",
};

// Monaco takes hex only.
function appColors() {
  return Object.fromEntries(Object.entries(APP_COLORS).map(([key, v]) => [key, toHex(cssVar(v))]));
}
