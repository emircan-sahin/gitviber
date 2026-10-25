// A tab shown as a touch device (a phone or tablet in device mode): the page finds a touch
// screen, and the mouse that drives the tab (the user's, or an agent's native clicks) reaches it
// as a finger. Runs in the page's world at document start, in every frame, and is added only to
// a tab whose device has touch (macos/page_tools.rs). WebKit on a Mac has no touch input, so
// this stands in for what a page can tell: touch events, maxTouchPoints, `ontouchstart`, and the
// media features (pointer: coarse), (hover: none) in matchMedia and in the styles it can read
// (a stylesheet from another origin can't be). :hover still follows the real pointer.
(() => {
  const define = (target, name, value) => Object.defineProperty(target, name, { value, writable: true, configurable: true });

  // ---- What a page checks for.
  Object.defineProperty(window.Navigator.prototype, "maxTouchPoints", { get: () => 5, configurable: true });

  class Touch {
    constructor(init) {
      Object.assign(this, { radiusX: 11.5, radiusY: 11.5, rotationAngle: 0, force: 1 }, init);
    }
  }

  class TouchList {
    constructor(items = []) {
      items.forEach((t, i) => (this[i] = t));
      define(this, "length", items.length);
    }
    item(i) {
      return this[i] ?? null;
    }
    *[Symbol.iterator]() {
      for (let i = 0; i < this.length; i++) yield this[i];
    }
  }

  class TouchEvent extends window.UIEvent {
    constructor(type, init = {}) {
      super(type, init);
      for (const k of ["touches", "targetTouches", "changedTouches"]) define(this, k, new TouchList([...(init[k] ?? [])]));
      for (const k of ["altKey", "ctrlKey", "metaKey", "shiftKey"]) define(this, k, !!init[k]);
    }
  }

  for (const [name, value] of Object.entries({ Touch, TouchList, TouchEvent })) define(window, name, value);

  const createEvent = window.Document.prototype.createEvent;
  define(window.Document.prototype, "createEvent", function (kind) {
    return /^touchevent$/i.test(kind) ? new TouchEvent("") : createEvent.call(this, kind);
  });

  // `on<touch event>` handlers, as the element and window attributes: found by `in`, and run.
  const handlers = new WeakMap();
  for (const type of ["touchstart", "touchmove", "touchend", "touchcancel"]) {
    const on = `on${type}`;
    const attribute = {
      configurable: true,
      enumerable: true,
      get() {
        return handlers.get(this)?.[on]?.fn ?? null;
      },
      set(fn) {
        const mine = handlers.get(this) ?? handlers.set(this, {}).get(this);
        if (mine[on]) this.removeEventListener(type, mine[on].run);
        mine[on] = null;
        if (typeof fn !== "function") return;
        const run = (e) => fn.call(this, e) === false && e.preventDefault();
        mine[on] = { fn, run };
        this.addEventListener(type, run);
      },
    };
    for (const proto of [window.HTMLElement?.prototype, window.SVGElement?.prototype, window.Document.prototype, window]) {
      if (proto) Object.defineProperty(proto, on, attribute);
    }
  }

  // ---- (pointer: coarse) and (hover: none), for a query that names them: the rest of it is
  // kept, each such feature standing in as a condition that is always or never so.
  const ALWAYS = "(min-width: 0px)";
  const NEVER = "(min-width: 100000px)";
  const FEATURE = /\(\s*(?:any-)?(pointer|hover)\s*(?::\s*(none|coarse|fine|hover)\s*)?\)/gi;
  const rewrite = (query) =>
    String(query).replace(FEATURE, (_all, feature, value) => {
      const touch = feature.toLowerCase() === "pointer" ? "coarse" : "none";
      return (value ? value.toLowerCase() === touch : touch === "coarse") ? ALWAYS : NEVER;
    });

  const matchMedia = window.matchMedia;
  if (matchMedia) {
    define(window, "matchMedia", function (query) {
      const list = matchMedia.call(this, rewrite(query));
      Object.defineProperty(list, "media", { value: String(query), configurable: true });
      return list;
    });
  }

  // The styles: each @media rule, @import and sheet is read for those features, once its sheet
  // is in; a rule added later through the CSSOM is rewritten on the way in. A sheet is walked
  // once (a <style> whose text changes has a new one), its @imports followed as they load.
  const seen = new WeakSet();
  const fixMedia = (media) => {
    const was = media?.mediaText;
    if (was && rewrite(was) !== was) media.mediaText = rewrite(was);
  };
  const fixRules = (rules) => {
    for (const rule of rules) {
      fixMedia(rule.media);
      if (rule.styleSheet) fixSheet(rule.styleSheet);
      if (rule.cssRules) fixRules(rule.cssRules);
    }
  };
  function fixSheet(sheet) {
    try {
      if (!seen.has(sheet)) {
        seen.add(sheet);
        fixMedia(sheet.media);
        fixRules(sheet.cssRules);
        return;
      }
      // Seen: only an @import that has arrived since, which come before the sheet's styles.
      for (const rule of sheet.cssRules) {
        if (rule.styleSheet) fixSheet(rule.styleSheet);
        else if (rule.selectorText !== undefined) break;
      }
    } catch {
      // A sheet from another origin can't be read.
    }
  }
  for (const proto of [window.CSSStyleSheet?.prototype, window.CSSGroupingRule?.prototype]) {
    const insert = proto?.insertRule;
    if (insert) define(proto, "insertRule", function (text, index) {
      return insert.call(this, rewrite(text), index);
    });
  }
  for (const name of ["replaceSync", "replace"]) {
    const original = window.CSSStyleSheet?.prototype[name];
    if (original) define(window.CSSStyleSheet.prototype, name, function (text) {
      return original.call(this, rewrite(text));
    });
  }
  if (window.MutationObserver) {
    // A <style> is read when it comes in, and again when its text is set or changed (a text
    // node put in, replaced or edited: Tailwind's script, CSS-in-JS, hot reload). A <link>
    // waits for its load below: its sheet has no rules before.
    new window.MutationObserver((records) => {
      const styles = new Set();
      for (const r of records) {
        if (r.type === "characterData") {
          if (r.target.parentNode?.localName === "style") styles.add(r.target.parentNode);
          continue;
        }
        if (r.target.localName === "style") styles.add(r.target);
        for (const node of r.addedNodes) {
          if (node.nodeType !== 1) continue;
          if (node.localName === "style") styles.add(node);
          else if (node.firstElementChild) node.querySelectorAll("style").forEach((inner) => styles.add(inner));
        }
      }
      for (const style of styles) if (style.sheet) fixSheet(style.sheet);
    }).observe(document, { childList: true, subtree: true, characterData: true });
  }
  document.addEventListener("load", (e) => e.target.sheet && fixSheet(e.target.sheet), true);
  document.addEventListener("DOMContentLoaded", () => [...(document.styleSheets ?? [])].forEach(fixSheet));

  // ---- The mouse as a finger. Its trusted events are held back and replaced by a touch's:
  // pointer events of a touch, then touch events, which stay on the element pressed. A tap
  // keeps its mouse events after (the page gets what a touch browser gives it) unless a touch
  // handler cancelled it, which also cancels them. Only the main button touches; there is no
  // hovering with a finger.
  let pressed = null;
  let cancelled = false;

  const pointer = (type, e, init = {}) =>
    new window.PointerEvent(type, {
      bubbles: true,
      cancelable: true,
      composed: true,
      view: window,
      pointerId: e.pointerId ?? 1,
      pointerType: "touch",
      isPrimary: true,
      width: 23,
      height: 23,
      clientX: e.clientX,
      clientY: e.clientY,
      screenX: e.screenX,
      screenY: e.screenY,
      altKey: e.altKey,
      ctrlKey: e.ctrlKey,
      metaKey: e.metaKey,
      shiftKey: e.shiftKey,
      ...init,
    });

  const touch = (type, e, target) => {
    const finger = new Touch({ identifier: 1, target, clientX: e.clientX, clientY: e.clientY, pageX: e.pageX, pageY: e.pageY, screenX: e.screenX, screenY: e.screenY });
    const down = type === "touchstart" || type === "touchmove";
    return new TouchEvent(type, {
      bubbles: true,
      cancelable: type !== "touchcancel",
      composed: true,
      view: window,
      touches: down ? [finger] : [],
      targetTouches: down ? [finger] : [],
      changedTouches: [finger],
      altKey: e.altKey,
      ctrlKey: e.ctrlKey,
      metaKey: e.metaKey,
      shiftKey: e.shiftKey,
    });
  };

  const held = (e) => {
    e.stopImmediatePropagation();
  };

  // What the event hit, inside a shadow tree too: `target` there is its host.
  const hit = (e) => e.composedPath?.()[0] ?? e.target;

  const listen = (type, fn) => window.addEventListener(type, (e) => e.isTrusted && e.pointerType !== "touch" && fn(e), true);

  listen("pointerdown", (e) => {
    if (e.button !== 0) return;
    held(e);
    pressed = hit(e);
    cancelled = false;
    pressed.dispatchEvent(pointer("pointerdown", e, { button: 0, buttons: 1, pressure: 0.5 }));
    if (!pressed.dispatchEvent(touch("touchstart", e, pressed))) cancelled = true;
  });
  listen("pointermove", (e) => {
    if (!pressed) return e.buttons === 0 && held(e);
    held(e);
    hit(e).dispatchEvent(pointer("pointermove", e, { button: -1, buttons: 1, pressure: 0.5 }));
    pressed.dispatchEvent(touch("touchmove", e, pressed));
  });
  listen("pointerup", (e) => {
    if (!pressed || e.button !== 0) return;
    held(e);
    const target = pressed;
    pressed = null;
    hit(e).dispatchEvent(pointer("pointerup", e, { button: 0, buttons: 0, pressure: 0 }));
    if (!target.dispatchEvent(touch("touchend", e, target))) cancelled = true;
  });
  // The press is taken from the page (a drag begins, the pointer is cancelled): the finger lifts.
  const cancel = (e) => {
    if (!pressed) return;
    const target = pressed;
    pressed = null;
    if (e.type === "pointercancel") held(e);
    target.dispatchEvent(pointer("pointercancel", e, { button: -1, buttons: 0, pressure: 0 }));
    target.dispatchEvent(touch("touchcancel", e, target));
  };
  listen("pointercancel", cancel);
  listen("dragstart", cancel);
  for (const type of ["pointerover", "pointerenter", "pointerout", "pointerleave"]) listen(type, held);
  listen("mousemove", (e) => pressed && held(e));
  // A touch handler that cancelled the tap cancels the mouse events after it, and their defaults.
  for (const type of ["mousedown", "mouseup", "click"]) {
    listen(type, (e) => {
      if (!cancelled || e.button !== 0) return;
      if (type === "click") cancelled = false;
      e.preventDefault();
      held(e);
    });
  }
})();
