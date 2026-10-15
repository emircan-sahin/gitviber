// What `gitviber browser` does in a page (browser/agent.rs): the page as a tree an agent can act
// on, and acting on what it named. The body of an async function run in GitViber's own content
// world with `input` as JSON: the page can't see this code or its refs, and the refs (WeakRefs
// in this world's window) go with the page.
const { cmd, floor = 0, ...a } = JSON.parse(input);
// An element keeps its ref across snapshots. `floor` is the highest ref the tab ever handed out
// (agent.rs keeps it): a new document, or one back from the back-forward cache, numbers past it,
// so an old page's e5 can never name an element of this one.
const state = (window.__gvAgent ??= { refs: new Map(), ids: new WeakMap(), next: floor + 1 });
state.next = Math.max(state.next, floor + 1);

/** Lines a snapshot gives at most: past them, -s, -d or -i narrow it. */
const LINES = 1500;
const NAME = 80;
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

const flat = (s) => String(s ?? "").toWellFormed().replace(/\s+/g, " ").trim();
// Never through a pair: half of one is no JSON the app reads.
const cut = (s, n = NAME) => {
  if (s.length <= n) return s;
  const end = /[\uD800-\uDBFF]/.test(s[n - 2]) ? n - 2 : n - 1;
  return `${s.slice(0, end)}…`;
};
const quote = (s) => JSON.stringify(s);

// A same-origin frame's page is read and acted in as part of this one: each element in its own
// document, with that document's window, viewport and coordinates.
const docOf = (el) => el.ownerDocument ?? document;
const viewOf = (el) => docOf(el).defaultView ?? window;
function styleOf(el) {
  const view = viewOf(el);
  return view !== window && view.getComputedStyle ? view.getComputedStyle(el) : getComputedStyle(el);
}

/** A frame's document, or nothing when it's another origin's (or no frame). */
function frameDoc(el) {
  try {
    return el.contentDocument ?? null;
  } catch {
    return null;
  }
}

/** The <iframe>s an element is inside, innermost first. */
function framesOf(el) {
  const out = [];
  for (let view = viewOf(el); view !== window && view.frameElement; view = viewOf(view.frameElement)) out.push(view.frameElement);
  return out;
}

/** Point `at` of a frame's viewport in the viewport of the window above it. */
function above(frame, at) {
  const r = frame.getBoundingClientRect();
  const s = styleOf(frame);
  return { x: at.x + r.x + (frame.clientLeft ?? 0) + (parseFloat(s.paddingLeft) || 0), y: at.y + r.y + (frame.clientTop ?? 0) + (parseFloat(s.paddingTop) || 0) };
}

/** Point `at` of `el`'s viewport in the top one, where native input and screenshots are. */
const onTop = (el, at) => framesOf(el).reduce((p, frame) => above(frame, p), at);

