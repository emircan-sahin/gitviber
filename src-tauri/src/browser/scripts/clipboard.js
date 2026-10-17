// An agent's tab: the page's own clipboard calls (execCommand's copy, cut and paste, and
// navigator.clipboard) keep to the tab's store, kept by GitViber (page_tools.rs), instead of
// reaching the user's clipboard. A native click gives the page user activation that outlasts the
// command, and what a page wrote there would be the user's next paste into a terminal. So the
// guard is on from the page's start, in every frame, and stays on until GitViber says otherwise:
// it pushes the guard from outside, by evaluating in each frame that said hello, when the user
// acts in the tab or it becomes theirs, and again with an agent's next command. Nothing the
// page can reach decides it: no reply to a promise, no method of a prototype, no property of
// window is read for it, so a page that replaces them all, in this frame or another, only
// changes what it sees itself. Runs in the page's world at document start, in every frame,
// before the page's scripts can swap anything it uses.
//
// Two secrets, neither readable by the page. The tab's key, which only ever leaves in a string
// made here for a function taken here (no global the page can replace carries it), authenticates
// the frame to GitViber. The frame's own token, said in the hello, is what GitViber's pushes
// answer to: a frame that is gone and replaced by a page of the page's making holds a token that
// has no use.
(() => {
  const key = __GV_KEY__;
  const origin = window.origin;
  const Wrap = Promise;
  const chain = Promise.prototype.then;
  const Text = String;
  const channel = window.webkit?.messageHandlers?.gvClipboard;
  const post = channel && channel.postMessage.bind(channel);
  const wait = setTimeout.bind(window);
  const unwait = clearTimeout.bind(window);
  const token = Array.from(crypto.getRandomValues(new Uint32Array(4)), (n) => n.toString(16)).join("");
  // The most a copy can be, in characters: GitViber's cap is in bytes and refuses what is over
  // it, but a longer string is not worth the trip to it.
  const MAX = 4 << 20;
  // Without GitViber to talk to there is no agent here: the user's. Otherwise on, until pushed.
  let on = Boolean(post);
  const exec = Document.prototype.execCommand;
  const denied = (why = "The clipboard isn't allowed here.") => new DOMException(why, "NotAllowedError");

  // The store's calls, each answered by GitViber or refused with "gv:" and why. What an answer is
  // is only ever text for the page: nothing here lets one decide the guard.
  const ask = (op, text = "") =>
    new Wrap((resolve, reject) => {
      if (!post) return reject(new Error("gone"));
      const timer = wait(() => reject(new Error("slow")), 2000);
      try {
        chain.call(
          post(`${key}\n${op}\n${origin}\n${text}`),
          (reply) => (unwait(timer), resolve(reply)),
          (error) => (unwait(timer), reject(error)),
        );
      } catch (error) {
        unwait(timer);
        reject(error);
      }
    });
  const why = (error) => {
    if (Text(error?.message) === "gv:size") return new DOMException("The tab's clipboard holds 4 MiB at most.", "QuotaExceededError");
    return denied();
  };
  // The agent reads the page's console: the page may swallow the rejection.
  const refuse = (problem) => {
    console.error(`GitViber: nothing was copied: ${problem.message}`);
    return problem;
  };
  const keep = (text) => {
    if (text.length > MAX) return Wrap.reject(refuse(why(new Error("gv:size"))));
    return chain.call(ask("set", text), () => undefined, (error) => {
      throw refuse(why(error));
    });
  };
  const kept = () =>
    chain.call(ask("get"), (text) => text ?? "", () => {
      throw denied();
    });

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
    return ok ? Wrap.resolve() : Wrap.reject(denied());
  };

  const proto = window.Clipboard?.prototype;
  const own = { writeText: proto?.writeText, readText: proto?.readText, write: proto?.write, read: proto?.read };
  const api = {
    writeText(text) {
      const given = Text(text);
      if (on) return keep(given);
      return own.writeText ? own.writeText.call(this, given) : realWrite(given);
    },
    readText() {
      if (on) return kept();
      return own.readText ? own.readText.call(this) : Wrap.reject(denied());
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
  // reached from here, and its origin has a store of its own that this frame can't speak for.
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
    const selected = secret ? "" : field ? target.value.slice(target.selectionStart, target.selectionEnd) : Text(doc.getSelection?.() ?? "");
    const text = target?.dispatchEvent(event) === false ? data.getData("text/plain") : selected;
    if (name === "cut" && !event.defaultPrevented) exec.call(doc, "delete");
    return keep(text);
  };
  // A paste event with the store's text: the text, for GitViber to put in, unless the page took
  // it. None in a frame of another origin: its text is not this page's to give.
  const paste = async () => {
    const at = focused();
    if (!at) return "";
    let text = "";
    try {
      text = await kept();
    } catch {
      return "";
    }
    const data = new DataTransfer();
    data.setData("text/plain", text);
    const event = new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true, composed: true });
    const target = at.el ?? at.doc.body ?? at.doc.documentElement;
    return target?.dispatchEvent(event) === false ? "" : text;
  };
  Document.prototype.execCommand = function (command, ...rest) {
    const name = Text(command).toLowerCase();
    if (!on || (name !== "copy" && name !== "cut" && name !== "paste")) return exec.call(this, command, ...rest);
    if (name === "paste") return false;
    chain.call(copy(this, this.activeElement, name), undefined, () => {});
    return true;
  };

  // GitViber's calls, by this frame's token alone. Neither writable nor configurable: the page
  // can't put a stand-in of its own in its place. Anyone else's call changes nothing.
  Object.defineProperty(window, "__gvClipboard", {
    value: (asked, what) => {
      if (asked !== token) return null;
      if (what === "on" || what === "off") on = what === "on";
      else if (what === "copy" || what === "cut") {
        const at = focused();
        return at ? chain.call(copy(at.doc, at.el, what), () => null, () => null) : null;
      } else if (what === "paste") return paste();
      return null;
    },
  });
  // Hello, so that GitViber can push the guard to this frame: nothing it says comes back through
  // here.
  if (post) chain.call(ask("hello", token), undefined, () => {});
})();
