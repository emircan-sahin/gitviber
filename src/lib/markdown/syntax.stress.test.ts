// Notes as they come from real vaults and from hostile ones: markup trying to get out of the
// sanitizer, code that looks like Obsidian syntax, prices next to math, and long notes.
import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import Markdown from "react-markdown";
import { type Flavor, markdownOptions } from "../github/markdown.ts";
import { stripComments } from "./syntax.ts";

const render = (text: string, flavor: Flavor = "obsidian", section?: string) =>
  renderToStaticMarkup(createElement(Markdown, markdownOptions({ idPrefix: "n-", flavor, section }), flavor === "obsidian" ? stripComments(text) : text));

// In a tag, not in text: escaped text may well read "onclick=".
const HANDLERS = /<[^>]*\s(on[a-z]+|style)=|<[^>]*(href|src)="\s*(javascript|data):|<(script|iframe|object|embed)\b/i;

test("hostile wikilinks, aliases, callout titles and raw HTML render inert", () => {
  const html = render(
    [
      '[[a" onmouseover="alert(1)|<img src=x onerror=alert(1)>]]',
      "[[Note|<script>alert(1)</script>]] ![[x.png|<svg onload=alert(1)>]]",
      "> [!note] <img src=x onerror=alert(1)> **title**",
      "> body <iframe src=javascript:alert(1)></iframe>",
      "",
      '> [!x" onclick="y] t',
      "",
      "[click](javascript:alert(1)) [data](data:text/html,<script>alert(1)</script>) ![img](javascript:alert(1))",
      "",
      '<a data-wikilink="Note" href="javascript:alert(1)" onclick="x">w</a><span data-tag="t" onmouseover="x">#t</span>',
      '<div data-embed="Note" style="position:fixed;inset:0">e</div><mark onclick="x">m</mark>',
      '<li data-task="x" onclick="y">t</li><object data="x"></object><embed src="x">',
    ].join("\n"),
  );
  assert.doesNotMatch(html, HANDLERS);
});

test("a wikilink to a URL-looking target stays a wikilink, never a live link", () => {
  const html = render("[[javascript:alert(1)]] and [[https://example.com/x]]");
  assert.match(html, /<a href="#" data-wikilink="javascript:alert\(1\)">/);
  assert.doesNotMatch(html, /href="javascript|href="https/);
});

test("frontmatter values reach the table as data, not markup", () => {
  const html = render('---\ntitle: "<script>alert(1)</script>"\nlink: "[[Note]]"\n---\nx', "repo");
  assert.match(html, /^<div data-frontmatter="title: &quot;&lt;script&gt;alert\(1\)&lt;\/script&gt;&quot;\nlink: &quot;\[\[Note\]\]&quot;"><\/div>/);
  assert.doesNotMatch(html, /<script/);
});

