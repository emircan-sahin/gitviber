/// <reference lib="webworker" />
// Syntax highlighting runs here so tokenizing a big file never blocks scrolling.
import { createHighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { bundledLanguages } from "shiki/langs";
import { bundledThemes } from "shiki/themes";
import { IGNORE } from "./language";

// VS Code's grammar for .gitignore and friends: comments and negations are all it colors.
const ignoreGrammar = {
  name: IGNORE,
  scopeName: "source.ignore",
  patterns: [
    { match: "^\\s*#.*", name: "comment.line.number-sign.ignore" },
    { match: "^\\s*!", name: "keyword.operator.negation.ignore" },
  ],
  repository: {},
};

type Req = { id: number; code: string; lang: string; theme: string };

const highlighter = createHighlighterCore({ themes: [], langs: [], engine: createJavaScriptRegexEngine({ forgiving: true }) });

self.onmessage = async (e: MessageEvent<Req>) => {
  const { id, code, lang, theme } = e.data;
  try {
    const h = await highlighter;
    if (!h.getLoadedThemes().includes(theme)) await h.loadTheme(bundledThemes[theme as keyof typeof bundledThemes]);
    if (!h.getLoadedLanguages().includes(lang)) await h.loadLanguage(lang === IGNORE ? ignoreGrammar : bundledLanguages[lang as keyof typeof bundledLanguages]);
    const lines = h.codeToTokensBase(code, { lang, theme });
    // Compact [text, color, fontStyle] tuples: far cheaper to post back than token objects.
    const out = lines.map((line) => line.map((t) => [t.content, t.color ?? "", t.fontStyle ?? 0]));
    self.postMessage({ id, lines: out, fg: h.getTheme(theme).fg });
  } catch (err) {
    self.postMessage({ id, error: String(err) });
  }
};
