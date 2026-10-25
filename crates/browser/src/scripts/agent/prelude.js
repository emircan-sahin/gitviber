// What `gitviber browser` does in a page (browser/agent.rs): the page as a tree an agent can act
// on, and acting on what it named. The body of an async function run in GitViber's own content
// world with `input` as JSON: the page can't see this code or its refs, and the refs (WeakRefs
// in this world's window) go with the page. agent.rs joins this script's parts in order:
// prelude.js (what all the rest use), read.js (the tree, find, get, the view), act.js (where an
// action acts, and each action), watch.js (what an action changed), run.js (the command run).

const { cmd, floor = 0, ...a } = JSON.parse(input);

// An element keeps its ref across snapshots. `floor` is the highest ref the tab ever handed out
// (agent.rs keeps it): a new document, or one back from the back-forward cache, numbers past it,
// so an old page's e5 can never name an element of this one.
const state = (window.__gvAgent ??= { refs: new Map(), ids: new WeakMap(), next: floor + 1 });

state.next = Math.max(state.next, floor + 1);

const NAME = 80;

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

const REF = /^(?:@|ref=)?e(\d+)$/;

/** No frame matches yet: a wait goes on, where any other fault in a frame's name ends it. */
class NoFrame extends Error {}

/**
 * The document a target looks in, and the CSS in it: `frame=<ref|css> >> <css>` is <css> in that
 * same-origin frame's own document (frames nest: `frame=a >> frame=b >> <css>`).
 */
function scopeOf(target, doc = document) {
  const framed = splitFrame(target);
  return framed ? scopeOf(framed[1], inFrame(framed[0], doc)) : { doc, css: target };
}

/** `frame=<name> >> <rest>` as [name, rest], cut at the first ` >> ` outside quotes and brackets. */
function splitFrame(target) {
  const start = /^\s*frame=/.exec(target)?.[0].length;
  if (start === undefined) return null;
  let quote = "";
  let depth = 0;
  for (let i = start; i < target.length; i++) {
    const c = target[i];
    if (quote) {
      if (c === "\\") i++;
      else if (c === quote) quote = "";
    } else if (c === '"' || c === "'") quote = c;
    else if (c === "[" || c === "(") depth++;
    else if (c === "]" || c === ")") depth--;
    else if (depth <= 0 && /\s/.test(c) && target.startsWith(">> ", i + 1)) {
      const name = target.slice(start, i).trim();
      const rest = target.slice(i + 4).trim();
      if (name && rest) return [name, rest];
    }
  }
  return null;
}

/** The page of the same-origin frame `name` (a ref, or CSS in `doc`) is. */
function inFrame(name, doc) {
  let frame;
  if (REF.test(name)) frame = resolve(name);
  else {
    try {
      frame = doc.querySelector(name);
    } catch {
      throw new Error(`Not a ref or a CSS selector: ${name}`);
    }
  }
  if (!frame) throw new NoFrame(`No frame matches ${name}`);
  if (!/^i?frame$/.test(frame.localName)) throw new Error(`${name} isn't a frame`);
  const inner = frameDoc(frame);
  if (!inner) throw new Error(`${name} is another origin's frame: its page can't be read`);
  return inner;
}

/** The element a ref (e3, @e3, ref=e3) names, or a CSS selector does (see scopeOf for frames). */
function resolve(target) {
  const ref = REF.exec(target);
  if (ref) {
    const name = `e${ref[1]}`;
    const el = state.refs.get(name)?.deref();
    if (live(el)) return el;
    // Past the highest ever given: a ref made up, not one gone stale.
    if (Number(ref[1]) >= state.next) throw new Error(`No ref ${name} on this page: take a new snapshot`);
    throw new Error(`${name} is stale: take a new snapshot`);
  }
  const { doc, css } = scopeOf(target);
  let el;
  try {
    el = doc.querySelector(css);
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

const typeOf = (el) => (el.type || el.getAttribute("type") || "text").toLowerCase();
