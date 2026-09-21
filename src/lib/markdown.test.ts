import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import Markdown from "react-markdown";
import { isGitHubHosted, markdownLink, markdownOptions } from "./markdown.ts";

const render = (text: string, idPrefix = "d-") =>
  renderToStaticMarkup(createElement(Markdown, { ...markdownOptions({ idPrefix, repo: "https://github.com/o/r" }), components: { a: markdownLink(() => {}) } }, text));

test("mentions and issue refs link, but not inside links, code or words", () => {
  const html = render('Thanks @octo-cat, fixes #12; not a@b.com, x#3, `@a #4`, <code>@b #5</code> or <a href="https://example.com">see @c #6</a>.');
  assert.match(html, /<a href="https:\/\/github.com\/octo-cat">@octo-cat<\/a>/);
  assert.match(html, /<a href="https:\/\/github.com\/o\/r\/issues\/12">#12<\/a>/);
  assert.match(html, /<a href="https:\/\/example.com">see @c #6<\/a>/);
  assert.match(html, /<code>@b #5<\/code>/);
  assert.match(html, /<code>@a #4<\/code>/);
  assert.doesNotMatch(html, /github.com\/b"|issues\/3"|issues\/5"|issues\/6"/);
});

test("footnote anchors survive the custom link and stay unique per block", () => {
  const text = "Claim[^1].\n\n[^1]: Source.";
  const desc = render(text, "d-");
  const comment = render(text, "c0-");
  // The reference keeps its id so the back-link has a target; hrefs stay bare for followLink.
  assert.match(desc, /<a id="d-fnref-1"[^>]*href="#fn-1"/);
  assert.match(desc, /<li id="d-fn-1">/);
  assert.match(desc, /<a data-footnote-backref[^>]*href="#fnref-1"/);
  assert.match(comment, /id="c0-fnref-1"/);
  assert.doesNotMatch(comment, /id="d-/);
});

test("raw HTML is cut down to the allowlist", () => {
  const html = render(
    '<details><summary>More</summary>\n\n**hidden**\n\n</details>\n\n<img src=x onerror="alert(1)"><script>alert(1)</script><a href="javascript:alert(1)" name="top">js</a><picture><source srcset="https://t.example/p.png"><img src="https://github.com/a.png"></picture><p id="root" style="color:red">s</p>',
  );
  assert.match(html, /<details><summary>More<\/summary>/);
  assert.match(html, /<strong>hidden<\/strong>/);
  assert.doesNotMatch(html, /onerror|<script|javascript:|srcset|<source|<picture|style=/);
  // An emptied link is plain text; its name still works as an anchor.
  assert.match(html, /<span id="d-top">js<\/span>/);
  assert.match(html, /<p id="d-root">/);
});

test("only GitHub's own image hosts load without asking", () => {
  assert.ok(isGitHubHosted("https://github.com/user-attachments/assets/012a2451-fa01-4fe5-8736-33f4a4d179f1"));
  assert.ok(isGitHubHosted("https://camo.githubusercontent.com/abc"));
  assert.ok(isGitHubHosted("https://private-user-images.githubusercontent.com/1/2-x.png?jwt=y"));
  assert.ok(!isGitHubHosted("https://img.shields.io/badge/x"));
  assert.ok(!isGitHubHosted("https://github.com.evil.example/a.png"));
  assert.ok(!isGitHubHosted("http://github.com/a.png"));
});
