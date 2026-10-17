// The page read: the tree with its refs, find, get, what a wait asks, and the screenshot's view.

/** Lines a snapshot gives at most: past them, -s, -d or -i narrow it. */
const LINES = 1500;

const INTERACTIVE = new Set(["link", "button", "textbox", "searchbox", "checkbox", "radio", "combobox", "slider", "spinbutton", "switch", "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "option", "treeitem", "clickable"]);

// Named by their text: what's inside says nothing more.
const LEAF = new Set(["link", "button", "textbox", "searchbox", "checkbox", "radio", "combobox", "slider", "spinbutton", "switch", "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "option", "treeitem", "img", "progressbar", "meter", "iframe"]);

// Named by their text too, but what acts inside them (a title's link) still gets a ref.
const TITLES = new Set(["heading"]);

const TAGS = { button: "button", select: "combobox", textarea: "textbox", nav: "navigation", main: "main", header: "banner", footer: "contentinfo", aside: "complementary", form: "form", dialog: "dialog", ul: "list", ol: "list", li: "listitem", table: "table", tr: "row", th: "columnheader", td: "cell", summary: "button", progress: "progressbar", meter: "meter", option: "option", fieldset: "group", details: "group", article: "article", iframe: "iframe" };

const INPUTS = { checkbox: "checkbox", radio: "radio", range: "slider", number: "spinbutton", search: "searchbox", button: "button", submit: "button", reset: "button", image: "button", file: "button", color: "button", hidden: null };

const SKIP = new Set(["script", "style", "noscript", "template", "head", "svg", "canvas"]);

// What a clickable div may hold that an agent acts on by itself: then it's no leaf.
const ACTIONABLE = "a[href], button, input, select, textarea, [role=button], [role=link], [role=checkbox], [role=tab]";

function roleOf(el) {
  const explicit = el.getAttribute("role")?.trim().split(/\s+/)[0];
  if (explicit) return explicit === "none" || explicit === "presentation" ? null : explicit;
  const tag = el.localName;
  if (tag === "a") return el.hasAttribute("href") ? "link" : null;
  if (tag === "input") {
    const type = (el.getAttribute("type") || "text").toLowerCase();
    return type in INPUTS ? INPUTS[type] : "textbox";
  }
  if (tag === "img") return el.getAttribute("alt") === "" ? null : "img";
  if (/^h[1-6]$/.test(tag)) return "heading";
  const editable = el.getAttribute("contenteditable");
  if (editable === "" || editable === "true" || editable === "plaintext-only") return "textbox";
  return TAGS[tag] ?? null;
}

/** Its accessible name, near enough; `byText` for the roles a text names (a button's). */
function nameOf(el, byText) {
  const label = flat(el.getAttribute("aria-label"));
  if (label) return label;
  const by = el.getAttribute("aria-labelledby");
  const labelled = by && flat(by.split(/\s+/).map((id) => document.getElementById(id)?.textContent ?? "").join(" "));
  if (labelled) return labelled;
  const tag = el.localName;
  if (tag === "input" || tag === "textarea" || tag === "select") {
    const labels = flat([...(el.labels ?? [])].map((l) => l.textContent).join(" "));
    if (labels) return labels;
    if (tag === "input" && ["button", "submit", "reset"].includes(el.getAttribute("type"))) return flat(el.value);
    return flat(el.getAttribute("placeholder") || el.getAttribute("title"));
  }
  if (tag === "img") return flat(el.getAttribute("alt") || el.getAttribute("title"));
  const title = flat(el.getAttribute("title"));
  if (!byText) return title;
  return flat(el.textContent) || title || flat(el.querySelector?.("img[alt]")?.getAttribute("alt"));
}

function valueOf(el) {
  if (el.localName === "select") return flat(el.options?.[el.selectedIndex]?.textContent);
  if ((el.getAttribute("type") ?? "").toLowerCase() === "password") return el.value ? "•".repeat(Math.min(el.value.length, 8)) : "";
  return "value" in el && typeof el.value === "string" ? el.value : flat(el.textContent);
}

/** One line of the tree: role, name, ref, and what an agent needs to know of its state. */
function line(el, role, name, ref) {
  let out = `- ${role}${name ? ` ${quote(cut(name))}` : ""}`;
  if (role === "heading") {
    const level = el.getAttribute("aria-level");
    out += ` [level=${/^\d{1,2}$/.test(level ?? "") ? level : /^h[1-6]$/.test(el.localName) ? el.localName[1] : "?"}]`;
  }
  if (ref) out += ` [ref=${ref}]`;
  if (el.checked === true || el.getAttribute("aria-checked") === "true") out += " [checked]";
  if (el.disabled === true || el.getAttribute("aria-disabled") === "true") out += " [disabled]";
  const expanded = el.getAttribute("aria-expanded");
  if (expanded) out += expanded === "true" ? " [expanded]" : " [collapsed]";
  if (el.getAttribute("aria-selected") === "true") out += " [selected]";
  if (role === "textbox" || role === "searchbox" || role === "combobox" || role === "spinbutton" || role === "slider") out += ` value=${quote(cut(valueOf(el)))}`;
  if (role === "link") out += ` -> ${cut(flat(el.getAttribute("href")), 120)}`;
  return out;
}

/**
 * The page under `scope` (a ref or CSS; the body if none) as lines of the tree: each element
 * with a role, refs on what acts, `depth` levels at most. Past `most` lines or `chars` characters
 * nothing more is looked at but counted: a huge page costs little more than a big one.
 */
function walk({ interactive = false, scope = null, depth = null, boxes = false, most = LINES, chars = CHARS }) {
  const root = scope ? resolve(scope) : (document.body ?? document.documentElement);
  const lines = [];
  let [full, size, omitted] = [false, 0, 0];
  const emit = (level, text) => {
    const at = `${"  ".repeat(interactive ? 0 : level)}${text}`;
    if (lines.length < most && size + at.length < chars) {
      lines.push(at);
      size += at.length + 1;
    } else full = true;
  };
  // An element and all its own, counted by the engine rather than walked a node at a time.
  const count = (node) => (node.nodeType === 1 ? node.getElementsByTagName("*").length + 1 : 0);
  // A same-origin frame's page goes on in the tree; another origin's can't be read from here.
  const frame = (el, level, muted) => {
    const name = nameOf(el, false);
    const inner = frameDoc(el);
    if (!inner) {
      let origin = "of unknown origin";
      try {
        origin = new URL(el.getAttribute("src") || "about:blank", docOf(el).baseURI).origin;
      } catch {}
      return emit(level, `- iframe${name ? ` ${quote(cut(name))}` : ""} (cross-origin frame ${origin}: not readable from the page)`);
    }
    if (!interactive) emit(level, line(el, "iframe", name, null));
    const body = inner.body ?? inner.documentElement;
    if (body) visit(body, interactive ? level : level + 1, false, muted);
  };
  // `muted`: under a title, whose text its name says already.
  const visit = (node, level, pointer, muted) => {
    if (full) return void (omitted += count(node));
    if (depth !== null && level > depth) return;
    if (node.nodeType === 3) {
      const text = interactive || muted ? "" : flat(node.nodeValue);
      if (text) emit(level, `- text ${quote(cut(text, 200))}`);
      return;
    }
    if (node.nodeType !== 1 || SKIP.has(node.localName)) return;
    const el = node;
    if (el.hidden || el.getAttribute("aria-hidden") === "true") return;
    const style = styleOf(el);
    if (style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse") return;
    if (el.localName === "iframe") return frame(el, level, muted);
    // A div that takes clicks (React's onClick) shows only as a pointer where its parent has none.
    const role = roleOf(el) ?? ((style.cursor === "pointer" && !pointer) || el.hasAttribute("onclick") ? "clickable" : null);
    let next = level;
    const leaf = LEAF.has(role) || (role === "clickable" && !el.querySelector(ACTIONABLE));
    if (role) {
      const acts = INTERACTIVE.has(role);
      const name = nameOf(el, leaf || TITLES.has(role));
      if (!interactive || acts) {
        const ref = acts ? refOf(el) : null;
        let text = line(el, role, name, ref);
        if (boxes && ref) text += ` [box=${boxOf(el).join(",")}]`;
        emit(level, text);
        next = level + 1;
      }
      if (leaf) return;
    }
    const children = el.shadowRoot ? [...el.shadowRoot.childNodes, ...el.childNodes] : [...el.childNodes];
    for (const child of children) visit(child, next, pointer || style.cursor === "pointer", muted || TITLES.has(role));
  };
  visit(root, 0, false, false);
  return { lines, omitted: full ? Math.max(omitted, 1) : 0 };
}

/** Lines and characters a snapshot gives at most (an agent's tool cuts its output near 30 000), and one written to a file. */
const CHARS = 25000;

const FILE_LINES = 20000;

const FILE_CHARS = 2000000;

function snapshot({ file = false, ...ask }) {
  const { lines, omitted } = walk(file ? { ...ask, most: FILE_LINES, chars: FILE_CHARS } : ask);
  if (omitted) lines.push(`- … omitted ~${omitted} elements: narrow it with --ref, -s, -d or -i`);
  return { out: lines.length ? lines.join("\n") : "(nothing on the page)", removed: prune() };
}

/** An element's box in the viewport, CSS px: x, y, width, height. */
function boxOf(el) {
  const r = rectOf(el);
  const at = onTop(el, { x: r.x, y: r.y });
  return [at.x, at.y, r.width, r.height].map(Math.round);
}

/** Matches a find shows at most, each with the line before and after it. */
const MATCHES = 30;

/** The tree's lines that have `pattern` (any case), or match it as /regex/flags. */
function find({ pattern }) {
  const re = /^\/(.+)\/([a-z]*)$/.exec(pattern);
  let test;
  if (re) {
    let r;
    try {
      r = new RegExp(re[1], re[2].replace("g", ""));
    } catch {
      throw new Error(`Not a regular expression: ${pattern}`);
    }
    test = (s) => r.test(s);
  } else {
    const low = pattern.toLowerCase();
    test = (s) => s.toLowerCase().includes(low);
  }
  const { lines } = walk({ most: FILE_LINES, chars: FILE_CHARS });
  const hits = lines.flatMap((l, i) => (test(l.trimStart()) ? [i] : []));
  if (!hits.length) throw new Error(`Nothing on the page matches ${pattern}`);
  const kept = [...new Set(hits.slice(0, MATCHES).flatMap((i) => [i - 1, i, i + 1]))].filter((i) => i >= 0 && i < lines.length);
  const out = [];
  kept.forEach((i, k) => {
    if (k && i > kept[k - 1] + 1) out.push("--");
    out.push(lines[i]);
  });
  if (hits.length > MATCHES) out.push(`… ${hits.length - MATCHES} more matches: narrow the pattern`);
  return { out: out.join("\n"), count: hits.length };
}

/** Characters `get` gives at most. */
const GET_TEXT = 4000;

const GET_HTML = 8000;

/** What `get styles` shows unless told. */
const STYLES = ["display", "visibility", "opacity", "position", "z-index", "pointer-events", "color", "background-color", "font-size"];

const capped = (s, n) => (s.length > n ? `${cut(s, n + 1)}\n… ${s.length - n} more characters: get a smaller element` : s);

const masked = (el, value) => (typeOf(el) === "password" && value ? "•".repeat(Math.min(value.length, 8)) : value);

/** One thing about an element: its text, HTML, value, an attribute, computed styles or box. */
function get({ what, target, names = [] }) {
  const el = resolve(target);
  switch (what) {
    case "text":
      return { out: capped(el.innerText ?? el.textContent ?? "", GET_TEXT) };
    case "html":
      return { out: capped(el.outerHTML ?? "", GET_HTML) };
    case "value": {
      if (el.localName === "select") return { out: [...el.options].filter((o) => o.selected).map((o) => flat(o.textContent)).join("\n") };
      if (typeOf(el) === "checkbox" || typeOf(el) === "radio") return { out: el.checked ? "checked" : "unchecked" };
      if (!fieldOf(el)) return { out: capped(el.innerText ?? el.textContent ?? "", GET_TEXT) };
      return { out: capped(masked(el, el.value), GET_TEXT) };
    }
    case "attr": {
      const value = el.getAttribute(names[0]);
      if (value === null) throw new Error(`${label(el)} has no ${names[0]} attribute`);
      return { out: capped(names[0] === "value" ? masked(el, value) : value, GET_TEXT) };
    }
    case "styles": {
      const style = styleOf(el);
      return { out: (names.length ? names : STYLES).map((p) => `${p}: ${style.getPropertyValue(p)}`).join("\n") };
    }
    case "box": {
      const [x, y, w, h] = boxOf(el);
      const inside = x + w > 0 && y + h > 0 && x < window.innerWidth && y < window.innerHeight;
      return { out: `${x},${y},${w},${h}${inside ? "" : " (outside the viewport: scroll to it first)"}`, box: { x, y, w, h } };
    }
    default:
      throw new Error(`No get ${what}`);
  }
}

/** The element's ref: the one it had, else the next. */
function refOf(el) {
  let ref = state.ids.get(el);
  if (!ref) {
    ref = `e${state.next++}`;
    state.ids.set(el, ref);
  }
  state.refs.set(ref, new WeakRef(el));
  return ref;
}

/** Refs whose element left the page, dropped: what `snapshot --json` reports removed. */
function prune() {
  const gone = [];
  for (const [ref, weak] of state.refs) {
    if (live(weak.deref())) continue;
    state.refs.delete(ref);
    gone.push(ref);
  }
  return gone;
}

const shows = (el) => el.getClientRects().length > 0 && styleOf(el).visibility !== "hidden";

/** For wait: whether an element matching `css` shows, and how many match. */
function exists({ css }) {
  let all;
  try {
    all = [...document.querySelectorAll(css)];
  } catch {
    return { found: false, invalid: true };
  }
  return { found: all.some(shows), count: all.length };
}

const hasText = ({ text }) => ({ found: (document.body?.innerText ?? "").includes(text) });

/** Labels a screenshot draws at most. */
const LABELS = 300;

/**
 * For a screenshot: the part of the viewport it takes, in CSS px (an element's box, brought into
 * view; `clip`; else all of it), kept to what shows; with `refs`, where each interactive element
 * shows in it, every one given its ref first.
 */
function view({ target = null, clip = null, refs = false }) {
  let r = clip ?? { x: 0, y: 0, w: window.innerWidth, h: window.innerHeight };
  const el = target && resolve(target);
  if (el) {
    into(el);
    const b = rectOf(el);
    const at = onTop(el, b);
    r = { x: at.x, y: at.y, w: b.width, h: b.height };
  }
  const x0 = Math.max(r.x, 0), y0 = Math.max(r.y, 0);
  const x1 = Math.min(r.x + r.w, window.innerWidth), y1 = Math.min(r.y + r.h, window.innerHeight);
  if (x1 <= x0 || y1 <= y0) throw new Error(el ? `${label(el)} doesn't show in the viewport` : `The clip is outside the ${window.innerWidth}×${window.innerHeight} viewport`);
  const rect = { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
  const labels = [];
  if (refs) {
    walk({ interactive: true, most: FILE_LINES, chars: FILE_CHARS });
    for (const [ref, weak] of state.refs) {
      const it = weak.deref();
      if (!live(it) || !it.getClientRects().length || labels.length === LABELS) continue;
      const b = rectOf(it);
      const at = onTop(it, b);
      if (at.x + b.width <= x0 || at.y + b.height <= y0 || at.x >= x1 || at.y >= y1) continue;
      labels.push({ ref, x: at.x, y: at.y, w: b.width, h: b.height });
    }
  }
  return { rect, labels };
}

/**
 * A full-page screenshot's steps. `begin`: the page at its top, its height and the viewport,
 * with the fixed and sticky elements noted. `to`: the page scrolled to `y`, those elements
 * shown only where a person would see them (one near the top in the first tile, one near the
 * bottom in the last, a sticky one in the first), so a header isn't drawn on every tile.
 * `end`: all put back, the page scrolled to where it was.
 */
async function tile({ step, y = 0, first = false, last = false }) {
  const t = state.tile;
  if (step === "begin") {
    state.tile = { was: [window.scrollX, window.scrollY], pinned: [] };
    window.scrollTo({ left: window.scrollX, top: 0, behavior: "instant" });
    await frame();
    for (const el of document.querySelectorAll("body, body *")) {
      const position = styleOf(el).position;
      if (position !== "fixed" && position !== "sticky") continue;
      const r = el.getBoundingClientRect();
      if (!r.width || !r.height) continue;
      state.tile.pinned.push({ el, low: position === "fixed" && r.y + r.height / 2 > window.innerHeight / 2, set: [el.style.getPropertyValue("visibility"), el.style.getPropertyPriority("visibility")] });
    }
    return { height: document.documentElement.scrollHeight, w: window.innerWidth, h: window.innerHeight };
  }
  if (!t) throw new Error("No full-page screenshot is under way");
  const show = (p, on) => p.el.style.setProperty("visibility", on ? p.set[0] : "hidden", on ? p.set[1] : "important");
  if (step === "to") {
    window.scrollTo({ left: window.scrollX, top: y, behavior: "instant" });
    for (const p of t.pinned) show(p, (first && last) || (p.low ? last : first));
    await frame();
    await new Promise((done) => setTimeout(done, 100));
    return { y: window.scrollY };
  }
  for (const p of t.pinned) show(p, true);
  window.scrollTo({ left: t.was[0], top: t.was[1], behavior: "instant" });
  state.tile = null;
  return {};
}