/** On a page still shown: an element of a frame that navigated, or was taken out, is stale. */
const live = (el) => !!el?.isConnected && (docOf(el) === document || !!docOf(el).defaultView);

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
  const count = (node) => (node.nodeType === 1 ? 1 + [...node.childNodes].reduce((n, c) => n + count(c), 0) : 0);
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
function search({ pattern }) {
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

/** The element a ref (e3, @e3, ref=e3) names, or a CSS selector does. */
function resolve(target) {
  const ref = /^(?:@|ref=)?e(\d+)$/.exec(target);
  if (ref) {
    const name = `e${ref[1]}`;
    const el = state.refs.get(name)?.deref();
    if (live(el)) return el;
    // Past the highest ever given: a ref made up, not one gone stale.
    if (Number(ref[1]) >= state.next) throw new Error(`No ref ${name} on this page: take a new snapshot`);
    throw new Error(`${name} is stale: take a new snapshot`);
  }
  let el;
  try {
    el = document.querySelector(target);
  } catch {
    throw new Error(`Not a ref or a CSS selector: ${target}`);
  }
  if (!el) throw new Error(`No element matches ${target}`);
  return el;
}

const label = (el) => {
  const name = nameOf(el, true);
  return `${roleOf(el) ?? el.localName}${name ? ` ${quote(cut(name, 40))}` : ""}`;
};

/** How long an element gets to show, stop moving or come out from under another one. */
const PATIENCE = 1000;
// A page that draws no frames (out of sight, throttled) still moves on.
const frame = () =>
  new Promise((done) => {
    const late = setTimeout(done, 50);
    window.requestAnimationFrame?.(() => {
      clearTimeout(late);
      done();
    });
  });

/** An element's box in the viewport; a display: contents one has none, so its children's together. */
function rectOf(el) {
  if (styleOf(el).display !== "contents") return el.getBoundingClientRect();
  const boxes = [...(el.children ?? [])].map(rectOf).filter((r) => r.width && r.height);
  if (!boxes.length) return { x: 0, y: 0, width: 0, height: 0 };
  const x = Math.min(...boxes.map((r) => r.x)), y = Math.min(...boxes.map((r) => r.y));
  const right = Math.max(...boxes.map((r) => r.x + r.width)), bottom = Math.max(...boxes.map((r) => r.y + r.height));
  return { x, y, width: right - x, height: bottom - y };
}

/** Brought into the viewport once, at once, unless it's all there: a smooth scroll asked again each try never lands. */
function into(el) {
  const shown = (r, view) => r.x >= 0 && r.y >= 0 && r.x + r.width <= view.innerWidth && r.y + r.height <= view.innerHeight;
  const r = rectOf(el);
  if (!shown(r, viewOf(el))) {
    const target = styleOf(el).display === "contents" ? el.children?.[0] : el;
    target?.scrollIntoView?.({ block: "center", inline: "center", behavior: "instant" });
  }
  // Then each window above, as far as the element's box needs and no further.
  const box = rectOf(el);
  let at = { x: box.x, y: box.y };
  for (const frame of framesOf(el)) {
    const view = viewOf(frame);
    let p = above(frame, at);
    if (!shown({ x: p.x, y: p.y, width: box.width, height: box.height }, view)) {
      const left = p.x + box.width / 2 - view.innerWidth / 2, top = p.y + box.height / 2 - view.innerHeight / 2;
      view.scrollBy?.({ left, top, behavior: "instant" });
      p = above(frame, at);
    }
    at = p;
  }
}

/** How an element reads in an error: <div#consent.banner>. */
function tagOf(el) {
  const id = el.id ? `#${el.id}` : "";
  const classes = typeof el.className === "string" ? el.className.trim().split(/\s+/).filter(Boolean).slice(0, 2) : [];
  return `<${el.localName}${cut(flat(id + classes.map((c) => `.${c}`).join("")), 40)}>`;
}

const disabled = (el) => el.matches?.(":disabled") || el.disabled === true || el.getAttribute("aria-disabled") === "true";

/** Why an element can't be acted on now, or nothing: on the page, drawn, and enabled. */
function unready(el, enabled) {
  if (!live(el)) return "left the page: take a new snapshot";
  const style = styleOf(el);
  if (style.visibility === "hidden" || style.visibility === "collapse") return "isn't visible (visibility: hidden)";
  const contents = style.display === "contents";
  if (!contents && !el.getClientRects().length) return "isn't visible (display: none, here or above)";
  const r = rectOf(el);
  if (!r.width || !r.height) return contents ? "isn't visible (display: contents, and nothing inside it shows)" : "isn't visible (no size)";
  if (enabled && disabled(el)) return "is disabled";
  return null;
}

/** Whether a pointer at the top element `hit` reaches `el`: it's `el`, inside it, or its label. */
function reaches(el, hit) {
  for (let n = hit; n; n = n.parentNode ?? n.host) {
    if (n === el) return true;
    if (n.localName === "label" && [...(el.labels ?? [])].includes(n)) return true;
  }
  return false;
}

/** The top element at a point, into shadow roots as far as they go. */
function top(doc, x, y) {
  let hit = doc.elementFromPoint(x, y);
  while (hit?.shadowRoot) {
    const inner = hit.shadowRoot.elementFromPoint(x, y);
    if (!inner || inner === hit) break;
    hit = inner;
  }
  return hit;
}

/** Why the top element at `at` (in `el`'s viewport) isn't `el`, or nothing. */
function coveredIn(el, at) {
  const over = top(docOf(el), at.x, at.y);
  if (over && reaches(el, over)) return null;
  if (!over) return "is outside the viewport";
  if (over.contains?.(el)) return "doesn't take the pointer (pointer-events: none?)";
  // The cover itself, not a line of text in it.
  let cover = over;
  while (cover.parentElement && !cover.parentElement.contains(el)) cover = cover.parentElement;
  return `is covered by ${tagOf(cover)}`;
}

/** As coveredIn, and each frame `el` is in on top at that point in the window above it. */
function covered(el, at) {
  let why = coveredIn(el, at);
  for (let [p, frames] = [at, framesOf(el)]; !why && frames.length; frames = frames.slice(1)) {
    p = above(frames[0], p);
    why = coveredIn(frames[0], p);
  }
  return why;
}

/**
 * Where a pointer acts on `el`, in the viewport's CSS px, once a person could: on the page,
 * visible, enabled (unless `enabled` is false), still from one frame to the next, and on top at
 * its middle (unless `hit` is false). Else why not, once PATIENCE is up. Every action, synthetic
 * or native, takes its point from here.
 */
async function point(el, { enabled = true, hit = true } = {}) {
  const until = performance.now() + PATIENCE;
  let why;
  for (let scrolled = false; ; await frame()) {
    why = unready(el, enabled);
    if (!why) {
      if (!scrolled) into(el);
      scrolled = true;
      const was = rectOf(el);
      await frame();
      const r = rectOf(el);
      if (r.x !== was.x || r.y !== was.y || r.width !== was.width || r.height !== was.height) why = "is still moving";
      else {
        // The middle of what shows of it: a box taller than the viewport is clicked where it shows.
        const view = viewOf(el);
        const left = Math.max(r.x, 0), right = Math.min(r.x + r.width, view.innerWidth);
        const high = Math.max(r.y, 0), low = Math.min(r.y + r.height, view.innerHeight);
        const at = { x: (left + right) / 2, y: (high + low) / 2 };
        if (right <= left || low <= high) why = "is outside the viewport";
        else if (!hit || !(why = covered(el, at))) return at;
      }
    }
    if (performance.now() >= until) throw new Error(`${label(el)} ${why}`);
  }
}

function mouse(el, types, at) {
  for (const type of types) {
    const init = { bubbles: true, cancelable: true, composed: true, view: viewOf(el), button: 0, buttons: type.endsWith("down") ? 1 : 0, ...at };
    el.dispatchEvent(type.startsWith("pointer") ? new PointerEvent(type, { ...init, pointerId: 1, pointerType: "mouse", isPrimary: true }) : new MouseEvent(type, init));
  }
}

const fieldOf = (el) => el.localName === "input" || el.localName === "textarea";

/** A field's value as typing leaves it: through the prototype's setter, which frameworks watch. */
function setValue(el, value) {
  const proto = el.localName === "textarea" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
  el.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "insertText", data: value }));
}

