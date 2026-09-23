// Monaco, set up once for a read-only viewer: only the editor features it uses, colored by
// Shiki's TextMate grammars and themes (the ones the rest of the app uses), loaded as files open.
import { shikiToMonaco, textmateThemeToMonacoTheme } from "@shikijs/monaco";
import * as monaco from "monaco-editor/editor/editor.api";
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
import { IGNORE, ignoreGrammar } from "./language";

export { monaco };

// The diff is computed in a worker; there are no language services to run.
globalThis.MonacoEnvironment = { getWorker: () => new EditorWorker() };

const highlighter = createHighlighterCore({ themes: [], langs: [], engine: createOnigurumaEngine(import("shiki/wasm")) });
// shikiToMonaco wraps these each time it runs: start every run from the originals.
const { setTheme, create } = monaco.editor;

/** Monaco's id for a language: its Shiki id, or plain text. */
export const monacoLanguage = (lang: string) => (lang === "text" ? "plaintext" : lang);

/**
 * Loads a language and theme into Shiki, hands Monaco every grammar loaded so far, and applies
 * the theme with the app's own backgrounds and diff colors. Await it before showing a file, so
 * it's colored from its first paint.
 */
export function prepare(lang: string, theme: string) {
  // One at a time: two runs of shikiToMonaco at once would each reset the theme under the other.
  return (queue = queue.then(() => load(lang, theme)).catch(() => {}));
}
let queue = Promise.resolve();

async function load(lang: string, theme: string) {
  const h = await highlighter;
  let loaded = false;
  if (lang !== "text" && !h.getLoadedLanguages().includes(lang)) {
    await h.loadLanguage(lang === IGNORE ? ignoreGrammar : bundledLanguages[lang as keyof typeof bundledLanguages]);
    loaded = true;
  }
  if (!h.getLoadedThemes().includes(theme)) {
    await h.loadTheme(bundledThemes[theme as keyof typeof bundledThemes]);
    loaded = true;
  }
  if (loaded) {
    const known = new Set(monaco.languages.getLanguages().map((l) => l.id));
    for (const id of h.getLoadedLanguages()) if (!known.has(id)) monaco.languages.register({ id });
    Object.assign(monaco.editor, { setTheme, create });
    shikiToMonaco(h, monaco);
    // It redefined the themes with their own colors.
    applied = "";
  }
  applyTheme(h, theme);
}

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
