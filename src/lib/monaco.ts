// Monaco, set up once for a read-only viewer: only the editor features it uses, colored by
// Shiki's TextMate grammars and themes (the ones the rest of the app uses), loaded as files open.
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
import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createOnigurumaEngine } from "shiki/engine/oniguruma";
import { bundledLanguages } from "shiki/langs";
import { bundledThemes } from "shiki/themes";
import type { DiffRow } from "./api";
import { hunks, type Pos } from "./diffHunks";
import { indentUnit, TAB, widen, widenColumn } from "./indent";
import { IGNORE, ignoreGrammar } from "./language";
import { codeFontFamily, getSettings, subscribeSettings } from "./settings";

export { monaco };

// Find opens on editor.find (commands.ts, MonacoView), which the user can rebind; Monaco's own ⌘F would stay behind.
monaco.editor.addKeybindingRule({ keybinding: monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyF, command: "-actions.find" });

// Code fonts load lazily (Geist Mono, JetBrains Mono): measure again once they're in, or wrapping and
// selections keep the fallback font's widths.
document.fonts.addEventListener("loadingdone", () => monaco.editor.remeasureFonts());
// Monaco keeps a font's widths for good once measured, including a custom font's fallback widths
// from before it was installed: a new pick measures afresh.
let codeFont = codeFontFamily(getSettings());
subscribeSettings(() => {
  const next = codeFontFamily(getSettings());
  if (next === codeFont) return;
  codeFont = next;
  monaco.editor.remeasureFonts();
});

// Only a fallback computes diffs (see `createModels`); there are no language services to run.
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
 * Models for a file's two versions (the old one only for a diff, which then shows git's `rows`).
 * `unit`: spaces per indentation level turned into tabs for display (see lib/indent), or 0.
 */
export function createModels(lang: string, modifiedText: string, original: { text: string; rows: DiffRow[] } | null) {
  const id = monacoLanguage(lang);
  const unit = indentUnit(modifiedText, original?.text);
  const modified = monaco.editor.createModel(widen(modifiedText, unit), id);
  const old = original && monaco.editor.createModel(widen(original.text, unit), id);
  // Tabs are 4 wide everywhere; Monaco guesses per file otherwise (2 in a two-space file).
  for (const m of [modified, old]) m?.updateOptions({ tabSize: TAB, indentSize: TAB });
  if (original) gitDiffs.set(modified, gitDiff(original.rows, original.text, modifiedText, unit));
  return { modified, original: old, unit };
}

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
};

function appColors() {
  const css = getComputedStyle(document.documentElement);
  return Object.fromEntries(Object.entries(APP_COLORS).map(([key, v]) => [key, hex(css.getPropertyValue(v).trim())]));
}

// Monaco takes hex only; the canvas resolves any CSS color (rgb() with alpha, var-free values).
const probe = document.createElement("canvas").getContext("2d", { willReadFrequently: true })!;
function hex(color: string) {
  probe.clearRect(0, 0, 1, 1);
  probe.fillStyle = color;
  probe.fillRect(0, 0, 1, 1);
  return "#" + [...probe.getImageData(0, 0, 1, 1).data].map((n) => n.toString(16).padStart(2, "0")).join("");
}
