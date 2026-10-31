import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// scripts/reader.js, reader mode's page, built from fixture articles as the app runs it: through
// vendor/Readability.js, with the DOM the library's own tests use (tests/js/vendor/JSDOMParser.js).
const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");
const scope: Record<string, any> = {};
new Function(read("vendor/JSDOMParser.js")).call(scope);
const Readability = new Function(`${read("../../src/scripts/vendor/Readability.js")}\nreturn Readability;`)();
const win: Record<string, any> = {};
new Function("window", "Readability", read("../../src/scripts/reader.js"))(win, Readability);
const reader = win.__gvReader;

const URL_ = "https://news.example.com/2026/tide-pools";

/** The reader's page for a fixture, and its markup. */
function page(fixture: string, url = URL_) {
  const doc = new scope.JSDOMParser().parse(read(`fixtures/${fixture}`), url);
  const article = new Readability(doc, { serializer: (el: unknown) => el }).parse();
  assert.ok(article, "Readability finds the article");
  const built = reader.build(article, doc, doc.baseURI);
  return { built, html: built.innerHTML as string };
}

const LOOK = {
  light: { bg: "#ffffff", fg: "#1f2328", muted: "#59636e", link: "#0969da", rule: "#d1d9e0", code: "#f6f8fa" },
  dark: { bg: "#171718", fg: "#ececee", muted: "#a2a2a8", link: "#4a9ff5", rule: "#242426", code: "#1e1e20" },
  size: 19,
  font: "serif",
};

test("the header has the site, the title, the byline and the reading time", () => {
  const { built } = page("reader-article.html");
  assert.equal(built.localName, "article");
  assert.equal(built.className, "reader");
  const head = built.firstChild;
  assert.equal(head.localName, "header");
  const [site, title, meta] = head.childNodes;
  assert.equal(site.className, "site");
  assert.equal(site.textContent, "Example News");
  assert.equal(title.localName, "h1");
  assert.equal(title.textContent, "Why tide pools glow at night");
  assert.equal(meta.textContent, "Ada Marsh · 1 min read");
  assert.equal(built.childNodes[1].className, "body");
});

test("the body keeps the article's text, figures and links, made absolute", () => {
  const { html } = page("reader-article.html");
  assert.match(html, /glow comes from dinoflagellates/);
  assert.match(html, /<a href="https:\/\/news\.example\.com\/science\/burglar-alarm">burglar alarm<\/a>/);
  assert.match(html, /<img src="https:\/\/news\.example\.com\/img\/pool\.jpg" alt="A glowing tide pool"/);
  assert.match(html, /<figcaption>A tide pool at midnight\.<\/figcaption>/);
  assert.match(html, /<pre><code>light = luciferin/);
  assert.match(html, /<blockquote>/);
  // A footnote's link stays in the article, to its note.
  assert.match(html, /<a id="ref-1" href="#note-1">1<\/a>/);
  assert.match(html, /<p id="note-1">/);
  // An unknown tag gives way to its text.
  assert.match(html, /Some species glow for longer/);
  assert.doesNotMatch(html, /<font/);
  // Around the article: the page's menu, sidebar and footer.
  assert.doesNotMatch(html, /Subscribe for more|Copyright Example News|>World</);
});

