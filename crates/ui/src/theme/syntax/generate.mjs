// Turns the old app's 24 Shiki (VS Code TextMate) syntax themes into gpui-component highlight
// themes, one JSON per id in themes/. Takes @shikijs/themes's dist folder (4.4.3 made these,
// from `npm pack @shikijs/themes@4.4.3`):
//   node crates/ui/src/theme/syntax/generate.mjs <shiki-themes-dist-dir>
// The JSON is checked in, so the build never needs node.
//
// Tree-sitter names a token by what it is (`function`, `type`); TextMate by a dotted scope. Each
// capture gets the color the theme gives the scopes VS Code puts on that kind of token, matched
// the way TextMate does: the longest selector that is a prefix of the scope wins.

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const ids = [
  "github-dark-default", "dark-plus", "one-dark-pro", "vitesse-dark", "tokyo-night", "solarized-dark",
  "catppuccin-mocha", "vesper", "houston", "nord", "rose-pine", "kanagawa-wave", "everforest-dark", "poimandres",
  "github-light-default", "light-plus", "one-light", "vitesse-light", "catppuccin-latte", "min-light",
  "solarized-light", "rose-pine-dawn", "kanagawa-lotus", "everforest-light",
];

// Capture → the scopes VS Code gives such a token, best first.
const CAPTURES = {
  attribute: ["entity.other.attribute-name", "meta.attribute"],
  boolean: ["constant.language.boolean", "constant.language"],
  comment: ["comment"],
  comment_doc: ["comment.block.documentation", "comment"],
  constant: ["variable.other.constant", "constant.other", "constant"],
  constructor: ["entity.name.type.class", "entity.name.type", "support.class"],
  embedded: ["meta.embedded"],
  emphasis: ["markup.italic"],
  "emphasis.strong": ["markup.bold"],
  enum: ["entity.name.type.enum", "entity.name.type"],
  function: ["entity.name.function", "support.function", "meta.function-call"],
  hint: ["comment"],
  keyword: ["keyword", "storage.type", "storage"],
  label: ["entity.name.label", "keyword"],
  link_text: ["string.other.link.title.markdown", "string.other.link", "markup.underline.link"],
  link_uri: ["markup.underline.link", "string.other.link"],
  number: ["constant.numeric"],
  operator: ["keyword.operator"],
  predictive: ["comment"],
  preproc: ["meta.preprocessor", "keyword.control.directive", "keyword"],
  property: ["variable.other.property", "variable.other.object.property", "support.type.property-name", "meta.object-literal.key"],
  punctuation: ["punctuation"],
  "punctuation.bracket": ["punctuation.bracket", "meta.brace", "punctuation"],
  "punctuation.delimiter": ["punctuation.separator", "punctuation.terminator", "punctuation"],
  "punctuation.list_marker": ["punctuation.definition.list.begin.markdown", "markup.list", "punctuation"],
  "punctuation.special": ["punctuation.definition.template-expression", "punctuation.section.embedded", "punctuation"],
  string: ["string"],
  "string.escape": ["constant.character.escape", "string"],
  "string.regex": ["string.regexp", "string"],
  "string.special": ["constant.other.symbol", "string"],
  "string.special.symbol": ["constant.other.symbol", "string"],
  tag: ["entity.name.tag"],
  "tag.doctype": ["meta.tag.metadata.doctype", "entity.name.tag"],
  "text.code.span": ["markup.inline.raw", "markup.raw", "string"],
  "text.literal": ["markup.inline.raw", "markup.raw", "string"],
  title: ["markup.heading", "entity.name.section"],
  type: ["entity.name.type", "support.type"],
  variable: ["variable.other.readwrite", "variable.other", "variable"],
  "variable.special": ["variable.language"],
  variant: ["variable.other.enummember", "constant.other.enum", "entity.name.type.enum", "constant"],
};

/** A rule's selectors, plain scopes only: descendant selectors ("a b") and exclusions don't apply here. */
function selectors(rule) {
  const scope = rule.scope;
  if (!scope) return [];
  const list = Array.isArray(scope) ? scope : scope.split(",");
  return list.map((s) => s.trim()).filter((s) => s && !/\s/.test(s));
}

/** The setting `key` for `scope`: the longest matching selector's, a later rule winning a tie. */
function lookup(rules, scope, key) {
  let best = null;
  let bestLen = -1;
  for (const rule of rules) {
    const value = rule.settings?.[key];
    if (value === undefined) continue;
    for (const sel of selectors(rule)) {
      if (scope === sel || scope.startsWith(`${sel}.`)) {
        const len = sel.split(".").length;
        if (len >= bestLen) [best, bestLen] = [value, len];
      }
    }
  }
  return best;
}

/** #RGB, #RGBA, #RRGGBB, #RRGGBBAA as #rrggbb[aa]; null if it isn't one. */
function hex(color) {
  if (typeof color !== "string" || !color.startsWith("#")) return null;
  let h = color.slice(1).toLowerCase();
  if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
  if (h.length !== 6 && h.length !== 8) return null;
  if (h.endsWith("ff") && h.length === 8) h = h.slice(0, 6);
  return `#${h}`;
}

function style(rules, scopes) {
  let color = null;
  let fontStyle = null;
  for (const scope of scopes) {
    color ??= hex(lookup(rules, scope, "foreground"));
    fontStyle ??= lookup(rules, scope, "fontStyle");
    if (color) break;
  }
  if (!color && fontStyle === null) return null;
  const out = {};
  if (color) out.color = color;
  if (typeof fontStyle === "string") {
    if (fontStyle.includes("italic")) out.font_style = "italic";
    if (fontStyle.includes("bold")) out.font_weight = 700;
  }
  return Object.keys(out).length ? out : null;
}

const dist = process.argv[2];
if (!dist) {
  console.error("usage: node crates/ui/src/theme/syntax/generate.mjs <shiki-themes-dist-dir>");
  process.exit(1);
}

for (const id of ids) {
  const theme = (await import(join(dist, `${id}.mjs`))).default;
  const rules = theme.tokenColors ?? [];
  const root = rules.find((r) => !r.scope)?.settings ?? {};
  const syntax = {};
  for (const [capture, scopes] of Object.entries(CAPTURES)) {
    const s = style(rules, scopes);
    if (s) syntax[capture] = s;
  }
  const fg = hex(theme.colors?.["editor.foreground"]) ?? hex(root.foreground);
  if (fg) syntax.primary = { color: fg };
  const out = {
    name: id,
    appearance: theme.type === "light" ? "light" : "dark",
    style: {
      ...(fg ? { "editor.foreground": fg } : {}),
      syntax,
    },
  };
  writeFileSync(join(here, "themes", `${id}.json`), `${JSON.stringify(out)}\n`);
}
console.log(`wrote ${ids.length} themes`);