/** Text in at the caret as the editor would put it (input events and all), else set. */
function insert(el, text) {
  if (docOf(el).execCommand("insertText", false, text)) return;
  if (fieldOf(el)) setValue(el, el.value + text);
}

/** A click at `at` as the mouse gives one: its events in turn, focus between, then the click. */
function clickAt(el, at) {
  const where = { clientX: at.x, clientY: at.y };
  mouse(el, ["pointerover", "mouseover", "pointermove", "mousemove", "pointerdown", "mousedown"], where);
  el.focus?.({ preventScroll: true });
  mouse(el, ["pointerup", "mouseup"], where);
  // An SVG element has no click() of its own.
  if (typeof el.click === "function") el.click();
  else mouse(el, ["click"], where);
}

async function click({ target }) {
  const el = resolve(target);
  clickAt(el, await point(el));
  return { out: `Clicked ${label(el)}` };
}

const TEXT_TYPES = new Set(["text", "search", "email", "url", "tel", "password", "number"]);
// What the browser takes for each: anything else it quietly drops for "", which would read as filled.
const FORMATS = {
  date: [/^\d{4}-\d{2}-\d{2}$/, "YYYY-MM-DD"],
  time: [/^\d{2}:\d{2}(:\d{2}(\.\d{1,3})?)?$/, "HH:MM or HH:MM:SS"],
  "datetime-local": [/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d{1,3})?)?$/, "YYYY-MM-DDTHH:MM"],
  month: [/^\d{4}-\d{2}$/, "YYYY-MM"],
  week: [/^\d{4}-W\d{2}$/, "YYYY-Www"],
  color: [/^#[\da-f]{6}$/i, "#rrggbb"],
  range: [/^-?\d+(\.\d+)?$/, "a number"],
};

