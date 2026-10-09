// The element picker (browser/macos/page_tools.rs pick). Runs in GitViber's own content world: the page
// can't see this code or post to gvPick, so it can't forge a pick. Defined once a page, then
// started and stopped by name.
(() => {
  if (window.__gvPicker) return;
  // What says how it looks, without the font stack or the size the box already gives.
  const STYLES = ["display", "position", "color", "background-color", "font-size", "font-weight", "padding"];
  let host = null;
  let outline = null;
  let hovered = null;
  let nonce = "";
  // Each pick's own tag on its element, the nonce and its number: ⇧-clicks may tag a few at once.
  let picks = 0;

  const post = (message) => window.webkit.messageHandlers.gvPick.postMessage(JSON.stringify(message));

  // Short and unique: an id, a test id, else the path of tags and their places among siblings.
  const selector = (el) => {
    const unique = (s) => {
      try {
        return document.querySelectorAll(s).length === 1;
      } catch {
        return false;
      }
    };
    if (el.id && unique(`#${CSS.escape(el.id)}`)) return `#${CSS.escape(el.id)}`;
    for (const attr of ["data-testid", "data-test", "data-cy"]) {
      const v = el.getAttribute(attr);
      if (v && unique(`[${attr}="${CSS.escape(v)}"]`)) return `[${attr}="${CSS.escape(v)}"]`;
    }
    const parts = [];
    for (let n = el; n && n.nodeType === 1 && n !== document.documentElement; n = n.parentElement) {
      if (n.id && unique(`#${CSS.escape(n.id)}`)) {
        parts.unshift(`#${CSS.escape(n.id)}`);
        break;
      }
      const tag = n.localName;
      const same = n.parentElement ? [...n.parentElement.children].filter((c) => c.localName === tag) : [];
      parts.unshift(same.length > 1 ? `${tag}:nth-of-type(${same.indexOf(n) + 1})` : tag);
      if (unique(parts.join(" > "))) break;
    }
    return parts.join(" > ");
  };

  // To max UTF-16 units with the ellipsis, never splitting a pair (as console.js).
  const cut = (s, max) => {
    if (s.length <= max) return s;
    const end = /[\uD800-\uDBFF]/.test(s[max - 2]) ? max - 2 : max - 1;
    return `${s.slice(0, end)}…`;
  };

  const pick = (el) => {
    const box = el.getBoundingClientRect();
    const computed = getComputedStyle(el);
    const found = {
      selector: selector(el),
      tag: el.localName,
      html: cut(el.outerHTML, 600),
      text: cut((el.innerText || "").trim(), 200),
      box: { x: box.x, y: box.y, w: box.width, h: box.height },
      styles: Object.fromEntries(STYLES.map((s) => [s, computed.getPropertyValue(s)])),
      nonce: `${nonce}:${++picks}`,
    };
    // Read above, before it: for the page's own world, which reads React's fiber off the element.
    el.setAttribute("data-gv-pick", found.nonce);
    return found;
  };

  const target = (e) => document.elementFromPoint(e.clientX, e.clientY);

  const show = (el) => {
    hovered = el;
    if (!el) return void (outline.style.display = "none");
    const r = el.getBoundingClientRect();
    Object.assign(outline.style, { display: "block", left: `${r.x}px`, top: `${r.y}px`, width: `${r.width}px`, height: `${r.height}px` });
  };

  const swallow = (e) => {
    e.preventDefault();
    e.stopPropagation();
    e.stopImmediatePropagation();
  };

  const onMove = (e) => show(target(e));
  // ⇧-click adds the element and picks on; a click picks the last one.
  const onClick = (e) => {
    swallow(e);
    const el = target(e) || hovered;
    const more = !!el && e.shiftKey;
    if (!more) stop();
    post(el ? { pick: { ...pick(el), more } } : { cancelled: true });
  };
  const onKey = (e) => {
    if (e.key !== "Escape") return;
    swallow(e);
    stop();
    post({ cancelled: true });
  };

  const LISTENERS = [
    ["mousemove", onMove],
    ["click", onClick],
    ["keydown", onKey],
    ["mousedown", swallow],
    ["mouseup", swallow],
    ["pointerdown", swallow],
    ["pointerup", swallow],
  ];

  function start(next) {
    stop();
    nonce = next;
    host = document.createElement("div");
    // A closed shadow root: the page's styles and scripts can't reach the outline.
    const root = host.attachShadow({ mode: "closed" });
    outline = document.createElement("div");
    outline.style.cssText = "position:fixed;display:none;pointer-events:none;box-sizing:border-box;border:2px solid #3b82f6;background:rgba(59,130,246,.12);border-radius:2px;z-index:2147483647";
    root.append(outline);
    host.style.cssText = "position:fixed;inset:0;pointer-events:none;z-index:2147483647";
    document.documentElement.append(host);
    for (const [type, fn] of LISTENERS) window.addEventListener(type, fn, true);
  }

  function stop() {
    for (const [type, fn] of LISTENERS) window.removeEventListener(type, fn, true);
    host?.remove();
    host = outline = hovered = null;
  }

  window.__gvPicker = { start, stop };
})();
