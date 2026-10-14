// An agent's tab: the page's own clipboard calls (navigator.clipboard, execCommand's copy, cut
// and paste) keep to the page instead of reaching the user's clipboard. An agent's native click
// gives the page the user activation they need, and what a page wrote there would be the user's
// next paste into a terminal. Runs in the page's world at document start, in every frame, before
// the page's scripts can keep WebKit's own.
(() => {
  let kept = "";
  const proto = window.Clipboard?.prototype;
  if (proto) {
    proto.writeText = async function (text) {
      kept = String(text);
    };
    proto.readText = async function () {
      return kept;
    };
    proto.write = async function (items) {
      for (const item of items ?? []) {
        if (item?.types?.includes("text/plain")) {
          kept = await (await item.getType("text/plain")).text();
          return;
        }
      }
    };
    proto.read = async function () {
      return [new ClipboardItem({ "text/plain": new Blob([kept], { type: "text/plain" }) })];
    };
  }
  const exec = Document.prototype.execCommand;
  Document.prototype.execCommand = function (command, ...rest) {
    const name = String(command).toLowerCase();
    if (name === "paste") return false;
    if (name !== "copy" && name !== "cut") return exec.call(this, command, ...rest);
    // As WebKit's: a copy event the page may fill, else the selection.
    const data = new DataTransfer();
    const event = new ClipboardEvent(name, { clipboardData: data, bubbles: true, cancelable: true, composed: true });
    const target = this.activeElement ?? this.body ?? this.documentElement;
    const field = target && (target.localName === "input" || target.localName === "textarea") && typeof target.selectionStart === "number";
    const selected = field ? target.value.slice(target.selectionStart, target.selectionEnd) : String(this.getSelection?.() ?? "");
    kept = target?.dispatchEvent(event) === false ? data.getData("text/plain") : selected;
    if (name === "cut" && !event.defaultPrevented) exec.call(this, "delete");
    return true;
  };
})();
