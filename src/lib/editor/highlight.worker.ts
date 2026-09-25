/// <reference lib="webworker" />
// Syntax highlighting runs here so tokenizing a big file never blocks scrolling.
import { createHighlighterCore } from "shiki/core";
import { createOnigurumaEngine } from "shiki/engine/oniguruma";
import { bundledLanguages } from "shiki/langs";
import { bundledThemes } from "shiki/themes";
import { IGNORE, ignoreGrammar } from "./language";

type Req = { id: number; code: string; lang: string; theme: string };

// Longer lines (minified code) stay plain: they cost more to tokenize than coloring is worth.
const MAX_LINE = 4000;

// Oniguruma, not the JS regex engine: in JavaScriptCore (the macOS webview) the JS engine
// took 570ms to tokenize a 684-line .tsx file (2.4s the first time) against 56ms here.
const highlighter = createHighlighterCore({ themes: [], langs: [], engine: createOnigurumaEngine(import("shiki/wasm")) });

self.onmessage = async (e: MessageEvent<Req>) => {
  const { id, code, lang, theme } = e.data;
  try {
    const h = await highlighter;
    if (!h.getLoadedThemes().includes(theme)) await h.loadTheme(bundledThemes[theme as keyof typeof bundledThemes]);
    if (!h.getLoadedLanguages().includes(lang)) await h.loadLanguage(lang === IGNORE ? ignoreGrammar : bundledLanguages[lang as keyof typeof bundledLanguages]);
    const lines = h.codeToTokensBase(code, { lang, theme, tokenizeMaxLineLength: MAX_LINE });
    // Compact [text, color, fontStyle] tuples: far cheaper to post back than token objects.
    const out = lines.map((line) => line.map((t) => [t.content, t.color ?? "", t.fontStyle ?? 0]));
    self.postMessage({ id, lines: out, fg: h.getTheme(theme).fg });
  } catch (err) {
    self.postMessage({ id, error: String(err) });
  }
};
