// The page's errors and warnings, for the browser tab's console badge (browser/console.rs). Runs
// in the page's own world at document start, so it sees what the page logs; the page could post
// to gvConsole itself, which only adds lines to its own console.
(() => {
  const handler = window.webkit?.messageHandlers?.gvConsole;
  if (!handler || window.__gvConsole) return;
  window.__gvConsole = true;
  const MAX = 2048;
  const cut = (s) => (s.length > MAX ? `${s.slice(0, MAX)}…` : s);
  const text = (v) => {
    if (v instanceof Error) return `${v.name}: ${v.message}`;
    if (typeof v === "string") return v;
    try {
      return JSON.stringify(v) ?? String(v);
    } catch {
      return String(v);
    }
  };
  const send = (level, args, stack) => {
    try {
      const error = args.find((a) => a instanceof Error);
      handler.postMessage(
        JSON.stringify({ level, msg: cut(args.map(text).join(" ")), stack: cut(stack ?? error?.stack ?? ""), url: location.href, ts: Date.now() }),
      );
    } catch {
      // A message the page made unsendable is no reason to break its console.
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
