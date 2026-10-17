// An agent's tab: the page's own clipboard calls (execCommand's copy, cut and paste, and
// navigator.clipboard) keep to the tab's store, kept by GitViber (page_tools.rs), instead of
// reaching the user's clipboard. A native click gives the page user activation that outlasts the
// command, and what a page wrote there would be the user's next paste into a terminal. So the
// guard is on from the page's start until the user acts in the tab (a press or a key of their
// own) or it becomes theirs, and on again with an agent's next command. Runs in the page's world
// at document start, in every frame, before the page's scripts can swap anything it uses.
//
// The key only GitViber and this closure know authenticates every call, and travels only in a
// string made here and handed to a function taken here: no global the page can replace (JSON,
// Reflect, bind) touches it, and no function is ever called with it but the closure's own. A
// frame is never given it: GitViber switches the main frame, which tells its frames to ask.
(() => {
  const key = __GV_KEY__;
  const origin = window.origin;
  const Wrap = Promise;
  const Text = String;
  const channel = window.webkit?.messageHandlers?.gvClipboard;
  const post = channel && channel.postMessage.bind(channel);
  const wait = setTimeout.bind(window);
  const unwait = clearTimeout.bind(window);
  // Without GitViber to ask there is no agent here: a tab it let go of (its pane closed) is the
  // user's, and their page's clipboard is theirs. A page's calls never get this far on their own.
  let on = Boolean(post);
  const exec = Document.prototype.execCommand;
  const denied = (why = "The clipboard isn't allowed here.") => new DOMException(why, "NotAllowedError");

  // The store's calls, each answered by GitViber or refused with "gv:" and why. A tab it no
  // longer answers for says so by throwing or by an error of WebKit's own; one that is only slow
  // says "slow".
  const ask = (op, text = "") =>
    new Wrap((resolve, reject) => {
      if (!post) return reject(new Error("gone"));
      const timer = wait(() => reject(new Error("slow")), 2000);
      try {
        post(`${key}\n${op}\n${origin}\n${text}`).then(
          (reply) => (unwait(timer), resolve(reply)),
          (error) => (unwait(timer), reject(error)),
        );
      } catch (error) {
        unwait(timer);
        reject(error);
      }
    });
  const why = (error) => {
    const message = Text(error?.message);
    if (message === "gv:size") return new DOMException("The tab's clipboard holds 4 MiB at most.", "QuotaExceededError");
    return denied();
  };
  const keep = (text) =>
    ask("set", text).then(
      () => undefined,
      (error) => {
        const problem = why(error);
        // The agent reads the page's console: the page may swallow the rejection.
        console.error(`GitViber: nothing was copied: ${problem.message}`);
        throw problem;
      },
    );
  const kept = () => ask("get").then((text) => text ?? "", () => { throw denied(); });

  // A frame's first press or key whose answer isn't in yet: a copy waits for it where it can.
  let settling = null;
  const settled = (run) => (settling ? settling.then(run) : run());

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
      return settled(() => {
        if (on) return keep(given);
        return own.writeText ? own.writeText.call(this, given) : realWrite(given);
      });
    },
    readText() {
      return settled(() => {
        if (on) return kept();
        return own.readText ? own.readText.call(this) : Wrap.reject(denied());
      });
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
    copy(this, this.activeElement, name).catch(() => {});
    return true;
  };

  // Whether GitViber still keeps the guard on, asked of it: the answer is the only thing that
  // lets it down, and the latest answer is the only one that counts, or an agent's click right
  // after a user's would be let through. Until the answer is in, the guard is on: a user's Enter
  // on a Copy button right then lands in the store, not their clipboard. That costs them a copy
  // and no one anything else; the other way round, an agent's click could reach the clipboard.
  let latest = 0;
  const sync = () => {
    const mine = ++latest;
    on = true;
    const answered = ask("guard").then(
      (guarded) => {
        if (mine === latest) on = guarded !== "0";
      },
      (error) => {
        // Gone: the tab is the user's. Slow: unchanged, and asked again at the next press.
        if (mine === latest && error?.message !== "slow") on = false;
      },
    );
    settling = answered;
    answered.then(() => {
      if (settling === answered) settling = null;
    });
    return answered;
  };
  if (window !== window.top) {
    const probe = (e) => {
      if (e.isTrusted) sync();
    };
    addEventListener("pointerdown", probe, true);
    addEventListener("keydown", probe, true);
  }
  // Frames are told to ask, never given the key: a frame of the page's own making may have a
  // function of its own by this name.
  const nudge = () => {
    for (let i = 0; i < window.length; i++) {
      try {
        window[i].__gvClipboard?.(null, "ask");
      } catch {}
    }
  };
  // Neither writable nor configurable: the page can't take the key from a stand-in.
  Object.defineProperty(window, "__gvClipboard", {
    value: (asked, what) => {
      if (asked !== key) {
        // Anyone may make it ask GitViber, which answers by what is true.
        if (what === "ask" && window !== window.top) {
          sync();
          nudge();
        }
        return null;
      }
      if (what === "copy" || what === "cut") {
        const at = focused();
        return at ? copy(at.doc, at.el, what).then(() => null, () => null) : null;
      }
      if (what === "paste") return paste();
      if (what === "on" || what === "off") {
        on = what === "on";
        latest++;
        nudge();
      }
      return null;
    },
  });
})();
