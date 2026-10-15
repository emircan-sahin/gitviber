// Acting: where an action acts once a person could (actionability, mapped to the top viewport
// for native input), and each action by the page's own events.

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

/** Focused for typing, its caret at the end of its text: where native typing goes in. */
function focus({ target }) {
  const el = resolve(target);
  into(el);
  el.focus?.({ preventScroll: true });
  try {
    if (fieldOf(el)) el.setSelectionRange(el.value.length, el.value.length);
    else if (el.isContentEditable) viewOf(el).getSelection?.()?.collapse(el, el.childNodes.length);
  } catch {
    // An email or number field has no caret to place: it types at its end anyway.
  }
  const active = deepActive();
  if (!el.contains(active)) throw new Error(`${label(el)} doesn't take focus: click it, or fill it`);
  // An <input> is one line, a web component's own too: a line break there would submit its form.
  return { label: label(el), line: active?.localName === "input" };
}

/** What has focus, down through same-origin frames and shadow roots, where keys go. */
function deepActive() {
  let el = document.activeElement;
  for (;;) {
    const frame = el?.localName === "iframe" || el?.localName === "frame" ? frameDoc(el) : null;
    const inner = el?.shadowRoot?.activeElement ?? frame?.activeElement;
    if (!inner || inner === el) return el;
    el = inner;
  }
}

/** What has focus, for a key's reply, and whether it's a <select> a key would open the menu of. */
const focused = () => {
  const el = deepActive();
  const menu = el?.localName === "select" && !el.multiple && el.size <= 1;
  return { label: el && el !== docOf(el).body ? label(el) : "the page", menu };
};

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

/**
 * Where an action acts, for agent.rs: `target`'s point once it's actionable, or the point `x`,
 * `y` itself, in the top viewport even for an element in a frame; and what's there, for its
 * reply. For a native press there, whether it would open a native picker (a <select>'s menu, a
 * colour panel) or start HTML5 drag-and-drop, which agent.rs refuses: the one holds the app, the
 * other follows the real mouse. `scrolled`: the page, or the element's frame, moved to bring it
 * into view, which may have moved another point already taken.
 */
async function aim({ target = null, x = null, y = null }) {
  const where = (view) => [view.scrollX, view.scrollY];
  const was = where(window);
  let el, at, inner;
  if (target !== null) {
    el = resolve(target);
    // A hidden file input is pressed through its label, as the page's own button does.
    if (el.localName === "input" && el.type === "file" && !el.getClientRects().length) el = el.labels?.[0] ?? el;
    const view = viewOf(el);
    if (view !== window) inner = [view, where(view)];
    at = onTop(el, await point(el));
  } else {
    if (!(x >= 0 && y >= 0 && x < window.innerWidth && y < window.innerHeight)) {
      throw new Error(`${x},${y} is outside the ${window.innerWidth}×${window.innerHeight} viewport`);
    }
    el = top(document, x, y);
    at = { x, y };
  }
  const menu = el?.closest?.("select");
  const picker = (!!menu && !menu.multiple && menu.size <= 1) || (el?.localName === "input" && el.type === "color");
  const selection = (el ? viewOf(el) : window).getSelection?.();
  const selected = !!el && !!selection && !selection.isCollapsed && !!selection.containsNode?.(el, true);
  const drag = !!el?.closest?.('[draggable="true"], a[href]:not([draggable="false"]), img:not([draggable="false"])') || selected;
  const moved = (view, was) => view.scrollX !== was[0] || view.scrollY !== was[1];
  const scrolled = moved(window, was) || (!!inner && moved(...inner));
  return { ...at, label: el ? label(el) : "nothing", picker, drag, scrolled };
}
