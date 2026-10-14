// What the page sends and loads, for `gitviber browser network` (browser/network.rs): its fetch
// and XMLHttpRequest calls with headers and bodies as the page sees them, its WebSockets with
// their frames, and everything else it loads as resource timing has it. Runs in an agent tab's
// own world at document start; the page could post to gvNetwork itself, which only adds
// requests to its own log.
(() => {
  const handler = window.webkit?.messageHandlers?.gvNetwork;
  if (!handler || window.__gvNetwork) return;
  window.__gvNetwork = true;
  /** Characters of a body kept, and of a WebSocket frame. */
  const BODY = 8192;
  const FRAME = 1024;
  /** Bytes of a fetch's body read for its size and its end; past this it's left to the page. */
  const READ = 8 << 20;
  const HEADERS = 100;
  // A page requesting in a loop would otherwise post (and the app parse) every one of them.
  const PER_SECOND = 100;
  const later = setTimeout;
  const stringify = JSON.stringify;
  const clock = performance;
  const now = () => clock.timeOrigin + clock.now();
  // To `max` UTF-16 units with the ellipsis, never splitting a pair (the app's JSON parser
  // refuses half of one).
  const cut = (s, max) => {
    if (s.length <= max) return s;
    const end = /[\uD800-\uDBFF]/.test(s[max - 2]) ? max - 2 : max - 1;
    return `${s.slice(0, end)}…`;
  };
  const bytes = (s) => {
    let n = 0;
    for (let i = 0; i < s.length; i++) {
      const c = s.charCodeAt(i);
      n += c < 0x80 ? 1 : c < 0x800 ? 2 : c >= 0xd800 && c < 0xdc00 ? (i++, 4) : 3;
    }
    return n;
  };
  const post = (updates) => {
    try {
      handler.postMessage(stringify(updates));
    } catch {
      // A request the page made unsendable is no reason to break its request.
    }
  };

  // Each entry posts once as it starts and again as it ends, under a key of this document's.
  const doc = Math.random().toString(36).slice(2, 10);
  let count = 0;
  let since = 0;
  let sent = 0;
  let skipped = 0;
  const room = (counted = true) => {
    const t = Date.now();
    if (t - since >= 1000) {
      since = t;
      sent = 0;
    }
    if (sent < PER_SECOND) {
      sent++;
      return true;
    }
    if (counted && !skipped++) {
      later(() => {
        post([{ skipped }]);
        skipped = 0;
      }, 1000);
    }
    return false;
  };
  /** A new entry, or null past the limit. */
  const begin = (fields) => {
    if (!room()) return null;
    const rec = { key: `${doc}:${++count}`, start: now() };
    post([{ ...fields, key: rec.key, start: rec.start }]);
    return rec;
  };
  const update = (rec, fields) => rec && post([{ ...fields, key: rec.key }]);
  const ended = (rec) => now() - rec.start;

  const full = (url) => {
    try {
      return new URL(url, document.baseURI).href;
    } catch {
      return String(url);
    }
  };
  // Only what reads without side effects: a page's own iterator would be used up before its
  // request saw it.
  const pairs = (h) => {
    const out = [];
    const proto = h && typeof h === "object" ? Object.getPrototypeOf(h) : undefined;
    if (!(h instanceof Headers || Array.isArray(h) || proto === Object.prototype || proto === null)) return out;
    try {
      for (const [name, value] of new Headers(h)) {
        if (out.length === HEADERS) break;
        out.push([name, cut(value, 2048)]);
      }
    } catch {
      // Headers the request itself refuses.
    }
    return out;
  };
  const lines = (raw) =>
    raw
      .split(/\r?\n/)
      .filter((l) => l.includes(":"))
      .slice(0, HEADERS)
      .map((l) => {
        const at = l.indexOf(":");
        return [l.slice(0, at).trim().toLowerCase(), cut(l.slice(at + 1).trim(), 2048)];
      });
  const text = (s, type) => ({ text: cut(s, BODY), size: bytes(s), cut: s.length > BODY, ...(type && { type }) });
  const TOO_LONG = {};
  const json = (v) => {
    let size = 0;
    try {
      // Stops walking a huge object once what it has written can't fit anyway.
      return stringify(v, (key, x) => {
        size += key.length + (typeof x === "string" ? x.length : 8);
        if (size > BODY) throw TOO_LONG;
        return x;
      });
    } catch {
      return null;
    }
  };
  // A body the page sent, as text when it is some, else what it is: never one read from a
  // stream, and never the page's own object turned into a string.
  const sentBody = (body) => {
    if (body == null) return undefined;
    if (typeof body !== "object") return text(String(body));
    if (body instanceof URLSearchParams) return text(String(body), "application/x-www-form-urlencoded");
    if (body instanceof FormData) {
      const parts = [];
      for (const [name, value] of body) {
        parts.push(typeof value === "string" ? `${name}=${value}` : `${name}=[file ${value.name}, ${value.size} bytes]`);
      }
      return text(parts.join("&"), "multipart/form-data");
    }
    if (body instanceof Blob) return { type: body.type || "blob", size: body.size };
    if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return { type: "binary", size: body.byteLength };
    return { type: body instanceof ReadableStream ? "stream" : "object" };
  };
  const TEXT = /^text\/|json|xml|javascript|ecmascript|urlencoded|graphql/i;
  // A body as it streams in, on a copy of the page's: text kept to BODY, counted to its end for
  // its size, but never past READ, nor an event stream past what's kept, which may never end.
  const read = async (stream, type) => {
    const reader = stream.getReader();
    const textual = !type || TEXT.test(type);
    const endless = /event-stream/i.test(type);
    const decoder = new TextDecoder();
    let kept = "";
    let size = 0;
    let end = false;
    let error;
    try {
      while (size <= READ && !(endless && kept.length > BODY)) {
        const { done, value } = await reader.read();
        if (done) {
          end = true;
          break;
        }
        size += value.byteLength;
        if (textual && kept.length <= BODY) kept += decoder.decode(value, { stream: true });
      }
    } catch (e) {
      error = e?.name === "AbortError" ? "aborted" : "failed";
    }
    if (!end && !error) reader.cancel().catch(() => {});
    const body = { type: type || undefined, size: end ? size : undefined, cut: !end || kept.length > BODY };
    if (textual) body.text = cut(kept, BODY);
    return { body, end, error };
  };
  const failure = (e) => (e?.name === "AbortError" ? "aborted" : "failed");

  const fetch0 = window.fetch;
  if (typeof fetch0 === "function") {
    const answered = (rec, response) => {
      const type = response.headers.get("content-type") ?? "";
      update(rec, {
        status: response.status,
        statusText: response.statusText,
        to: response.redirected ? response.url : undefined,
        responseHeaders: pairs(response.headers),
      });
      if (!response.body) return update(rec, { duration: ended(rec) });
      // A copy, read alongside the page's: its own body stays unread and streams as it would.
      read(response.clone().body, type).then(({ body, end, error }) =>
        update(rec, { responseBody: body, duration: end || error ? ended(rec) : undefined, error }),
      );
    };
    // Named and sized as WebKit's own.
    window.fetch = function fetch(input) {
      const init = arguments[1];
      let rec = null;
      try {
        const request = input instanceof Request ? input : null;
        const headers = init?.headers ?? request?.headers;
        const has = init && "body" in init;
        rec = begin({
          kind: "fetch",
          method: String(init?.method ?? request?.method ?? "GET").toUpperCase(),
          url: full(request ? request.url : input),
          requestHeaders: pairs(headers),
          requestBody: has ? sentBody(init.body) : undefined,
        });
        // A Request's own body, from a copy made before the page's request uses it up.
        if (rec && !has && request?.body && !request.bodyUsed) {
          const copy = request.clone();
          read(copy.body, request.headers.get("content-type") ?? "").then(({ body }) =>
            update(rec, { requestBody: body }),
          );
        }
      } catch {
        // An odd argument is the page's own fetch's to refuse.
      }
      // A promise of its own, passing the answer on: one the page never handles still reports
      // as unhandled.
      const answer = fetch0.apply(this, arguments);
      if (!rec) return answer;
      return answer.then(
        (response) => {
          try {
            answered(rec, response);
          } catch {
            // As above.
          }
          return response;
        },
        (error) => {
          update(rec, { error: failure(error), duration: ended(rec) });
          throw error;
        },
      );
    };
  }

  const XHR = window.XMLHttpRequest;
  if (typeof XHR === "function") {
    // Each object's request now: one object may be opened again and again.
    const opened = new WeakMap();
    const sending = new WeakMap();
    const watched = new WeakSet();
    const proto = XHR.prototype;
    const open0 = proto.open;
    const header0 = proto.setRequestHeader;
    const send0 = proto.send;
    const xhrBody = (xhr, size) => {
      const type = xhr.getResponseHeader("content-type") ?? "";
      switch (xhr.responseType) {
        case "":
        case "text": {
          const s = xhr.responseText;
          return { ...text(s, type), size };
        }
        case "json": {
          const s = json(xhr.response);
          return s === null ? { type, size, cut: true } : { ...text(s, type), size };
        }
        default:
          return { type: type || xhr.responseType, size };
      }
    };
    const watch = (xhr) => {
      if (watched.has(xhr)) return;
      watched.add(xhr);
      let how = null;
      for (const type of ["abort", "error", "timeout"]) xhr.addEventListener(type, () => (how = type));
      xhr.addEventListener("loadend", (e) => {
        const rec = sending.get(xhr);
        sending.delete(xhr);
        const failed = how;
        how = null;
        if (!rec) return;
        try {
          const fields = { duration: ended(rec) };
          if (failed) fields.error = { abort: "aborted", timeout: "timeout" }[failed] ?? "failed";
          else {
            fields.status = xhr.status;
            fields.statusText = xhr.statusText;
            fields.responseHeaders = lines(xhr.getAllResponseHeaders());
            if (xhr.responseURL && xhr.responseURL !== rec.url) fields.to = xhr.responseURL;
            fields.responseBody = xhrBody(xhr, e.loaded);
          }
          update(rec, fields);
        } catch {
          update(rec, { duration: ended(rec) });
        }
      });
    };
    proto.open = function open(method, url) {
      const result = open0.apply(this, arguments);
      try {
        opened.set(this, { method: String(method).toUpperCase(), url: full(url), headers: [] });
      } catch {
        opened.delete(this);
      }
      return result;
    };
    proto.setRequestHeader = function setRequestHeader(name, value) {
      const result = header0.apply(this, arguments);
      const o = opened.get(this);
      if (o && o.headers.length < HEADERS) o.headers.push([String(name).toLowerCase(), cut(String(value), 2048)]);
      return result;
    };
    proto.send = function send() {
      const o = opened.get(this);
      let rec = null;
      // Sent once per open; a second send is the page's own error.
      if (o && !sending.has(this) && this.readyState === 1) {
        try {
          rec = begin({ kind: "xhr", method: o.method, url: o.url, requestHeaders: o.headers, requestBody: sentBody(arguments[0]) });
          if (rec) {
            rec.url = o.url;
            sending.set(this, rec);
            watch(this);
          }
        } catch {
          // As fetch's.
        }
      }
      try {
        return send0.apply(this, arguments);
      } catch (e) {
        if (rec && sending.get(this) === rec) {
          sending.delete(this);
          update(rec, { error: "failed", duration: ended(rec) });
        }
        throw e;
      }
    };
  }

  const WS = window.WebSocket;
  if (typeof WS === "function") {
    const sockets = new WeakMap();
    const frame = (ws, out, data) => {
      const s = sockets.get(ws);
      if (!s) return;
      s.n++;
      // Past the limit a frame isn't posted: its number tells the gap.
      if (!room(false)) return;
      const f = { n: s.n, out, at: ended(s.rec) };
      if (typeof data === "string") Object.assign(f, { text: cut(data, FRAME), size: bytes(data) });
      else f.size = data?.size ?? data?.byteLength ?? 0;
      update(s.rec, { frame: f });
    };
    const send0 = WS.prototype.send;
    WS.prototype.send = function send(data) {
      const result = send0.apply(this, arguments);
      try {
        frame(this, true, data);
      } catch {
        // As fetch's.
      }
      return result;
    };
    // The page's own class still: the same prototype, statics and errors.
    const Socket = function WebSocket(url) {
      if (!new.target) return WS.apply(this, arguments);
      const ws = Reflect.construct(WS, arguments, new.target);
      try {
        const rec = begin({ kind: "ws", method: "GET", url: ws.url });
        if (rec) {
          sockets.set(ws, { rec, n: 0 });
          ws.addEventListener("open", () => update(rec, { status: 101, statusText: "Switching Protocols" }));
          ws.addEventListener("message", (e) => frame(ws, false, e.data));
          ws.addEventListener("error", () => update(rec, { error: "failed" }));
          ws.addEventListener("close", (e) =>
            update(rec, { close: { code: e.code, reason: cut(e.reason, 256), clean: e.wasClean }, duration: ended(rec) }),
          );
        }
      } catch {
        // As fetch's.
      }
      return ws;
    };
    try {
      Socket.prototype = WS.prototype;
      Object.setPrototypeOf(Socket, WS);
      WS.prototype.constructor = Socket;
      window.WebSocket = Socket;
    } catch {
      // A page world that won't take it keeps its own.
    }
  }

  // Everything else, as WebKit timed it: fetch's and XMLHttpRequest's own are above, in full.
  // The ones before this ran come buffered.
  if (typeof PerformanceObserver === "function") {
    const types = PerformanceObserver.supportedEntryTypes ?? [];
    const seen = (list) => {
      const out = [];
      for (const e of list.getEntries()) {
        if (e.initiatorType === "fetch" || e.initiatorType === "xmlhttprequest") continue;
        out.push({
          key: `${doc}:${++count}`,
          kind: "resource",
          initiator: e.entryType === "navigation" ? "navigation" : e.initiatorType,
          url: e.name,
          start: clock.timeOrigin + e.startTime,
          duration: e.duration,
          size: e.transferSize || e.encodedBodySize || undefined,
          // Where the engine has it.
          status: e.responseStatus || undefined,
        });
      }
      // One post a batch, a hundred at most: a dev server's page loads its modules by the hundred.
      for (let i = 0; i < out.length; i += 100) post(out.slice(i, i + 100));
    };
    try {
      const observer = new PerformanceObserver(seen);
      for (const type of ["navigation", "resource"]) {
        if (types.includes(type)) observer.observe({ type, buffered: true });
      }
    } catch {
      // No timing, no resources.
    }
  }
})();
