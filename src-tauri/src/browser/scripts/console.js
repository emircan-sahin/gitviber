// The page's errors and warnings, for the browser tab's console badge (browser/console.rs). Runs
// in the page's own world at document start, so it sees what the page logs; the page could post
// to gvConsole itself, which only adds lines to its own console.
(() => {
  const handler = window.webkit?.messageHandlers?.gvConsole;
  if (!handler || window.__gvConsole) return;
  window.__gvConsole = true;
  const MAX = 2048;
  // A page logging in a loop would otherwise post (and the app parse) every line of it.
  const PER_SECOND = 50;
  const later = setTimeout;
  const TOO_LONG = {};
  // To MAX UTF-16 units with the ellipsis, never splitting a pair (the app's JSON parser refuses
  // half of one).
  const cut = (s) => {
    if (s.length <= MAX) return s;
    const end = /[\uD800-\uDBFF]/.test(s[MAX - 2]) ? MAX - 2 : MAX - 1;
    return `${s.slice(0, end)}…`;
  };
  const text = (v) => {
    if (v instanceof Error) return `${v.name}: ${v.message}`;
    if (typeof v === "string") return v;
    let size = 0;
    try {
      // Stops walking a huge object once what it has written can't fit anyway.
      return (
        JSON.stringify(v, (key, x) => {
          size += key.length + (typeof x === "string" ? x.length : 8);
          if (size > MAX) throw TOO_LONG;
          return x;
        }) ?? String(v)
      );
    } catch (e) {
      return e === TOO_LONG ? "[too large to show]" : String(v);
    }
  };
  const post = (line) => {
    try {
      handler.postMessage(JSON.stringify({ ...line, url: location.href, ts: Date.now() }));
    } catch {
      // A message the page made unsendable is no reason to break its console.
    }
  };
  let since = 0;
  let sent = 0;
  let held = null;
  const send = (level, args, stack) => {
    const now = Date.now();
    if (now - since >= 1000) {
      since = now;
      sent = 0;
    }
    if (sent >= PER_SECOND) {
      if (!held) {
        held = { count: 0, level: "warn" };
        later(() => {
          post({ level: held.level, msg: `… ${held.count} more suppressed`, stack: "" });
          held = null;
        }, 1000);
      }
      held.count++;
      if (level === "error") held.level = "error";
      return;
    }
    sent++;
    try {
      const error = args.find((a) => a instanceof Error);
      post({ level, msg: cut(args.map(text).join(" ")), stack: cut(stack ?? error?.stack ?? "") });
    } catch {
      // As above.
    }
  };
  for (const level of ["error", "warn"]) {
    const original = console[level];
    console[level] = function (...args) {
      send(level, args);
      return original.apply(this, args);
    };
  }
  window.addEventListener("error", (e) => send("error", [e.error ?? e.message], e.error?.stack ?? `${e.filename}:${e.lineno}:${e.colno}`));
  window.addEventListener("unhandledrejection", (e) => send("error", ["Unhandled rejection:", e.reason]));
})();
