// An agent's tab: the page's own clipboard calls (execCommand's copy, cut and paste, and
// navigator.clipboard) keep to the tab's one store, kept by GitViber (page_tools.rs), instead of
// reaching the user's clipboard. A native click gives the page user activation that outlasts the
// command, and what a page wrote there would be the user's next paste into a terminal. So the
// guard is on from the page's start until the user acts in the tab (a press or a key of their
// own) or it becomes theirs, and on again with an agent's next command. Runs in the page's world
// at document start, in every frame, before the page's scripts can keep WebKit's own. Only
// GitViber knows the key: it switches the main frame's guard (which passes it to the frames of
// its origin), and a frame of another origin asks it on its own first press or key.
(() => {
  const key = __GV_KEY__;
  let on = true;
  const channel = window.webkit?.messageHandlers?.gvClipboard;
  const post = channel && channel.postMessage.bind(channel);
  // The store's calls: set, get, and guard (whether GitViber keeps the guard on).
  const ask = (op, text) =>
    post ? post(JSON.stringify({ key, op, text })).catch(() => "") : Promise.resolve("");
  const keep = (text) => ask("set", text).then(() => undefined);
  const kept = () => ask("get").then((text) => text ?? "");
  const denied = () => new DOMException("The clipboard isn't allowed here.", "NotAllowedError");
  const exec = Document.prototype.execCommand;

  // WebKit's async API is off in an agent's tab (page_tools.rs), so a page finds none: it gets
  // writeText and readText of the store while an agent drives, and a real copy once the tab is
  // the user's (execCommand's, which asks for the user's own gesture, as the API would).
  const realWrite = (text) => {
    const doc = document;
    const active = doc.activeElement;
    const selection = doc.getSelection?.();
    const ranges = selection ? Array.from({ length: selection.rangeCount }, (_, i) => selection.getRangeAt(i)) : [];
    const field = doc.createElement("textarea");
    field.value = text;
    field.readOnly = true;
    field.style.cssText = "position:fixed;top:0;left:0;width:1px;height:1px;opacity:0;pointer-events:none";
    // The page's copy listeners on the document can't swap the text: this one runs first and stops the rest.
    const fill = (e) => {
      e.stopImmediatePropagation();
      e.preventDefault();
      e.clipboardData?.setData("text/plain", text);
    };
    addEventListener("copy", fill, true);
    (doc.body ?? doc.documentElement).append(field);
    field.select();
    let ok = false;
    try {
      ok = exec.call(doc, "copy");
    } catch {}
    removeEventListener("copy", fill, true);
    field.remove();
    if (selection) {
      selection.removeAllRanges();
      for (const range of ranges) selection.addRange(range);
    }
    active?.focus?.({ preventScroll: true });
    return ok ? Promise.resolve() : Promise.reject(denied());
  };

  const proto = window.Clipboard?.prototype;
  const own = { writeText: proto?.writeText, readText: proto?.readText, write: proto?.write, read: proto?.read };
  const api = {
    writeText(text) {
      if (on) return keep(String(text));
      return own.writeText ? own.writeText.call(this, text) : realWrite(String(text));
    },
    readText() {
      if (on) return kept();
      return own.readText ? own.readText.call(this) : Promise.reject(denied());
    },
  };
  if (proto) {
    proto.writeText = api.writeText;
    proto.readText = api.readText;
    proto.write = async function (items) {
      if (!on) return own.write.call(this, items);
      for (const item of items ?? []) {
        if (item?.types?.includes("text/plain")) return keep(await (await item.getType("text/plain")).text());
      }
    };
    proto.read = async function () {
      if (!on) return own.read.call(this);
      return [new ClipboardItem({ "text/plain": new Blob([await kept()], { type: "text/plain" }) })];
    };
  } else if (window.Navigator && !("clipboard" in navigator)) {
    const clipboard = Object.create(null, {
      writeText: { value: api.writeText, enumerable: true },
      readText: { value: api.readText, enumerable: true },
      [Symbol.toStringTag]: { value: "Clipboard" },
    });
    Object.defineProperty(Navigator.prototype, "clipboard", { get: () => clipboard, enumerable: true, configurable: true });
  }

  // The document of a frame down the focus, if its origin is the page's: another's can't be
  // reached from here, and its own text is the native copy and paste's.
  const focused = () => {
    let doc = document;
    for (;;) {
      const el = doc.activeElement;
      if (el?.localName !== "iframe" && el?.localName !== "frame") return { doc, el };
      let inner = null;
      try {
        inner = el.contentDocument;
      } catch {}
      if (!inner) return null;
      doc = inner;
    }
  };
  // As WebKit's: a copy event the page may fill, else the selection.
  const copy = (doc, el, name) => {
    const data = new DataTransfer();
    const event = new ClipboardEvent(name, { clipboardData: data, bubbles: true, cancelable: true, composed: true });
    const target = el ?? doc.body ?? doc.documentElement;
    const field = target && (target.localName === "input" || target.localName === "textarea") && typeof target.selectionStart === "number";
    const secret = field && target.type === "password";
    const selected = secret ? "" : field ? target.value.slice(target.selectionStart, target.selectionEnd) : String(doc.getSelection?.() ?? "");
    const text = target?.dispatchEvent(event) === false ? data.getData("text/plain") : selected;
    if (name === "cut" && !event.defaultPrevented) exec.call(doc, "delete");
    return keep(text);
  };
  // A paste event with the store's text: the text, for GitViber to put in, unless the page took it.
  const paste = async () => {
    const text = await kept();
    const at = focused();
    if (!at) return text;
    const data = new DataTransfer();
    data.setData("text/plain", text);
    const event = new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true, composed: true });
    const target = at.el ?? at.doc.body ?? at.doc.documentElement;
    return target?.dispatchEvent(event) === false ? "" : text;
  };
  Document.prototype.execCommand = function (command, ...rest) {
    const name = String(command).toLowerCase();
    if (!on || (name !== "copy" && name !== "cut" && name !== "paste")) return exec.call(this, command, ...rest);
    if (name === "paste") return false;
    copy(this, this.activeElement, name);
    return true;
  };

  // A frame of another origin hears nothing from the main frame: its first press or key says
  // the guard is on until GitViber, who knows whose press it was, says otherwise. Only the latest
  // answer counts, or an agent's click right after a user's would be let through.
  let latest = 0;
  if (window !== window.top) {
    const probe = (e) => {
      if (!e.isTrusted) return;
      const mine = ++latest;
      on = true;
      ask("guard").then((guarded) => {
        if (mine === latest && guarded === "0") on = false;
      });
    };
    addEventListener("pointerdown", probe, true);
    addEventListener("keydown", probe, true);
  }
  // Neither writable nor configurable: the page can't take the key from a stand-in.
  Object.defineProperty(window, "__gvClipboard", {
    value: (asked, what) => {
      if (asked !== key) return null;
      if (what === "copy" || what === "cut") {
        const at = focused();
        return at ? copy(at.doc, at.el, what).then(() => null) : null;
      }
      if (what === "paste") return paste();
      if (what === "on" || what === "off") {
        on = what === "on";
        latest++;
        // The frames of this origin, down to the last.
        for (let i = 0; i < window.length; i++) {
          try {
            window[i].__gvClipboard?.(key, what);
          } catch {}
        }
      }
      return null;
    },
  });
})();