test("nothing that could run a script or post a form comes through", () => {
  const { html } = page("reader-article.html");
  for (const bad of [/<script/i, /<style/i, /<iframe/i, /<form/i, /<input/i, /<button/i, /onerror/i, /javascript:/i, /style=/i, /class="(?!reader|site|title|meta|body)/]) {
    assert.doesNotMatch(html, bad);
  }
  // The link without a safe address keeps its words, not its target.
  assert.match(html, /\. Read the study to see/);
});

test("only allowed tags and attributes are copied, whatever the article holds", () => {
  const doc = new scope.JSDOMParser().parse(
    `<html><body><div id="c"><p onclick="x()" style="color:red" class="lede" data-x="1" title="t">Hi <svg><script>x()</script></svg>` +
      `<a href="mailto:a@example.com" target="_blank" rel="opener">mail</a> <a href="data:text/html,x">data</a> ` +
      `<img src="data:image/png;base64,AAAA" srcset="/a.png 1x, /b.png 2x" onload="x()"/> ` +
      `<img src="javascript:x()" srcset="javascript:x() 1x"/></p>` +
      `<table><tr><td colspan="2" bgcolor="red">cell</td></tr></table><object data="x.swf">alt</object></div></body></html>`,
    URL_,
  );
  const built = reader.build(
    { title: "T", content: doc.getElementById("c"), textContent: "Hi mail data cell" },
    doc,
    URL_,
  );
  // The article's own element is Readability's wrapper: its children come over.
  const body = built.childNodes[1].innerHTML;
  assert.equal(
    body,
    `<p title="t">Hi <a href="mailto:a@example.com">mail</a> <a>data</a> ` +
      `<img src="data:image/png;base64,AAAA" srcset="https://news.example.com/a.png 1x, https://news.example.com/b.png 2x"/> ` +
      `<img/></p><table><tr><td colspan="2">cell</td></tr></table>`,
  );
  // No site name: the host stands in, without its www.
  const head = reader.build({ title: "", content: doc.getElementById("c"), textContent: "" }, doc, "https://www.example.org/x").firstChild;
  assert.equal(head.childNodes[0].textContent, "example.org");
  assert.equal(head.childNodes[1].textContent, "example.org");
});

test("the article's language and direction carry over", () => {
  const doc = new scope.JSDOMParser().parse(`<html><body><div id="c"><p>שלום</p></div></body></html>`, URL_);
  const built = reader.build({ title: "כותרת", lang: "he", dir: "rtl", content: doc.getElementById("c"), textContent: "שלום" }, doc, URL_);
  assert.equal(built.getAttribute("lang"), "he");
  assert.equal(built.getAttribute("dir"), "rtl");
});

test("reading time: about 230 words a minute, characters for scripts without spaces, a minute at least", () => {
  assert.equal(reader.minutes(""), 1);
  assert.equal(reader.minutes("word ".repeat(230 * 7)), 7);
  assert.equal(reader.minutes("字".repeat(500 * 4)), 4);
});

test("the style takes the app's light colors, its dark ones in dark, and the text's size and face", () => {
  const css: string = reader.css(LOOK);
  assert.match(css, /:host\{[^}]*--bg:#ffffff;[^}]*--link:#0969da;/);
  assert.match(css, /@media \(prefers-color-scheme: dark\)\{:host\{color-scheme:dark;--bg:#171718;/);
  assert.match(css, /font-size:19px;line-height:1\.6/);
  assert.match(css, /font-family:ui-serif/);
  assert.match(css, /max-width:68ch/);
  // The page's own rules for the host can't move or show through it.
  for (const rule of ["display:block", "position:fixed", "inset:0", "z-index:2147483647", "visibility:visible"]) {
    assert.ok(css.includes(`${rule}!important`), rule);
  }
  // The reader's header has its rule, not one the article brings.
  assert.match(css, /\.reader>header\{margin:0 0 2em/);
  assert.doesNotMatch(css, /(^|[\n}])header\{/);
  const sans: string = reader.css({ ...LOOK, size: 24, font: "sans" });
  assert.match(sans, /font-family:-apple-system[^;]*;font-size:24px/);
  // What isn't a color or a size can't reach the style.
  const odd: string = reader.css({ ...LOOK, size: "9999", font: "comic", light: { ...LOOK.light, bg: "red;}*{display:none" } });
  assert.match(odd, /--bg:inherit;/);
  assert.match(odd, /font-size:40px/);
  assert.match(odd, /font-family:ui-serif/);
});

/** reader.js on a stand-in live page: its document a fixture's, with the bits the overlay uses. */
function live(fixture: string, Parser: unknown = Readability) {
  const html = read(`fixtures/${fixture}`);
  const own = new scope.JSDOMParser().parse(html, URL_);
  const props = new Map<string, [string, string]>([["visibility", ["visible", ""]]]);
  const body = own.createElement("body");
  body.style = {
    getPropertyValue: (n: string) => props.get(n)?.[0] ?? "",
    getPropertyPriority: (n: string) => props.get(n)?.[1] ?? "",
    setProperty: (n: string, v: string, p = "") => void props.set(n, [v, p]),
    removeProperty: (n: string) => void props.delete(n),
  };
  const head = own.createElement("head");
  head.setAttribute("inert", "");
  const documentElement = own.createElement("html");
  documentElement.appendChild(head);
  documentElement.appendChild(body);
  const listeners: Record<string, () => void> = {};
  const document = {
    baseURI: URL_,
    URL: URL_,
    body,
    documentElement,
    cloneNode: () => new scope.JSDOMParser().parse(html, URL_),
    createTextNode: (t: string) => own.createTextNode(t),
    createElement: (tag: string) => {
      const el = own.createElement(tag);
      el.focus = () => {};
      el.attachShadow = () => {
        const on: Record<string, (e: unknown) => void> = {};
        return (el.shadow = Object.assign(own.createElement("shadow"), {
          on,
          addEventListener: (type: string, fn: (e: unknown) => void) => void (on[type] = fn),
        }));
      };
      return el;
    },
  };
  class Sheet {
    text = "";
    replaceSync(text: string) {
      this.text = text;
    }
  }
  const window: Record<string, any> = { addEventListener: (type: string, fn: () => void) => void (listeners[type] = fn) };
  new Function("window", "document", "Readability", "CSSStyleSheet", read("../../src/scripts/reader.js"))(window, document, Parser, Sheet);
  return { reader: window.__gvReader, document, body, head, props, listeners };
}

test("shown, the article lies over the page, which is hidden beneath; hidden, the page is as it was", () => {
  const { reader, document, body, head, props, listeners } = live("reader-article.html");
  assert.equal(reader.style(LOOK), "hidden");
  assert.equal(reader.show(LOOK), "shown");
  const host = document.documentElement.lastChild;
  assert.equal(host.localName, "gitviber-reader");
  const [sheet] = host.shadow.adoptedStyleSheets;
  assert.match(sheet.text, /font-size:19px/);
  const page = host.shadow.firstChild;
  assert.equal(page.className, "page");
  assert.match(page.innerHTML, /<h1 class="title">Why tide pools glow at night<\/h1>/);
  assert.deepEqual(props.get("visibility"), ["hidden", "important"]);
  // Inert beneath, for Find in Page and VoiceOver: the reader's host isn't.
  assert.ok(body.hasAttribute("inert"));
  assert.ok(!host.hasAttribute("inert"));
  // Restyled in place, no second article.
  assert.equal(reader.style({ ...LOOK, size: 24 }), "shown");
  assert.match(sheet.text, /font-size:24px/);
  // Shown again: the one before goes first.
  assert.equal(reader.show(LOOK), "shown");
  assert.equal(document.documentElement.childNodes.length, 3);
  assert.equal(reader.hide(), "hidden");
  assert.equal(document.documentElement.childNodes.length, 2);
  assert.deepEqual(props.get("visibility"), ["visible", ""]);
  // What the page had inert stays so; what the reader made inert is let go.
  assert.ok(!body.hasAttribute("inert"));
  assert.ok(head.hasAttribute("inert"));
  assert.equal(reader.hide(), "hidden");
  // Leaving the page takes it down too.
  reader.show(LOOK);
  listeners.pagehide();
  assert.equal(document.documentElement.childNodes.length, 2);
});

test("the page's key handlers don't hear keys meant for the reader, which still scrolls", () => {
  const { reader, document } = live("reader-article.html");
  reader.show(LOOK);
  const { on } = document.documentElement.lastChild.shadow;
  for (const type of ["keydown", "keypress", "keyup"]) {
    let stopped = false;
    let prevented = false;
    on[type]({ stopPropagation: () => (stopped = true), preventDefault: () => (prevented = true) });
    assert.ok(stopped && !prevented, type);
  }
});

test("a parse that throws (a page enforcing Trusted Types) answers none and leaves the page as it was", () => {
  class Blocked {
    parse(): never {
      throw new TypeError("This assignment requires a TrustedHTML");
    }
  }
  const { reader, document, body, props } = live("reader-article.html", Blocked);
  assert.equal(reader.show(LOOK), "none");
  assert.equal(document.documentElement.childNodes.length, 2);
  assert.deepEqual(props.get("visibility"), ["visible", ""]);
  assert.ok(!body.hasAttribute("inert"));
  assert.equal(reader.style(LOOK), "hidden");
});

test("a srcset's URLs may hold commas: candidates part at a comma after a descriptor or a URL's end", () => {
  const doc = new scope.JSDOMParser().parse(
    `<html><body><div id="c"><img srcset="/i/w_400,h_300/a.jpg 1x, /i/w_800,h_600/a.jpg 2x"/>` +
      `<img srcset="/b.jpg,/c.jpg 2x,/d.jpg 3x"/><img srcset="/e.jpg,, /f.jpg 2x"/></div></body></html>`,
    URL_,
  );
  const body = reader.build({ title: "T", content: doc.getElementById("c"), textContent: "" }, doc, URL_).childNodes[1];
  const sets = body.childNodes.map((img: any) => img.getAttribute("srcset"));
  assert.deepEqual(sets, [
    "https://news.example.com/i/w_400,h_300/a.jpg 1x, https://news.example.com/i/w_800,h_600/a.jpg 2x",
    "https://news.example.com/b.jpg,/c.jpg 2x, https://news.example.com/d.jpg 3x",
    "https://news.example.com/e.jpg, https://news.example.com/f.jpg 2x",
  ]);
});

test("a link to a place in the page stays a jump, behind a <base> unlike the page's address too", () => {
  const base = "https://cdn.example.com/";
  const doc = new scope.JSDOMParser().parse(
    `<html><body><div id="c"><a href="${base}#note-1">1</a> <a href="${URL_}#note-2">2</a> ` +
      `<a href="${URL_}?page=2#top">next</a> <a href="${base}guide#install">guide</a></div></body></html>`,
    URL_,
  );
  const body = reader.build({ title: "T", content: doc.getElementById("c"), textContent: "" }, doc, base, URL_).childNodes[1];
  assert.equal(
    body.innerHTML,
    `<a href="#note-1">1</a> <a href="#note-2">2</a> ` +
      `<a href="${URL_}?page=2#top">next</a> <a href="${base}guide#install">guide</a>`,
  );
});
