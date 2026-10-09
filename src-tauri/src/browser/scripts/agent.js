// What `gitviber browser` does in a page (browser/agent.rs): the page as a tree an agent can act
// on, and acting on what it named. The body of an async function run in GitViber's own content
// world with `input` as JSON: the page can't see this code or its refs, and the refs (WeakRefs
// in this world's window) go with the page.
const { cmd, ...a } = JSON.parse(input);
const state = (window.__gvAgent ??= { refs: new Map() });

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

const flat = (s) => String(s ?? "").replace(/\s+/g, " ").trim();
const cut = (s, n = NAME) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);
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
  if (el.getAttribute("type") === "password") return el.value ? "•".repeat(Math.min(el.value.length, 8)) : "";
  return "value" in el && typeof el.value === "string" ? el.value : flat(el.textContent);
}

/** One line of the tree: role, name, ref, and what an agent needs to know of its state. */
function line(el, role, name, ref) {
  let out = `- ${role}${name ? ` ${quote(cut(name))}` : ""}`;
  if (role === "heading") out += ` [level=${el.getAttribute("aria-level") ?? el.localName.slice(1)}]`;
  if (ref) out += ` [ref=${ref}]`;
  if (el.checked === true || el.getAttribute("aria-checked") === "true") out += " [checked]";
  if (el.disabled === true || el.getAttribute("aria-disabled") === "true") out += " [disabled]";
  const expanded = el.getAttribute("aria-expanded");
  if (expanded) out += expanded === "true" ? " [expanded]" : " [collapsed]";
  if (el.getAttribute("aria-selected") === "true") out += " [selected]";
  if (role === "textbox" || role === "searchbox" || role === "combobox" || role === "spinbutton" || role === "slider") out += ` value=${quote(cut(valueOf(el)))}`;
  if (role === "link") out += ` -> ${cut(el.getAttribute("href") ?? "", 120)}`;
  return out;
}

function snapshot({ interactive = false, scope = null, depth = null }) {
  const root = scope ? document.querySelector(scope) : (document.body ?? document.documentElement);
  if (!root) throw new Error(`No element matches ${scope}`);
  state.refs = new Map();
  const lines = [];
  let more = 0;
  const emit = (level, text) => (lines.length < LINES ? lines.push(`${"  ".repeat(interactive ? 0 : level)}${text}`) : more++);
  const visit = (node, level, pointer) => {
    if (depth !== null && level > depth) return;
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
        if (acts) {
          ref = `e${state.refs.size + 1}`;
          state.refs.set(ref, new WeakRef(el));
        }
        emit(level, line(el, role, name, ref));
        next = level + 1;
      }
      if (leaf) return;
    }
    const children = el.shadowRoot ? [...el.shadowRoot.childNodes, ...el.childNodes] : [...el.childNodes];
    for (const child of children) visit(child, next, pointer || style.cursor === "pointer");
  };
  visit(root, 0, false);
  if (more) lines.push(`- … ${more} more lines: narrow it with -s <css>, -d <n> or -i`);
  return { out: lines.length ? lines.join("\n") : "(nothing on the page)" };
}

/** The element a ref (e3, @e3, ref=e3) from the last snapshot names, or a CSS selector does. */
function find(target) {
  const ref = /^(?:@|ref=)?(e\d+)$/.exec(target);
  if (ref) {
    const el = state.refs.get(ref[1])?.deref();
    if (!el?.isConnected) throw new Error(`No ${ref[1]} on the page now: take a snapshot again`);
    return el;
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

function click({ target }) {
  const el = find(target);
  if (el.disabled === true || el.getAttribute("aria-disabled") === "true") throw new Error(`${label(el)} is disabled`);
  const at = center(el);
  mouse(el, ["pointerover", "mouseover", "pointermove", "mousemove", "pointerdown", "mousedown"], at);
  el.focus?.({ preventScroll: true });
  mouse(el, ["pointerup", "mouseup"], at);
  el.click();
  return { out: `Clicked ${label(el)}` };
}

function fill({ target, text }) {
  const el = find(target);
  if (el.localName === "select") throw new Error(`${label(el)} is a <select>: use select`);
  center(el);
  el.focus?.({ preventScroll: true });
  if (fieldOf(el)) el.select();
  else window.getSelection()?.selectAllChildren(el);
  const done = document.execCommand(text ? "insertText" : "delete", false, text);
  if (fieldOf(el) ? el.value !== text : !done) {
    if (fieldOf(el)) setValue(el, text);
    else el.textContent = text;
  }
  el.dispatchEvent(new Event("change", { bubbles: true }));
  return { out: `Filled ${label(el)}` };
}

const KEYS = { enter: "Enter", return: "Enter", tab: "Tab", esc: "Escape", escape: "Escape", space: " ", backspace: "Backspace", delete: "Delete", up: "ArrowUp", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight", arrowup: "ArrowUp", arrowdown: "ArrowDown", arrowleft: "ArrowLeft", arrowright: "ArrowRight", home: "Home", end: "End", pageup: "PageUp", pagedown: "PageDown" };
const MODS = { meta: "metaKey", cmd: "metaKey", command: "metaKey", control: "ctrlKey", ctrl: "ctrlKey", alt: "altKey", option: "altKey", shift: "shiftKey" };

function keyEvent(el, type, key, mods = {}) {
  const code = key.length === 1 ? (/[a-z]/i.test(key) ? `Key${key.toUpperCase()}` : /\d/.test(key) ? `Digit${key}` : key === " " ? "Space" : "") : key;
  return el.dispatchEvent(new KeyboardEvent(type, { key, code, bubbles: true, cancelable: true, composed: true, ...mods }));
}

function type({ target, text }) {
  const el = find(target);
  center(el);
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

function select({ target, values }) {
  const el = find(target);
  if (el.localName !== "select") throw new Error(`${label(el)} isn't a <select>`);
  const options = [...el.options];
  const text = (o) => flat(o.textContent);
  const chosen = values.map((v) => {
    const o = options.find((o) => o.value === v) ?? options.find((o) => text(o).toLowerCase() === v.toLowerCase());
    if (!o) throw new Error(`No option ${quote(v)} in ${label(el)}: ${cut(options.map((o) => quote(text(o))).join(", "), 300)}`);
    return o;
  });
  if (chosen.length > 1 && !el.multiple) throw new Error(`${label(el)} takes one option`);
  for (const o of options) o.selected = chosen.includes(o);
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
  return { out: `Selected ${chosen.map((o) => quote(text(o))).join(", ")} in ${label(el)}` };
}

function hover({ target }) {
  const el = find(target);
  mouse(el, ["pointerover", "pointerenter", "mouseover", "mouseenter", "pointermove", "mousemove"], center(el));
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
    throw new Error(`Not a CSS selector: ${css}`);
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

const run = { snapshot, click, fill, type, press, select, hover, scroll, exists, hasText, rect };
try {
  if (!Object.hasOwn(run, cmd)) throw new Error(`No ${cmd} here`);
  return JSON.stringify(run[cmd](a));
} catch (e) {
  return JSON.stringify({ error: String(e?.message ?? e) });
}
