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
const LEAF = new Set(["link", "button", "textbox", "searchbox", "checkbox", "radio", "combobox", "slider", "spinbutton", "switch", "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "option", "treeitem", "heading", "img", "progressbar", "meter", "iframe"]);
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

function snapshot({ interactive = false, scope = null, depth = null }) {
  const root = scope ? document.querySelector(scope) : (document.body ?? document.documentElement);
  if (!root) throw new Error(`No element matches ${scope}`);
  const lines = [];
  let full = false;
  const emit = (level, text) => {
    if (lines.length < LINES) lines.push(`${"  ".repeat(interactive ? 0 : level)}${text}`);
    else full = true;
  };
  // Past the limit nothing more is looked at: a huge page costs no more than a big one.
  const visit = (node, level, pointer) => {
    if (full || (depth !== null && level > depth)) return;
    if (node.nodeType === 3) {
      const text = interactive ? "" : flat(node.nodeValue);
      if (text) emit(level, `- text ${quote(cut(text, 200))}`);
      return;
    }
    if (node.nodeType !== 1 || SKIP.has(node.localName)) return;
    const el = node;
    if (el.hidden || el.getAttribute("aria-hidden") === "true") return;
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse") return;
    // A div that takes clicks (React's onClick) shows only as a pointer where its parent has none.
    const role = roleOf(el) ?? ((style.cursor === "pointer" && !pointer) || el.hasAttribute("onclick") ? "clickable" : null);
    let next = level;
    const leaf = LEAF.has(role) || (role === "clickable" && !el.querySelector(ACTIONABLE));
    if (role) {
      const acts = INTERACTIVE.has(role);
      const name = nameOf(el, leaf);
      if (!interactive || acts) {
        let ref = null;
        if (acts) ref = refOf(el);
        emit(level, line(el, role, name, ref));
        next = level + 1;
      }
      if (leaf) return;
    }
    const children = el.shadowRoot ? [...el.shadowRoot.childNodes, ...el.childNodes] : [...el.childNodes];
    for (const child of children) visit(child, next, pointer || style.cursor === "pointer");
  };
  visit(root, 0, false);
  if (full) lines.push("- … more on the page: narrow it with -s <css>, -d <n> or -i");
  return { out: lines.length ? lines.join("\n") : "(nothing on the page)", removed: prune() };
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
    if (weak.deref()?.isConnected) continue;
    state.refs.delete(ref);
    gone.push(ref);
  }
  return gone;
}

/** The element a ref (e3, @e3, ref=e3) names, or a CSS selector does. */
function find(target) {
  const ref = /^(?:@|ref=)?e(\d+)$/.exec(target);
  if (ref) {
    const name = `e${ref[1]}`;
    const el = state.refs.get(name)?.deref();
    if (el?.isConnected) return el;
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

/** Scrolled into view: where a click on it would land. */
function center(el) {
  el.scrollIntoView?.({ block: "center", inline: "center" });
  const r = el.getBoundingClientRect();
  return { clientX: r.x + r.width / 2, clientY: r.y + r.height / 2 };
}

/** Frames an element gets to show, stop moving or come out from under another one. */
const FRAMES = 30;
// A page that draws no frames (out of sight, throttled) still moves on.
const frame = () =>
  new Promise((done) => {
    const late = setTimeout(done, 50);
    window.requestAnimationFrame?.(() => {
      clearTimeout(late);
      done();
    });
  });

/** How an element reads in an error: <div#consent.banner>. */
function tagOf(el) {
  const id = el.id ? `#${el.id}` : "";
  const classes = typeof el.className === "string" ? el.className.trim().split(/\s+/).filter(Boolean).slice(0, 2) : [];
  return `<${el.localName}${cut(flat(id + classes.map((c) => `.${c}`).join("")), 40)}>`;
}

const disabled = (el) => el.matches?.(":disabled") || el.disabled === true || el.getAttribute("aria-disabled") === "true";

/** Why an element can't be acted on now, or nothing: on the page, drawn, and enabled. */
function unready(el, enabled) {
  if (!el.isConnected) return "left the page: take a new snapshot";
  const style = getComputedStyle(el);
  if (style.visibility === "hidden" || style.visibility === "collapse") return "isn't visible (visibility: hidden)";
  if (!el.getClientRects().length) return "isn't visible (display: none, here or above)";
  const r = el.getBoundingClientRect();
  if (!r.width || !r.height) return "isn't visible (no size)";
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
function top(x, y) {
  let hit = document.elementFromPoint(x, y);
  while (hit?.shadowRoot) {
    const inner = hit.shadowRoot.elementFromPoint(x, y);
    if (!inner || inner === hit) break;
    hit = inner;
  }
  return hit;
}

/**
 * Where a pointer acts on `target` (an element, a ref or CSS), in the viewport's CSS px, once it
 * can, as Playwright checks: on the page, visible, enabled (unless `enabled` is false), still for
 * a frame, and the top element at its middle is it or inside it (unless `hit` is false). Else it
 * throws why, after FRAMES of waiting. Native input (a click --at) takes its point from here too.
 */
async function point(target, { enabled = true, hit = true } = {}) {
  const el = typeof target === "string" ? find(target) : target;
  let why;
  for (let i = 0; i < FRAMES; i++, await frame()) {
    why = unready(el, enabled);
    if (why) continue;
    center(el);
    const was = el.getBoundingClientRect();
    await frame();
    const r = el.getBoundingClientRect();
    if (r.x !== was.x || r.y !== was.y || r.width !== was.width || r.height !== was.height) {
      why = "is still moving";
      continue;
    }
    // The middle of what shows of it: a box taller than the viewport is clicked where it shows.
    const left = Math.max(r.x, 0), right = Math.min(r.x + r.width, window.innerWidth ?? Infinity);
    const high = Math.max(r.y, 0), low = Math.min(r.y + r.height, window.innerHeight ?? Infinity);
    if (right <= left || low <= high) {
      why = "is outside the viewport";
      continue;
    }
    const at = { x: (left + right) / 2, y: (high + low) / 2 };
    if (!hit) return at;
    const over = top(at.x, at.y);
    if (over && reaches(el, over)) return at;
    if (!over) why = "is outside the viewport";
    else if (over.contains?.(el)) why = "doesn't take the pointer (pointer-events: none?)";
    else {
      // The cover itself, not a line of text in it.
      let cover = over;
      while (cover.parentElement && !cover.parentElement.contains(el)) cover = cover.parentElement;
      why = `is covered by ${tagOf(cover)}`;
    }
  }
  throw new Error(`${label(el)} ${why}`);
}

function mouse(el, types, at) {
  for (const type of types) {
    const init = { bubbles: true, cancelable: true, composed: true, view: window, button: 0, buttons: type.endsWith("down") ? 1 : 0, ...at };
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
  if (document.execCommand("insertText", false, text)) return;
  if (fieldOf(el)) setValue(el, el.value + text);
}

/** A click at `at` as the mouse gives one: its events in turn, focus between, then the click. */
function clickAt(el, at) {
  const where = { clientX: at.x, clientY: at.y };
  mouse(el, ["pointerover", "mouseover", "pointermove", "mousemove", "pointerdown", "mousedown"], where);
  el.focus?.({ preventScroll: true });
  mouse(el, ["pointerup", "mouseup"], where);
  el.click();
}

async function click({ target }) {
  const el = find(target);
  clickAt(el, await point(el));
  return { out: `Clicked ${label(el)}` };
}

const TEXT_TYPES = new Set(["text", "search", "email", "url", "tel", "password", "number"]);
// What the browser takes for each, as Playwright checks it: anything else it drops for "".
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
  const inner = el.querySelectorAll?.("input:not([type=hidden]), textarea, select, [contenteditable]:not([contenteditable=false])") ?? [];
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

async function fill({ target, text, secret = false }) {
  const el = control(find(target));
  const kind = writable(el);
  if (kind === "format" && text) {
    const [shape, hint] = FORMATS[typeOf(el)];
    if (!shape.test(text)) throw new Error(`Malformed value for ${label(el)} (a ${typeOf(el)} input): ${secret ? "the secret" : quote(text)} isn't ${hint}`);
  }
  await point(el, { hit: false });
  el.focus?.({ preventScroll: true });
  if (kind === "format") setValue(el, text);
  else {
    if (fieldOf(el)) el.select?.();
    else window.getSelection?.()?.selectAllChildren(el);
    const done = document.execCommand?.(text ? "insertText" : "delete", false, text);
    if (fieldOf(el) ? el.value !== text : !done) {
      if (fieldOf(el)) setValue(el, text);
      else el.textContent = text;
    }
  }
  el.dispatchEvent(new Event("change", { bubbles: true }));
  // Read back: a maxlength, a mask or the page's own handler may have kept something else.
  const now = holds(el);
  const same = kind === "rich" ? now === flat(text) : kind === "format" && typeOf(el) === "color" ? now.toLowerCase() === text.toLowerCase() : now === text;
  if (!same) {
    const long = el.maxLength > 0 && text.length > el.maxLength ? ` (maxlength ${el.maxLength})` : "";
    const it = secret ? "something else" : quote(cut(now, 120));
    throw new Error(`${label(el)} didn't take ${secret ? "the secret" : quote(cut(text, 120))}: it holds ${it}${long}`);
  }
  return { out: `Filled ${label(el)}` };
}

const KEYS = { enter: "Enter", return: "Enter", tab: "Tab", esc: "Escape", escape: "Escape", space: " ", backspace: "Backspace", delete: "Delete", up: "ArrowUp", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight", arrowup: "ArrowUp", arrowdown: "ArrowDown", arrowleft: "ArrowLeft", arrowright: "ArrowRight", home: "Home", end: "End", pageup: "PageUp", pagedown: "PageDown" };
const MODS = { meta: "metaKey", cmd: "metaKey", command: "metaKey", control: "ctrlKey", ctrl: "ctrlKey", alt: "altKey", option: "altKey", shift: "shiftKey" };

function keyEvent(el, type, key, mods = {}) {
  const code = key.length === 1 ? (/[a-z]/i.test(key) ? `Key${key.toUpperCase()}` : /\d/.test(key) ? `Digit${key}` : key === " " ? "Space" : "") : key;
  return el.dispatchEvent(new KeyboardEvent(type, { key, code, bubbles: true, cancelable: true, composed: true, ...mods }));
}

async function type({ target, text }) {
  const el = control(find(target));
  writable(el);
  await point(el, { hit: false });
  el.focus?.({ preventScroll: true });
  if (fieldOf(el)) el.setSelectionRange?.(el.value.length, el.value.length);
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
  const el = document.activeElement ?? document.body;
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
      document.execCommand(key === "Backspace" ? "delete" : "forwardDelete");
    } else if (command && key.toLowerCase() === "a") {
      if (fieldOf(el)) el.select();
      else document.execCommand("selectAll");
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
  const el = control(find(target));
  if (el.localName !== "select") throw new Error(`${label(el)} isn't a <select>`);
  const options = [...el.options];
  const text = (o) => flat(o.textContent);
  const chosen = values.map((v) => {
    const o = options.find((o) => o.value === v) ?? options.find((o) => text(o).toLowerCase() === v.toLowerCase());
    if (!o) throw new Error(`No option ${quote(v)} in ${label(el)}: ${cut(options.map((o) => quote(text(o))).join(", "), 300)}`);
    if (o.disabled) throw new Error(`Option ${quote(text(o))} in ${label(el)} is disabled`);
    return o;
  });
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
  const found = find(target);
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
  const el = find(target);
  const at = await point(el, { enabled: false });
  mouse(el, ["pointerover", "pointerenter", "mouseover", "mouseenter", "pointermove", "mousemove"], { clientX: at.x, clientY: at.y });
  return { out: `Hovered ${label(el)}` };
}

function scroll({ to, px }) {
  if (to === "up" || to === "down") {
    const by = px ?? Math.round(window.innerHeight * 0.8);
    window.scrollBy({ top: to === "up" ? -by : by, behavior: "instant" });
  } else {
    center(find(to));
  }
  const most = Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
  return { out: `Scrolled to ${Math.round(window.scrollY)} of ${most}` };
}

const shows = (el) => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden";

/** For wait: whether an element matching `css` shows. */
function exists({ css }) {
  let el;
  try {
    el = document.querySelector(css);
  } catch {
    return { found: false, invalid: true };
  }
  return { found: !!el && shows(el) };
}

const hasText = ({ text }) => ({ found: (document.body?.innerText ?? "").includes(text) });

/** For a screenshot of one element: its box in the viewport, scrolled into view. */
function rect({ target }) {
  const el = find(target);
  center(el);
  const r = el.getBoundingClientRect();
  return { rect: { x: r.x, y: r.y, w: r.width, h: r.height } };
}

// A page's lone half of a pair would reach the app as JSON it can't read.
function wellFormed(_key, value) {
  return typeof value === "string" ? value.toWellFormed() : value;
}

/** For native input: where a click on `target` lands, once it can. */
const where = async ({ target }) => await point(target);

const run = { snapshot, click, fill, type, press, select, check, hover, scroll, exists, hasText, rect, point: where };
try {
  if (!Object.hasOwn(run, cmd)) throw new Error(`No ${cmd} here`);
  return JSON.stringify({ ...(await run[cmd](a)), last: state.next - 1 }, wellFormed);
} catch (e) {
  return JSON.stringify({ error: String(e?.message ?? e), last: state.next - 1 }, wellFormed);
}
