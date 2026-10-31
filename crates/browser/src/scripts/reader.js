// Reader mode (browser/reader.rs), in GitViber's own content world on the page's main frame, run
// just after vendor/Readability.js: the article it finds, rebuilt from allowed tags and attributes
// in a closed shadow root over the page, which stays as it was beneath. Nothing that can carry a
// script (an element, an on* attribute, a javascript: link) is copied, so none of the page's runs.
(() => {
  if (window.__gvReader) return;
  const KEEP = new Set(
    ("a abbr article aside b bdi bdo blockquote br caption cite code col colgroup dd del details dfn div dl dt em " +
      "figcaption figure footer h1 h2 h3 h4 h5 h6 header hr i img ins kbd li main mark ol p picture pre q s samp " +
      "section small source span strong sub summary sup table tbody td tfoot th thead time tr u ul var wbr").split(" "),
  );
  // With what's inside them; any other tag (font, center, a custom element) gives way to its children.
  const DROP = new Set(
    ("applet area audio base button canvas dialog embed form frame frameset head iframe input link map math meta " +
      "noscript object option script select style svg template textarea title track video").split(" "),
  );
  const SHARED = ["id", "title", "lang", "dir"];
  const ATTRS = {
    a: ["href"],
    img: ["src", "srcset", "sizes", "alt", "width", "height"],
    source: ["srcset", "sizes", "media", "type"],
    ol: ["start", "reversed", "type"],
    li: ["value"],
    blockquote: ["cite"],
    q: ["cite"],
    del: ["cite", "datetime"],
    ins: ["cite", "datetime"],
    time: ["datetime"],
    td: ["colspan", "rowspan", "headers"],
    th: ["colspan", "rowspan", "headers", "scope", "abbr"],
    col: ["span"],
    colgroup: ["span"],
    details: ["open"],
  };
  const FONTS = {
    serif: 'ui-serif, "New York", "Iowan Old Style", Charter, Georgia, serif',
    sans: '-apple-system, system-ui, "Helvetica Neue", sans-serif',
  };
  const UI_FONT = '-apple-system, system-ui, "Helvetica Neue", sans-serif';

  /** An http(s) address (or, for an image, a data: one) made absolute; null for anything else. */
  const web = (value, base, image) => {
    try {
      const url = new URL(value.trim(), base);
      const ok = url.protocol === "http:" || url.protocol === "https:" || (image && /^data:image\//i.test(url.href));
      return ok ? url.href : null;
    } catch {
      return null;
    }
  };

  /** A srcset's candidates as [url, descriptors]: a URL (which may hold commas) runs to a space,
   *  or ends its candidate with trailing commas; its descriptors run to the next comma. */
  function candidates(v) {
    const list = [];
    let rest = v;
    for (;;) {
      rest = rest.replace(/^[\s,]+/, "");
      if (!rest) return list;
      const url = rest.match(/^\S+/)[0];
      rest = rest.slice(url.length);
      if (url.endsWith(",")) {
        list.push([url.replace(/,+$/, ""), ""]);
        continue;
      }
      const end = rest.indexOf(",");
      list.push([url, (end < 0 ? rest : rest.slice(0, end)).trim()]);
      rest = end < 0 ? "" : rest.slice(end + 1);
    }
  }

  /** `url` minus its fragment. */
  const unfragmented = (url) => url.split("#")[0];

  /** A URL attribute's value as copied, or null to leave it out. */
  const address = (name, value, base, page) => {
    const v = value.trim();
    if (name === "href") {
      if (v.startsWith("#") || /^mailto:/i.test(v)) return v;
      const to = web(v, base, false);
      // Readability resolves "#note" against a <base> unlike the page's address: a link back to
      // the page or that base stays a jump in the article rather than a load of another page.
      const at = to ? to.indexOf("#") : -1;
      const own = at > 0 && [page, base].some((u) => unfragmented(u) === to.slice(0, at));
      return own ? to.slice(at) : to;
    }
    if (name === "src") return web(v, base, true);
    if (name === "srcset") {
      const parts = candidates(v);
      const urls = parts.map(([url]) => web(url, base, false));
      return urls.every(Boolean) ? parts.map(([, d], i) => (d ? `${urls[i]} ${d}` : urls[i])).join(", ") : null;
    }
    if (name === "cite") return web(v, base, false);
    return value;
  };

  /** `from`'s children as allowed, appended to `into`, made by `doc`. */
  function copy(into, from, doc, base, page) {
    for (const node of Array.from(from.childNodes)) {
      if (node.nodeType === 3) {
        into.appendChild(doc.createTextNode(node.textContent));
        continue;
      }
      const tag = node.nodeType === 1 ? (node.localName || "").toLowerCase() : "";
      if (!tag || DROP.has(tag)) continue;
      if (!KEEP.has(tag)) {
        copy(into, node, doc, base, page);
        continue;
      }
      const el = doc.createElement(tag);
      for (const name of SHARED.concat(ATTRS[tag] || [])) {
        const value = node.getAttribute(name);
        const kept = value == null ? null : address(name, value, base, page);
        if (kept != null) el.setAttribute(name, kept);
      }
      copy(el, node, doc, base, page);
      into.appendChild(el);
    }
  }

  /** Minutes to read, at about 230 words a minute; scripts without spaces by their characters. */
  function minutes(text) {
    const words = (text.match(/\S+/g) || []).length;
    const wide = (text.match(/[぀-ヿ㐀-鿿가-힯]/g) || []).length;
    return Math.max(1, Math.round(words / 230 + wide / 500));
  }

  const element = (doc, tag, className, text) => {
    const el = doc.createElement(tag);
    if (className) el.className = className;
    if (text) el.textContent = text;
    return el;
  };

  /** The reader's page for Readability's `article` (its content an element): header, then body;
   *  `url` the page's own address where a <base> sets another. */
  function build(article, doc, base, url = base) {
    const page = element(doc, "article", "reader");
    if (article.lang) page.setAttribute("lang", article.lang);
    if (article.dir) page.setAttribute("dir", article.dir);
    const head = element(doc, "header");
    let host = "";
    try {
      host = new URL(base).hostname.replace(/^www\./, "");
    } catch {}
    const site = (article.siteName || host || "").trim();
    const byline = (article.byline || "").trim();
    if (site) head.appendChild(element(doc, "p", "site", site));
    head.appendChild(element(doc, "h1", "title", (article.title || site).trim()));
    const time = `${minutes(article.textContent || "")} min read`;
    const meta = byline && byline !== site ? `${byline} · ${time}` : time;
    head.appendChild(element(doc, "p", "meta", meta));
    page.appendChild(head);
    const body = element(doc, "div", "body");
    copy(body, article.content, doc, base, url);
    page.appendChild(body);
    return page;
  }

  const color = (c) => (/^#[0-9a-f]{3,8}$/i.test(c || "") ? c : "inherit");
  const vars = (c) =>
    `--bg:${color(c.bg)};--fg:${color(c.fg)};--muted:${color(c.muted)};--link:${color(c.link)};` +
    `--rule:${color(c.rule)};--code:${color(c.code)};`;

  /** The page's style: the app's light colors, its dark ones as the page shows dark, the text's size and face. */
  function css(look) {
    const size = Math.min(40, Math.max(10, Number(look.size) || 19));
    const font = FONTS[look.font] || FONTS.serif;
    // Important: the page's own rules for the host outweigh :host's otherwise.
    return `:host{all:initial;display:block!important;position:fixed!important;inset:0!important;z-index:2147483647!important;
visibility:visible!important;color-scheme:light;${vars(look.light)}}
@media (prefers-color-scheme: dark){:host{color-scheme:dark;${vars(look.dark)}}}
.page{position:absolute;inset:0;overflow:auto;overscroll-behavior:contain;outline:none;background:var(--bg);color:var(--fg);
font-family:${font};font-size:${size}px;line-height:1.6;-webkit-font-smoothing:antialiased;text-rendering:optimizeLegibility;
overflow-wrap:break-word;hyphens:auto}
.reader{display:block;max-width:68ch;margin:0 auto;padding:56px 28px 120px}
.reader>header{margin:0 0 2em;padding:0 0 1.2em;border-bottom:1px solid var(--rule)}
.site,.meta{font-family:${UI_FONT};color:var(--muted)}
.site{margin:0 0 .7em;font-size:.7em;font-weight:600;letter-spacing:.08em;text-transform:uppercase}
.title{margin:0;font-size:2em;line-height:1.2;letter-spacing:-.01em;font-weight:700}
.meta{margin:.9em 0 0;font-size:.78em}
.body h1,.body h2{font-size:1.45em;line-height:1.3;margin:1.8em 0 .6em}
.body h3{font-size:1.2em;line-height:1.35;margin:1.6em 0 .5em}
.body h4,.body h5,.body h6{font-size:1em;margin:1.4em 0 .4em}
p,ul,ol,dl,blockquote,pre,table,details{margin:0 0 1.1em}
li{margin:.25em 0}
a{color:var(--link);text-decoration:underline;text-decoration-thickness:1px;text-underline-offset:.15em}
img{max-width:100%;height:auto}
figure{margin:1.6em 0}
figure img,p>img:only-child{display:block;margin:0 auto;border-radius:4px}
figcaption{margin-top:.6em;font-family:${UI_FONT};font-size:.75em;color:var(--muted)}
blockquote{margin-left:0;padding-left:1em;border-left:3px solid var(--rule);color:var(--muted)}
code,kbd,samp,pre{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:.85em}
code{background:var(--code);padding:.1em .3em;border-radius:4px}
pre{background:var(--code);padding:1em;border-radius:6px;overflow:auto;line-height:1.45;hyphens:none}
pre code{background:none;padding:0;font-size:1em}
table{display:block;overflow:auto;border-collapse:collapse;font-size:.9em}
th,td{border:1px solid var(--rule);padding:.4em .6em;text-align:start}
hr{border:0;border-top:1px solid var(--rule);margin:2em 0}
::selection{background:color-mix(in srgb,var(--link) 30%,transparent)}`;
  }

  let shown = null;

  /** A link to a place in the article goes there in the reader, not on the page beneath. */
  const jump = (root) => (e) => {
    const a = e.target && e.target.closest && e.target.closest('a[href^="#"]');
    if (!a) return;
    e.preventDefault();
    let id = a.getAttribute("href").slice(1);
    try {
      id = decodeURIComponent(id);
    } catch {}
    const to = id && root.getElementById(id);
    if (to) to.scrollIntoView({ block: "start" });
  };

  function hide() {
    if (!shown) return "hidden";
    shown.host.remove();
    for (const el of shown.inert) el.removeAttribute("inert");
    const body = document.body;
    if (body) {
      if (shown.visibility[0]) body.style.setProperty("visibility", ...shown.visibility);
      else body.style.removeProperty("visibility");
    }
    shown = null;
    return "hidden";
  }

  /** The reader's host with its page, made before the page is touched; null without an article. */
  function make(look) {
    const article = new Readability(document.cloneNode(true), { serializer: (el) => el }).parse();
    if (!article || !article.content) return null;
    const host = document.createElement("gitviber-reader");
    const root = host.attachShadow({ mode: "closed" });
    // Constructed, not a <style>: a page's style-src doesn't stop it.
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css(look));
    root.adoptedStyleSheets = [sheet];
    const page = element(document, "div", "page");
    page.tabIndex = -1;
    page.appendChild(build(article, document, document.baseURI, document.URL));
    root.appendChild(page);
    root.addEventListener("click", jump(root));
    // The page's own key handlers (j/k, one cancelling Space) stay out; scrolling is the default.
    for (const type of ["keydown", "keypress", "keyup"]) root.addEventListener(type, (e) => e.stopPropagation());
    return { host, sheet, page };
  }

  /** "shown", or "none" where Readability finds no article. */
  function show(look) {
    hide();
    let made = null;
    // A page enforcing Trusted Types stops Readability's innerHTML: no article, not silence.
    try {
      made = make(look);
    } catch {}
    if (!made) return "none";
    const { host, sheet, page } = made;
    // Hidden and inert beneath, so Find in Page and VoiceOver meet the article only.
    const body = document.body;
    const visibility = body
      ? [body.style.getPropertyValue("visibility"), body.style.getPropertyPriority("visibility")]
      : ["", ""];
    if (body) body.style.setProperty("visibility", "hidden", "important");
    const inert = Array.from(document.documentElement.children).filter((el) => !el.hasAttribute("inert"));
    for (const el of inert) el.setAttribute("inert", "");
    document.documentElement.appendChild(host);
    page.focus({ preventScroll: true });
    // Gone as the page goes: one the back-forward cache keeps comes back as it was.
    window.addEventListener("pagehide", hide, { once: true });
    shown = { host, sheet, visibility, inert };
    return "shown";
  }

  /** The text's size and face, or the colors, changed while it shows. */
  function style(look) {
    if (shown) shown.sheet.replaceSync(css(look));
    return shown ? "shown" : "hidden";
  }

  window.__gvReader = { show, hide, style, build, css, minutes };
})();
