// An agent's tab: the page's own clipboard calls (execCommand's copy, cut and paste, and
// navigator.clipboard where WebKit still has it on) keep to the page instead of reaching the
// user's clipboard. A native click gives the page user activation that outlasts the command,
// and what a page wrote there would be the user's next paste into a terminal. So the guard is
// on from the page's start until the user acts in the tab (a press or a key of their own) or it
// becomes theirs, and on again with an agent's next command. Runs in the page's world at
// document start, in every frame, before the page's scripts can keep WebKit's own; only
// GitViber knows its key, so only it switches the guard (macos/page_tools.rs), in the main
// frame: a frame's guard stays on, and each frame keeps the text it copied.
(() => {
  const key = __GV_KEY__;
  let on = true;
  let kept = "";
  const proto = window.Clipboard?.prototype;
  const own = { writeText: proto?.writeText, readText: proto?.readText, write: proto?.write, read: proto?.read };
  if (proto) {
    proto.writeText = function (text) {
      if (!on) return own.writeText.call(this, text);
      kept = String(text);
      return Promise.resolve();
    };
    proto.readText = function () {
      return on ? Promise.resolve(kept) : own.readText.call(this);
    };
    proto.write = async function (items) {
      if (!on) return own.write.call(this, items);
      for (const item of items ?? []) {
        if (item?.types?.includes("text/plain")) {
          kept = await (await item.getType("text/plain")).text();
          return;
        }
      }
    };
    proto.read = function () {
      if (!on) return own.read.call(this);
      return Promise.resolve([new ClipboardItem({ "text/plain": new Blob([kept], { type: "text/plain" }) })]);
    };
  }
  const exec = Document.prototype.execCommand;
  // As WebKit's: a copy event the page may fill, else the selection.
  const copy = (doc, name) => {
    const data = new DataTransfer();
    const event = new ClipboardEvent(name, { clipboardData: data, bubbles: true, cancelable: true, composed: true });
    const target = doc.activeElement ?? doc.body ?? doc.documentElement;
    const field = target && (target.localName === "input" || target.localName === "textarea") && typeof target.selectionStart === "number";
    const secret = field && target.type === "password";
    const selected = secret ? "" : field ? target.value.slice(target.selectionStart, target.selectionEnd) : String(doc.getSelection?.() ?? "");
    kept = target?.dispatchEvent(event) === false ? data.getData("text/plain") : selected;
    if (name === "cut" && !event.defaultPrevented) exec.call(doc, "delete");
    return true;
  };
  // A paste event with the kept text: the text, for GitViber to put in, unless the page took it.
  const paste = () => {
    const data = new DataTransfer();
    data.setData("text/plain", kept);
    const event = new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true, composed: true });
    const target = document.activeElement ?? document.body ?? document.documentElement;
    return target?.dispatchEvent(event) === false ? "" : kept;
  };
  Document.prototype.execCommand = function (command, ...rest) {
    const name = String(command).toLowerCase();
    if (!on || (name !== "copy" && name !== "cut" && name !== "paste")) return exec.call(this, command, ...rest);
    return name === "paste" ? false : copy(this, name);
  };
  // Neither writable nor configurable: the page can't take the key from a stand-in.
  Object.defineProperty(window, "__gvClipboard", {
    value: (asked, ask) => {
      if (asked !== key) return null;
      if (ask === "on" || ask === "off") on = ask === "on";
      else if (ask === "copy" || ask === "cut") copy(document, ask);
      else if (ask === "paste") return paste();
      return null;
    },
  });
})();
