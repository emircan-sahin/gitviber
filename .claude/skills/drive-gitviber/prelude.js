// Helpers for `ui.sh js`, put in front of every script. Synthetic events are untrusted, which the
// app doesn't check; native-only paths (menus, open panels, clipboard) still need ui.sh's real keys.
const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];
const visible = (e) => !!(e.offsetWidth || e.offsetHeight || e.getClientRects().length);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** The innermost visible element whose text is `t` (exactly, else starting with it). */
const byText = (t, root = document) => {
  const all = [...root.querySelectorAll("*")].filter(visible);
  const exact = all.filter((e) => e.textContent.trim() === t);
  const pick = (list) => list.find((e) => ![...e.children].some((c) => list.includes(c)));
  return pick(exact) ?? pick(all.filter((e) => e.textContent.trim().startsWith(t)));
};
const find = (x) => {
  const e = typeof x === "string" ? ($(x) ?? byText(x)) : x;
  if (!e) throw new Error(`not found: ${x}`);
  return e;
};
/** Radix opens menus on pointerdown, so a plain .click() isn't enough. */
const press = (x, extra = {}) => {
  const e = find(x);
  const o = { bubbles: true, cancelable: true, composed: true, button: 0, pointerType: "mouse", ...extra };
  for (const t of ["pointerdown", "mousedown", "pointerup", "mouseup", "click"])
    e.dispatchEvent(new (t.startsWith("pointer") ? PointerEvent : MouseEvent)(t, o));
  return e.textContent.trim().slice(0, 80);
};
const rightClick = (x) => {
  const e = find(x);
  const r = e.getBoundingClientRect();
  e.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: r.x + 5, clientY: r.y + 5 }));
};
/** A key on the focused element, e.g. key("s", { altKey: true, metaKey: true, code: "KeyS" }). */
const key = (k, mods = {}) => {
  const t = document.activeElement ?? document.body;
  const o = { key: k, code: mods.code ?? (k.length === 1 ? `Key${k.toUpperCase()}` : k), bubbles: true, cancelable: true, ...mods };
  t.dispatchEvent(new KeyboardEvent("keydown", o));
  t.dispatchEvent(new KeyboardEvent("keyup", o));
};
/** Types into an input or textarea the way React notices. */
const typeInto = (x, text) => {
  const e = find(x);
  e.focus();
  const proto = e instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value").set.call(e, text);
  e.dispatchEvent(new Event("input", { bubbles: true }));
};
const waitFor = async (fn, ms = 5000) => {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${fn}`);
    await sleep(50);
  }
};
const text = (x) => find(x).innerText;
/** What the toasts say now. */
const toasts = () => $$("[aria-label=Notifications] [role=status], [aria-label=Notifications] [role=alert]").map((e) => e.innerText.trim());