const typeOf = (el) => (el.type || el.getAttribute("type") || "text").toLowerCase();
const editable = (el) => el.isContentEditable ?? ["", "true", "plaintext-only"].includes(el.getAttribute("contenteditable"));

/** The field an element stands for: a label's control, or the one field inside a wrapper. */
function control(el) {
  if (el.localName === "label" && el.control) return el.control;
  if (["input", "textarea", "select"].includes(el.localName) || editable(el)) return el;
  const fields = "input:not([type=hidden]), textarea, select, [contenteditable]:not([contenteditable=false])";
  // A web component's field is in its shadow root.
  const inner = [...(el.querySelectorAll?.(fields) ?? []), ...(el.shadowRoot?.querySelectorAll(fields) ?? [])];
  return inner.length === 1 ? inner[0] : el;
}

/** What fill or type can write into `el`: "text", "format" (a date's, a color's, ...) or "rich" (contenteditable); else why not. */
function writable(el) {
  let kind;
  if (el.localName === "textarea") kind = "text";
  else if (el.localName === "input") {
    const type = typeOf(el);
    if (type === "checkbox" || type === "radio") throw new Error(`${label(el)} is a ${type}: use check or uncheck`);
    if (type === "file") throw new Error(`${label(el)} is a file input: fill can't choose files`);
    if (FORMATS[type]) kind = "format";
    else if (TEXT_TYPES.has(type)) kind = "text";
    else throw new Error(`${label(el)} is an <input type=${type}>: it takes no text`);
  } else if (el.localName === "select") throw new Error(`${label(el)} is a <select>: use select`);
  else if (editable(el)) kind = "rich";
  else throw new Error(`${label(el)} isn't a text field: fill takes an input, a textarea or a contenteditable element`);
  if (el.readOnly === true || el.getAttribute("aria-readonly") === "true") throw new Error(`${label(el)} is read-only`);
  return kind;
}

/** What a field holds now, as fill compares it. */
const holds = (el) => (el.localName === "input" || el.localName === "textarea" ? el.value : flat(el.innerText ?? el.textContent));

/** Whether a field holds `text`, as its type writes it: a color in lower case, a time without its :00 seconds. */
function holdsText(el, kind, now, text) {
  if (kind === "rich") return now === flat(text);
  const normal = (v) => (kind === "format" ? v.toLowerCase().replace(/:00(\.0+)?$/, "") : v);
  return normal(now) === normal(text);
}

async function fill({ target, text, secret = false }) {
  const el = control(resolve(target));
  const kind = writable(el);
  const what = secret ? "the secret" : quote(cut(text, 120));
  if (kind === "format" && text) {
    const [shape, hint] = FORMATS[typeOf(el)];
    if (!shape.test(text)) throw new Error(`Malformed value for ${label(el)} (a ${typeOf(el)} input): ${what} isn't ${hint}`);
  }
  await point(el, { hit: false });
  const before = holds(el);
  el.focus?.({ preventScroll: true });
  if (kind === "format") setValue(el, text);
  else {
    if (fieldOf(el)) el.select?.();
    else viewOf(el).getSelection?.()?.selectAllChildren(el);
    // Typed as the editor types: it holds to a maxlength, and the page's handlers see it. Set
    // only where that does nothing.
    if (!docOf(el).execCommand?.(text ? "insertText" : "delete", false, text)) {
      if (fieldOf(el)) setValue(el, text);
      else el.textContent = text;
    }
  }
  el.dispatchEvent(new Event("change", { bubbles: true }));
  const now = holds(el);
  if (holdsText(el, kind, now, text)) return { out: `Filled ${label(el)}` };
  const it = secret ? "something else" : quote(cut(now, 120));
  if (el.maxLength > 0 && text.length > el.maxLength) throw new Error(`${label(el)} didn't take ${what}: it holds ${it} (maxlength ${el.maxLength})`);
  if ((!now && text) || now === before) throw new Error(`${label(el)} didn't take ${what}: it holds ${it}`);
  // Anything else is the page's own format of it (a phone mask, thousands separators).
  return { out: `Filled ${label(el)}; the page shows ${secret ? "it in its own format" : it}` };
}

