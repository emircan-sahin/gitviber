import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import Markdown from "react-markdown";
import { type Flavor, markdownOptions } from "../github/markdown.ts";
import { parseWikilink, stripComments, wikilinkLabel } from "./syntax.ts";

const render = (text: string, flavor: Flavor = "obsidian", section?: string) =>
  renderToStaticMarkup(createElement(Markdown, markdownOptions({ idPrefix: "n-", flavor, section }), flavor === "obsidian" ? stripComments(text) : text));

test("comments are cut, except in code", () => {
  assert.equal(stripComments("a %%hidden%% b"), "a  b");
  assert.equal(stripComments("a\n%%\nhidden\n%%\nb"), "a\n\nb");
  assert.equal(stripComments("`%%kept%%` and %%gone%%"), "`%%kept%%` and ");
  assert.equal(stripComments("```\n%%kept%%\n```\n%%gone"), "```\n%%kept%%\n```\n");
});

test("wikilinks split into path, heading or block, and alias", () => {
  assert.deepEqual(parseWikilink("Folder/Note#Part#Sub|Shown"), { path: "Folder/Note", anchor: "Part#Sub", alias: "Shown" });
  assert.equal(wikilinkLabel(parseWikilink("Note#Part#Sub")), "Note > Part > Sub");
  assert.equal(wikilinkLabel(parseWikilink("#Part")), "Part");
  const html = render("See [[Note|this]], [[Other#Part]] and `[[code]]`.");
  assert.match(html, /<a href="#" data-wikilink="Note">this<\/a>/);
  assert.match(html, /<a href="#" data-wikilink="Other#Part">Other &gt; Part<\/a>/);
  assert.match(html, /<code>\[\[code\]\]<\/code>/);
});

test("repo and GitHub markdown leave Obsidian's syntax as text", () => {
  for (const flavor of ["repo", "github"] as const) {
    const html = render("[[Note]] ==hi== #tag %%c%%", flavor);
    assert.match(html, /\[\[Note\]\] ==hi== #tag %%c%%/);
  }
});

test("embeds: alone a block, in text inline", () => {
  assert.match(render("![[Note#Part]]"), /^<div data-embed="Note#Part" data-embed-alias=""><\/div>$/);
  assert.match(render("an ![[pic.png|300]] inline"), /<p>an <span data-embed="pic.png" data-embed-alias="300"><\/span> inline<\/p>/);
});

test("highlights, tags and inline footnotes", () => {
  const html = render("==a **b**== and == no == #tag/sub #123 x#not ^[a [b] note]");
  assert.match(html, /<mark>a <strong>b<\/strong><\/mark>/);
  assert.match(html, /== no ==/);
  assert.match(render("==**bold**== and ==`code`=="), /<mark><strong>bold<\/strong><\/mark> and <mark><code>code<\/code><\/mark>/);
  assert.match(html, /<span data-tag="tag\/sub">#tag\/sub<\/span>/);
  assert.doesNotMatch(html, /data-tag="123"|data-tag="not"/);
  assert.match(html, /<sup><a href="#fn-inline-1"/);
  assert.match(html, /a \[b\] note/);
});

test("block ids land on their block", () => {
  const html = render("A line ^para1\n\n- item ^item1\n\n| a |\n| - |\n| 1 |\n\n^table1");
  assert.match(html, /<p id="n-\^para1">A line<\/p>/);
  assert.match(html, /<li id="n-\^item1"/);
  assert.match(html, /<table id="n-\^table1">/);
  assert.doesNotMatch(html, /\^table1<\/p>/);
});

test("callouts: Obsidian's types, titles and folds; GitHub's five alerts", () => {
  const html = render("> [!faq]- Why **this**?\n> Because.\n\n> [!custom-type]\n> Body");
  assert.match(html, /<div data-callout="question" data-callout-fold="-">\n<div data-callout-title="">Why <strong>this<\/strong>\?<\/div>\n<div data-callout-body=""><p>Because.<\/p>/);
  assert.match(render("> [!constructor] T"), /data-callout="constructor"/);
  assert.match(html, /data-callout="custom-type"[^>]*>\n<div data-callout-title="">Custom type<\/div>/);
  assert.match(render("> [!tip]+ Only a title"), /<div data-callout="tip" data-callout-fold="\+">\n<div data-callout-title="">Only a title<\/div>\n<\/div>/);
  const gh = render("> [!IMPORTANT]\n> Read this.\n\n> [!NOTE] Titled\n> not an alert\n\n> [!faq]\n> not one either", "github");
  assert.match(gh, /<div data-callout="gh-important" data-callout-fold="">\n<div data-callout-title="">Important<\/div>\n<div data-callout-body=""><p>Read this.<\/p>/);
  assert.match(gh, /<blockquote>\n<p>\[!NOTE\] Titled/);
  assert.match(gh, /<blockquote>\n<p>\[!faq\]/);
});

test("math needs no space inside its dollars", () => {
  const html = render("Costs $5 and $10, area $\\pi r^2$.", "github");
  assert.match(html, /Costs \$5 and \$10/);
  assert.match(render("Costs $5 for **this** and $6", "github"), /Costs \$5 for <strong>this<\/strong> and \$6/);
  assert.match(html, /<code class="language-math math-inline">\\pi r\^2<\/code>/);
  assert.match(render("$$\nx^2\n$$", "github"), /<pre><code class="language-math math-display">x\^2<\/code><\/pre>/);
});

test("frontmatter becomes a table's data, in files only", () => {
  assert.match(render("---\ntags: [a]\n---\n# T", "repo"), /^<div data-frontmatter="tags: \[a\]"><\/div>/);
  assert.match(render("---\ntags: [a]\n---\n# T", "github"), /<hr\/>/);
});

test("task statuses beyond done", () => {
  const html = render("- [/] half\n- [x] done\n- [ ] open");
  assert.match(html, /<li data-task="\/">half<\/li>/);
  assert.match(html, /<li class="task-list-item" data-task="x"><input type="checkbox" disabled="" checked=""\/> done<\/li>/);
});

test("an embed of a heading or block keeps just that part", () => {
  const note = "# Top\nintro\n## Part\nin part\n### Deeper\nstill\n## Next\nout\n\nA block ^b1";
  const part = render(note, "obsidian", "Part");
  assert.match(part, /Part[\s\S]*in part[\s\S]*Deeper[\s\S]*still/);
  assert.doesNotMatch(part, /intro|Next|out/);
  assert.match(render(note, "obsidian", "^b1"), /^<p id="n-\^b1">A block<\/p>$/);
  assert.match(render("---\ntags: [a]\n---\nBody", "obsidian", ""), /^<p>Body<\/p>$/);
});

test("raw HTML can't smuggle in more than the plugins add", () => {
  const html = render('<mark onclick="x">m</mark><span data-embed="a" style="x">e</span><a data-wikilink="n" href="javascript:alert(1)">w</a>');
  assert.doesNotMatch(html, /onclick|style=|javascript:/);
  assert.doesNotMatch(render("<mark>m</mark>", "github"), /<mark>/);
});
