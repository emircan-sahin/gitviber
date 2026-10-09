// The page's errors, warnings and failed requests, for the browser tab's console (browser/
// console.rs). Runs in the page's own world at document start, so it sees what the page logs;
// the page could post to gvConsole itself, which only adds lines to its own console.
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
  const cut = (s, max = MAX) => {
    if (s.length <= max) return s;
    const end = /[\uD800-\uDBFF]/.test(s[max - 2]) ? max - 2 : max - 1;
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

  // Requests that failed, as `[network] GET /api/x → 500`: the address without its query, which
  // can carry a token, and never a body or a header. A 4xx is a warning; a 5xx, or no answer at
  // all, an error. One the page aborted is no failure.
  const where = (url) => {
    try {
      const u = new URL(url, location.href);
      const path = u.origin === new URL(location.href).origin ? u.pathname : `${u.origin}${u.pathname}`;
      return cut(`${path}${u.search ? "?…" : ""}`, 300);
    } catch {
      return cut(String(url).split("?")[0], 300);
    }
  };
  const failed = (method, url, status) =>
    send(status >= 500 || !status ? "error" : "warn", [`[network] ${method} ${where(url)} ${status ? `→ ${status}` : "failed"}`], "");
  const fetch0 = window.fetch;
  if (typeof fetch0 === "function") {
    window.fetch = function (input, init) {
      let request = null;
      try {
        request = [String(init?.method ?? input?.method ?? "GET").toUpperCase(), String(input?.url ?? input)];
      } catch {
        // An odd argument is the page's own fetch's to refuse.
      }
      // A promise of its own, passing the answer on: one the page never handles still reports as
      // unhandled.
      return fetch0.apply(this, arguments).then(
        (response) => {
          if (request && response.status >= 400) failed(...request, response.status);
          return response;
        },
        (error) => {
          if (request && error?.name !== "AbortError") failed(...request, 0);
          throw error;
        },
      );
    };
  }
  const XHR = window.XMLHttpRequest;
  if (typeof XHR === "function") {
    // Each object's request now, read as it ends: one object may be opened again and again.
    const requests = new WeakMap();
    const watched = new WeakSet();
    const open = XHR.prototype.open;
    XHR.prototype.open = function (method, url) {
      requests.set(this, [String(method).toUpperCase(), String(url)]);
      return open.apply(this, arguments);
    };
    const sendRequest = XHR.prototype.send;
    XHR.prototype.send = function () {
      if (!watched.has(this)) {
        watched.add(this);
        const now = () => requests.get(this) ?? ["GET", ""];
        this.addEventListener("load", () => this.status >= 400 && failed(...now(), this.status));
        this.addEventListener("error", () => failed(...now(), 0));
        this.addEventListener("timeout", () => failed(...now(), 0));
      }
      return sendRequest.apply(this, arguments);
    };
  }
})();