const KEYS = { enter: "Enter", return: "Enter", tab: "Tab", esc: "Escape", escape: "Escape", space: " ", backspace: "Backspace", delete: "Delete", up: "ArrowUp", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight", arrowup: "ArrowUp", arrowdown: "ArrowDown", arrowleft: "ArrowLeft", arrowright: "ArrowRight", home: "Home", end: "End", pageup: "PageUp", pagedown: "PageDown" };
const MODS = { meta: "metaKey", cmd: "metaKey", command: "metaKey", control: "ctrlKey", ctrl: "ctrlKey", alt: "altKey", option: "altKey", shift: "shiftKey" };

function keyEvent(el, type, key, mods = {}) {
  const code = key.length === 1 ? (/[a-z]/i.test(key) ? `Key${key.toUpperCase()}` : /\d/.test(key) ? `Digit${key}` : key === " " ? "Space" : "") : key;
  return el.dispatchEvent(new KeyboardEvent(type, { key, code, bubbles: true, cancelable: true, composed: true, ...mods }));
}

async function type({ target, text }) {
  const el = control(resolve(target));
  writable(el);
  await point(el, { hit: false });
  el.focus?.({ preventScroll: true });
  // An email or number field has no selection to set, and says so by throwing.
  try {
    if (fieldOf(el)) el.setSelectionRange?.(el.value.length, el.value.length);
  } catch {}
  for (const ch of text) {
    const go = keyEvent(el, "keydown", ch) && keyEvent(el, "keypress", ch);
    if (go) insert(el, ch);
    keyEvent(el, "keyup", ch);
  }
  el.dispatchEvent(new Event("change", { bubbles: true }));
  return { out: `Typed into ${label(el)}` };
}

const tabbable = () =>
  [...document.querySelectorAll("a[href], button, input, select, textarea, [tabindex], [contenteditable]")].filter(
    (e) => !e.disabled && e.tabIndex >= 0 && e.getClientRects().length > 0,
  );

function press({ key: chord }) {
  const parts = chord.split("+");
  const raw = parts.pop() || "+";
  const key = KEYS[raw.toLowerCase()] ?? raw;
  const mods = {};
  for (const m of parts) {
    const flag = MODS[m.toLowerCase()];
    if (!flag) throw new Error(`Not a modifier: ${m} (Meta, Control, Alt, Shift)`);
    mods[flag] = true;
  }
  let el = document.activeElement ?? document.body;
  // Focus inside a same-origin frame: the key goes to what has it there.
  for (let inner = frameDoc(el); inner?.activeElement; inner = frameDoc(el)) el = inner.activeElement;
  if (keyEvent(el, "keydown", key, mods)) {
    const command = mods.metaKey || mods.ctrlKey;
    if (key === "Enter" && !command) {
      if (el.localName === "input" && el.form) el.form.requestSubmit();
      else if (el.localName === "textarea" || el.isContentEditable) insert(el, "\n");
      else if (el.localName === "button" || el.localName === "a" || el.getAttribute("role") === "button") el.click();
    } else if (key === "Tab") {
      const all = tabbable();
      const next = all[(all.indexOf(el) + (mods.shiftKey ? -1 : 1) + all.length) % all.length];
      next?.focus();
    } else if (key === "Backspace" || key === "Delete") {
      docOf(el).execCommand(key === "Backspace" ? "delete" : "forwardDelete");
    } else if (command && key.toLowerCase() === "a") {
      if (fieldOf(el)) el.select();
      else docOf(el).execCommand("selectAll");
    } else if (key === " " && (el.localName === "button" || el.type === "checkbox" || el.type === "radio")) {
      el.click();
    } else if (key.length === 1 && !command && keyEvent(el, "keypress", key, mods)) {
      insert(el, key);
    }
  }
  keyEvent(el, "keyup", key, mods);
  return { out: `Pressed ${chord} on ${label(el)}` };
}

async function select({ target, values }) {
  const el = control(resolve(target));
  if (el.localName !== "select") throw new Error(`${label(el)} isn't a <select>`);
  const options = [...el.options];
  const text = (o) => flat(o.textContent);
  const asked = values.map((v) => {
    const o = options.find((o) => o.value === v) ?? options.find((o) => text(o).toLowerCase() === v.toLowerCase());
    if (!o) throw new Error(`No option ${quote(v)} in ${label(el)}: ${cut(options.map((o) => quote(text(o))).join(", "), 300)}`);
    if (o.disabled) throw new Error(`Option ${quote(text(o))} in ${label(el)} is disabled`);
    return o;
  });
  const chosen = [...new Set(asked)];
  if (chosen.length > 1 && !el.multiple) throw new Error(`${label(el)} takes one option`);
  await point(el, { hit: false });
  for (const o of options) o.selected = chosen.includes(o);
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
  // Read back: the page's change handler may have put it back.
  const now = options.filter((o) => o.selected);
  if (now.length !== chosen.length || chosen.some((o) => !o.selected)) {
    throw new Error(`${label(el)} didn't keep the choice: it shows ${now.map((o) => quote(text(o))).join(", ") || "nothing"}`);
  }
  return { out: `Selected ${chosen.map((o) => quote(text(o))).join(", ")} in ${label(el)}` };
}

const TOGGLES = new Set(["checkbox", "radio", "switch", "menuitemcheckbox", "menuitemradio"]);

/** check (`on`) or uncheck: a click when it isn't so already, and the state read back after. */
async function check({ target, on }) {
  const found = resolve(target);
  const el = TOGGLES.has(roleOf(found)) ? found : control(found);
  const native = el.localName === "input" && (typeOf(el) === "checkbox" || typeOf(el) === "radio");
  const role = native ? typeOf(el) : roleOf(el);
  if (!TOGGLES.has(role)) throw new Error(`${label(el)} isn't a checkbox, radio or switch`);
  if (!on && (role === "radio" || role === "menuitemradio")) throw new Error(`${label(el)} is a radio button: check another of its group instead`);
  const checked = () => (native ? el.checked === true : el.getAttribute("aria-checked") === "true");
  const word = on ? "checked" : "unchecked";
  if (checked() === on) return { out: `${label(el)} was ${word} already` };
  // A styled checkbox hides its input and shows its label: the click goes there.
  const shown = !native || !unready(el, false);
  const via = shown ? el : ([...(el.labels ?? [])].find((l) => !unready(l, false)) ?? el);
  if (disabled(el)) throw new Error(`${label(el)} is disabled`);
  clickAt(via, await point(via));
  if (checked() !== on) throw new Error(`Clicked ${label(el)}, but it's still ${on ? "unchecked" : "checked"}`);
  return { out: `${on ? "Checked" : "Unchecked"} ${label(el)}` };
}

async function hover({ target }) {
  const el = resolve(target);
  const at = await point(el, { enabled: false });
  mouse(el, ["pointerover", "pointerenter", "mouseover", "mouseenter", "pointermove", "mousemove"], { clientX: at.x, clientY: at.y });
  return { out: `Hovered ${label(el)}` };
}

function scroll({ to, px }) {
  if (to === "up" || to === "down") {
    const by = px ?? Math.round(window.innerHeight * 0.8);
    window.scrollBy({ top: to === "up" ? -by : by, behavior: "instant" });
  } else {
    into(resolve(to));
  }
  const most = Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
  return { out: `Scrolled to ${Math.round(window.scrollY)} of ${most}` };
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

const DIALOGS = "dialog[open], [role=dialog], [role=alertdialog], [aria-modal=true]";
const dialogs = () => [...document.querySelectorAll(DIALOGS)].filter(shows).length;

/**
 * The page as an agent would find it again, the same after an action that did nothing: cheap
 * enough after any flood of changes, as it never reads the page's text.
 */
function pageState() {
  const focused = document.activeElement;
  const value = typeof focused?.value === "string" ? focused.value.slice(0, 200) : "";
  return [location.href, document.getElementsByTagName("*").length, Math.round(window.scrollY ?? 0), focused?.localName, value, dialogs()].join("|");
}

/** The page's document and each same-origin frame's, however deep. */
function documents(doc = document) {
  const frames = [...(doc.querySelectorAll?.("iframe") ?? [])].map(frameDoc).filter(Boolean);
  return [doc, ...frames.flatMap((inner) => documents(inner))];
}

/** What changes on the page from now, an action's, until `changes` reads it. */
function watch() {
  state.watch?.observer.disconnect();
  const seen = { added: 0, removed: 0, texts: 0, attributes: 0, at: Date.now() };
  const count = (records) => {
    if (records.length) seen.at = Date.now();
    for (const r of records) {
      if (r.type === "attributes") seen.attributes++;
      else if (r.type === "characterData") seen.texts++;
      else {
        for (const n of r.addedNodes) {
          if (n.nodeType === 1) seen.added++;
          else if (n.nodeType === 3) seen.texts++;
        }
        for (const n of r.removedNodes) if (n.nodeType === 1) seen.removed++;
      }
    }
  };
  const observer = new MutationObserver(count);
  for (const doc of documents()) observer.observe(doc, { subtree: true, childList: true, characterData: true, attributes: true });
  state.watch = { observer, seen, count, dialogs: dialogs() };
  return {};
}

/**
 * What changed since `watch`, once the page has been still for `quiet` ms or `most` have gone
 * (a spinner never stops); the watch ends. None on another document: the watch went with the old.
 */
async function changes({ quiet = 100, most = 500 }) {
  const w = state.watch;
  if (!w) return { gone: true, state: pageState() };
  const until = Date.now() + most;
  while (Date.now() < until && Date.now() - w.seen.at < quiet) await new Promise((r) => setTimeout(r, 25));
  w.count(w.observer.takeRecords());
  w.observer.disconnect();
  state.watch = null;
  const { added, removed, texts, attributes } = w.seen;
  return { added, removed, texts, attributes, dialogs: dialogs() - w.dialogs, state: pageState() };
}

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

// A page's lone half of a pair would reach the app as JSON it can't read.
function wellFormed(_key, value) {
  return typeof value === "string" ? value.toWellFormed() : value;
}

/**
 * Where an action acts, for agent.rs: `target`'s point once it's actionable, or the point `x`,
 * `y` itself, in the top viewport even for an element in a frame; and what's there.
 */
async function aim({ target = null, x = null, y = null }) {
  if (target !== null) {
    const el = resolve(target);
    return { ...onTop(el, await point(el)), label: label(el) };
  }
  if (!(x >= 0 && y >= 0 && x < window.innerWidth && y < window.innerHeight)) {
    throw new Error(`${x},${y} is outside the ${window.innerWidth}×${window.innerHeight} viewport`);
  }
  const over = top(document, x, y);
  return { x, y, label: over ? label(over) : "nothing" };
}

const run = { snapshot, find: search, get, aim, click, fill, type, press, select, check, hover, scroll, exists, hasText, view, watch, changes };
try {
  if (!Object.hasOwn(run, cmd)) throw new Error(`No ${cmd} here`);
  return JSON.stringify({ ...(await run[cmd](a)), last: state.next - 1 }, wellFormed);
} catch (e) {
  state.watch?.observer.disconnect();
  state.watch = null;
  return JSON.stringify({ error: String(e?.message ?? e), last: state.next - 1 }, wellFormed);
}