test("KaTeX's \\href and \\url stay math source, which the formula component renders untrusted", () => {
  const html = render("$\\href{javascript:alert(1)}{x}$ and $\\url{javascript:alert(2)}$", "github");
  assert.doesNotMatch(html, /href="javascript/);
  assert.match(html, /<code class="language-math math-inline">\\href\{javascript:alert\(1\)\}\{x\}<\/code>/);
});

test("%% inside code in a callout or a list is code, not a comment that hides the rest of the note", () => {
  // printf formats, SQL LIKE patterns and Mermaid's own %% comments are all common in fenced code.
  const callout = render('> [!example] Formatting\n> ```py\n> print("100%%")\n> ```\n\nAfter the callout');
  assert.match(callout, /print\(&quot;100%%&quot;\)/);
  assert.match(callout, /After the callout/);
  const list = render("- Architecture\n\n    ```mermaid\n    %% services\n    graph TD\n    ```\n\nAfter the list");
  assert.match(list, /%% services/);
  assert.match(list, /After the list/);
  const quote = render("> ```sql\n> SELECT * FROM t WHERE name LIKE '%%x%%';\n> ```\n\nend");
  assert.match(quote, /LIKE &#x27;%%x%%&#x27;/);
});

test("comments outside code still go, inline and across lines", () => {
  assert.equal(render("a %%hidden%% b\n\n%%\nhidden block\n%%\n\nc"), "<p>a  b</p>\n<p>c</p>");
  assert.match(render("```\n%%kept%%\n```\n~~~~\n%% kept too\n~~~~\nend"), /%%kept%%[\s\S]*%% kept too[\s\S]*<p>end<\/p>/);
});

test("prices stay text and math stays math", () => {
  const cases: [string, RegExp][] = [
    ["Costs $5 and $10.", /<p>Costs \$5 and \$10\.<\/p>/],
    ["From $5,000-$10,000 or $20/$30", /<p>From \$5,000-\$10,000 or \$20\/\$30<\/p>/],
    ["between $5 and $6 and $7", /<p>between \$5 and \$6 and \$7<\/p>/],
    ["\\$5 and \\$10 and `$$` and `$x$`", /<p>\$5 and \$10 and <code>\$\$<\/code> and <code>\$x\$<\/code><\/p>/],
    ["Area $\\pi r^2$, then $x$.", /math-inline">\\pi r\^2<\/code>, then <code class="language-math math-inline">x<\/code>/],
    ["```\n$$ not math $$\n```", /<pre><code>\$\$ not math \$\$\n<\/code><\/pre>/],
  ];
  for (const flavor of ["github", "obsidian"] as const) for (const [src, want] of cases) assert.match(render(src, flavor), want, `${flavor}: ${src}`);
  // Obsidian's syntax after a price pair still works.
  assert.match(render("Paid $5 and $10 for [[Lunch]] #food"), /Paid \$5 and \$10 for <a href="#" data-wikilink="Lunch">Lunch<\/a> <span data-tag="food">#food<\/span>/);
});

test("a wikilink whose name has two prices in it is still one link", () => {
  assert.match(render("See [[Budget $5k vs $10k]]."), /<a href="#" data-wikilink="Budget \$5k vs \$10k">Budget \$5k vs \$10k<\/a>/);
});

test("a wikilink with an escaped alias bar in a table keeps its alias", () => {
  assert.match(render("| a | b |\n| - | - |\n| [[Note\\|Alias]] | x |"), /<td><a href="#" data-wikilink="Note">Alias<\/a><\/td>/);
});

test("an embed of a heading written as Obsidian links it finds the heading", () => {
  // Obsidian leaves out what a link can't hold (":", "#") when it links a heading; scrolling
  // to it (VaultMarkdown's anchorTarget) already compares slugs, so both forms land.
  const note = "# Top\n## Q&A: Part 1\nanswer\n## Tagged #todo\ntagged\n## After\nout";
  assert.match(render(note, "obsidian", "Q&A: Part 1"), /answer/);
  assert.match(render(note, "obsidian", "Q&A Part 1"), /answer/);
  assert.match(render(note, "obsidian", "Tagged todo"), /tagged/);
});

test("a long note written line by line (one paragraph) costs little more than the same text as a repo file", () => {
  const lines = Array.from({ length: 2000 }, (_, i) => `${i} met [[Person ${i % 50}]] about ==this== #project/${i % 7} ^[n${i}]`).join("\n");
  // The best of three: one run on a loaded machine (other test processes) missed by 2%.
  const time = (flavor: Flavor) => {
    render(lines, flavor);
    let took = Infinity;
    let html = "";
    for (let i = 0; i < 3; i++) {
      const start = performance.now();
      html = render(lines, flavor);
      took = Math.min(took, performance.now() - start);
    }
    return { took, html };
  };
  const repo = time("repo");
  const vault = time("obsidian");
  assert.match(vault.html, /data-wikilink="Person 49"/);
  // GFM alone is superlinear on one long paragraph; the vault's bigger tree (marks, tags,
  // footnotes) costs a few times more, but a quadratic plugin would cost far more than this.
  assert.ok(vault.took < repo.took * 3 + 500, `${Math.round(vault.took)} ms (repo flavor: ${Math.round(repo.took)} ms)`);
});

test("many unclosed highlights and inline footnotes don't stall a note", () => {
  const start = performance.now();
  render("a ==b ".repeat(3000));
  render("x ^[a ".repeat(3000));
  assert.ok(performance.now() - start < 2000, `${Math.round(performance.now() - start)} ms`);
});
