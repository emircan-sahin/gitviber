// What the page sends and loads, for `gitviber browser network` (browser/network.rs): its fetch
// and XMLHttpRequest calls with headers and bodies as the page sees them, its WebSockets with
// their frames, and everything else it loads as resource timing has it. Runs in the page's own
// world at document start; the page could post to gvNetwork itself, which only adds requests to
// its own log.
(() => {
  const handler = window.webkit?.messageHandlers?.gvNetwork;
  if (!handler || window.__gvNetwork) return;
  window.__gvNetwork = true;
  // Set false from outside (page_tools.rs `release_network`) once the tab is no agent's: the
  // page stops copying its requests. A frame's own copy is only cut off from the app.
  const off = () => window.__gvNetwork === false;
  // Characters kept of a body, a frame, an address, a name (a header's, a status text, a close
  // reason), a header's value and a word (a method, an initiator): network.rs keeps the same,
  // and refuses a post past 1 MiB.
  const BODY = 8192;
  const FRAME = 1024;
  const URL_TEXT = 4096;
  const NAME = 256;
  const VALUE = 2048;
  const WORD = 32;
  const HEADERS = 100;
  // A page requesting in a loop, or a socket streaming ticks, would otherwise post (and the app
  // parse) every one of them. Frames have their own room: a busy socket hides no request.
  const PER_SECOND = 100;
  const later = setTimeout;
  const stringify = JSON.stringify;
  const clock = performance;
  const now = () => clock.timeOrigin + clock.now();
  // To `max` UTF-16 units with the ellipsis, never splitting a pair.
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
  // A lone surrogate (a page's own text cut mid-pair) as U+FFFD: the app's JSON parser refuses
  // a post with one.
  const whole = (key, v) =>
    typeof v === "string" ? v.replace(/[\uD800-\uDBFF][\uDC00-\uDFFF]|[\uD800-\uDFFF]/g, (c) => (c.length === 2 ? c : "�")) : v;
  // A frame's requests go to the page's one log, each saying which frame as it starts.
  const inFrame = window.top && window.top !== window ? window.location.origin : undefined;
  const post = (updates) => {
    if (off()) return;
    try {
      const tagged = inFrame ? updates.map((u) => (u.kind ? { ...u, inFrame } : u)) : updates;
      handler.postMessage(stringify(tagged, whole));
    } catch {
      // A request the page made unsendable is no reason to break its request.
    }
  };

  // Each entry posts once as it starts and again as it ends, under a key of this document's.
  const doc = Math.random().toString(36).slice(2, 10);
  let count = 0;
  const requests = { since: 0, sent: 0 };
  const frames = { since: 0, sent: 0 };
  const room = (r) => {
    const t = Date.now();
    if (t - r.since >= 1000) {
      r.since = t;
      r.sent = 0;
    }
    if (r.sent === PER_SECOND) return false;
    r.sent++;
    return true;
  };
  let skipped = 0;
  /** A new entry, or null past the limit, counted. */
  const begin = (fields, href) => {
    if (off()) return null;
    if (!room(requests)) {
      if (!skipped++) {
        later(() => {
          post([{ skipped }]);
          skipped = 0;
        }, 1000);
      }
      return null;
    }
    const rec = { key: `${doc}:${++count}`, start: now(), href };
    post([{ ...fields, key: rec.key, start: rec.start }]);
    return rec;
  };
  const update = (rec, fields) => rec && post([{ ...fields, key: rec.key }]);
  const ended = (rec) => now() - rec.start;
  const failure = (e) => (e?.name === "AbortError" ? "aborted" : "failed");

  // An address as the request goes out: whole, without its fragment.
  const resolved = (url) => {
    try {
      const u = new URL(url, document.baseURI);
      u.hash = "";
      return u.href;
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
        out.push([cut(name, NAME), cut(value, VALUE)]);
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
        return [cut(l.slice(0, at).trim().toLowerCase(), NAME), cut(l.slice(at + 1).trim(), VALUE)];
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

  /** The start of a body as text, read from a copy to BODY characters, then let go. */
  const readCopy = async (copy) => {
    const reader = copy.getReader();
    const decoder = new TextDecoder();
    let kept = "";
    let size = 0;
    let end = false;
    let error;
    try {
      while (kept.length <= BODY) {
        const { done, value } = await reader.read();
        if (done) {
          end = true;
          break;
        }
        size += value.byteLength;
        kept += decoder.decode(value, { stream: true });
      }
    } catch (e) {
      error = failure(e);
    }
    if (!end && !error) reader.cancel().catch(() => {});
    return { kept, size, end, error };
  };
  // A copy holds the page's connection open until it too is let go, and nothing tells it the
  // page let go: so a body that may stream on is never copied, only one of a type read whole,
  // or whose Content-Length says it ends. Left: a page stopping such a body with cancel() before
  // BODY characters came waits for them (or the end); an abort ends both at once.
  const STREAMS = /event-stream|ndjson|jsonl|multipart\/x-mixed-replace|grpc/i;
  const WHOLE = /json|xml|html|javascript|ecmascript/i;
  const TEXT = /^text\/|json|xml|javascript|ecmascript|urlencoded|graphql/i;
  const copied = (type, length) => !STREAMS.test(type) && (length ? !type || TEXT.test(type) : WHOLE.test(type));

  // A fetch's own timing entry comes once its response has ended: its full duration and size.
  // Matched to the fetch of its address that started nearest its start; a blob: or data: one
  // gets none.
  const timing = typeof PerformanceObserver === "function" && (PerformanceObserver.supportedEntryTypes ?? []).includes("resource");
  const awaiting = new Map();
  const expect = (rec) => {
    if (!timing || !/^https?:/.test(rec.href)) return;
    const list = awaiting.get(rec.href) ?? [];
    if (list.push(rec) > 20) list.shift();
    awaiting.set(rec.href, list);
    if (awaiting.size > 200) awaiting.delete(awaiting.keys().next().value);
  };
  /** `rec`, or the fetch of `href` that started nearest `start`, taken off. */
  const timed = (href, rec, start) => {
    const list = awaiting.get(href) ?? [];
    let at = rec ? list.indexOf(rec) : -1;
    if (!rec) {
      for (let i = 0, best = Infinity; i < list.length; i++) {
        const off = Math.abs(list[i].start - start);
        if (off < best) [at, best] = [i, off];
      }
    }
    const found = at < 0 ? undefined : list.splice(at, 1)[0];
    if (!list.length) awaiting.delete(href);
    return found;
  };

  const fetch0 = window.fetch;
  if (typeof fetch0 === "function") {
    const answered = (rec, response) => {
      const type = response.headers.get("content-type") ?? "";
      update(rec, {
        status: response.status,
        statusText: cut(response.statusText, NAME),
        to: response.redirected ? cut(response.url, URL_TEXT) : undefined,
        responseHeaders: pairs(response.headers),
      });
      // Until its headers, for a body not read; timing's entry gives the whole when it ends.
      if (!response.body || !copied(type, response.headers.has("content-length"))) {
        return update(rec, { duration: ended(rec), ...(response.body && { responseBody: { type: type || undefined } }) });
      }
      readCopy(response.clone().body).then(({ kept, size, end, error }) => {
        if (error) timed(rec.href, rec);
        update(rec, {
          responseBody: {
            text: cut(kept, BODY),
            type: type || undefined,
            size,
            cut: !end || kept.length > BODY,
            ...(!end && { atLeast: true }),
          },
          duration: ended(rec),
          error,
        });
      });
    };
    // Named and sized as WebKit's own.
    window.fetch = function fetch(input) {
      const init = arguments[1];
      let rec = null;
      try {
        const request = input instanceof Request ? input : null;
        const headers = init?.headers ?? request?.headers;
        const has = init && "body" in init;
        const href = resolved(request ? request.url : input);
        rec = begin(
          {
            kind: "fetch",
            method: cut(String(init?.method ?? request?.method ?? "GET").toUpperCase(), WORD),
            url: cut(href, URL_TEXT),
            requestHeaders: pairs(headers),
            requestBody: has ? sentBody(init.body) : undefined,
          },
          href,
        );
        // A Request's own body, from a copy made before the page's request uses it up.
        if (rec && !has && request?.body && !request.bodyUsed) {
          const type = request.headers.get("content-type") ?? "";
          readCopy(request.clone().body).then(({ kept, size, end }) =>
            update(rec, { requestBody: { text: cut(kept, BODY), type: type || undefined, size, cut: !end, ...(!end && { atLeast: true }) } }),
          );
        }
        if (rec) expect(rec);
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
          timed(rec.href, rec);
          update(rec, { error: failure(error), duration: ended(rec) });
          throw error;
        },
      );
    };
  }

  const XHR = window.XMLHttpRequest;
  if (typeof XHR === "function") {
    // Each object's request now: one object may be opened again and again, even from its own
    // load handler, which runs before the loadend this listens for.
    const opened = new WeakMap();
    const sending = new WeakMap();
    const stale = new WeakMap();
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
          const t = text(xhr.responseText, type);
          return size === undefined ? t : { ...t, size };
        }
        case "json": {
          const s = json(xhr.response);
          return s === null ? { type, size, cut: true } : { ...text(s, type), size };
        }
        default:
          return { type: type || xhr.responseType, size: size ?? xhr.response?.size ?? xhr.response?.byteLength };
      }
    };
    /** The object's request, ended as `how` said, or as the object stands. */
    const done = (xhr, how, size) => {
      const rec = sending.get(xhr);
      if (!rec) return;
      sending.delete(xhr);
      try {
        const fields = { duration: ended(rec) };
        const failed = how ?? (xhr.readyState !== 4 ? "abort" : xhr.status === 0 ? "error" : null);
        if (failed) fields.error = { abort: "aborted", timeout: "timeout" }[failed] ?? "failed";
        else {
          fields.status = xhr.status;
          fields.statusText = cut(xhr.statusText, NAME);
          fields.responseHeaders = lines(xhr.getAllResponseHeaders());
          if (xhr.responseURL && xhr.responseURL !== rec.href) fields.to = cut(xhr.responseURL, URL_TEXT);
          fields.responseBody = xhrBody(xhr, size);
        }
        update(rec, fields);
      } catch {
        update(rec, { duration: ended(rec) });
      }
    };
    const watch = (xhr) => {
      if (watched.has(xhr)) return;
      watched.add(xhr);
      let how = null;
      for (const type of ["abort", "error", "timeout"]) xhr.addEventListener(type, () => (how = type));
      xhr.addEventListener("loadend", (e) => {
        const failed = how;
        how = null;
        // Its request was ended by an open() in the page's own handler, before this.
        if (stale.get(xhr)) return void stale.set(xhr, stale.get(xhr) - 1);
        done(xhr, failed, e.loaded);
      });
    };
    proto.open = function open(method, url) {
      // Opened again: the request it had ends now, before open() clears what it got. One in
      // flight is dropped without an event; a finished one's loadend is still to come.
      if (sending.has(this)) {
        if (this.readyState === 4) stale.set(this, (stale.get(this) ?? 0) + 1);
        done(this);
      }
      const result = open0.apply(this, arguments);
      try {
        const href = resolved(url);
        opened.set(this, { method: cut(String(method).toUpperCase(), WORD), href, url: cut(href, URL_TEXT), headers: [] });
      } catch {
        opened.delete(this);
      }
      return result;
    };
    proto.setRequestHeader = function setRequestHeader(name, value) {
      const result = header0.apply(this, arguments);
      const o = opened.get(this);
      if (o && o.headers.length < HEADERS) o.headers.push([cut(String(name).toLowerCase(), NAME), cut(String(value), VALUE)]);
      return result;
    };
    proto.send = function send() {
      const o = opened.get(this);
      let rec = null;
      // Sent once per open; a second send is the page's own error.
      if (o && !sending.has(this) && this.readyState === 1) {
        try {
          rec = begin({ kind: "xhr", method: o.method, url: o.url, requestHeaders: o.headers, requestBody: sentBody(arguments[0]) }, o.href);
          if (rec) {
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
      if (!room(frames)) return;
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
        const rec = begin({ kind: "ws", method: "GET", url: cut(ws.url, URL_TEXT) }, ws.url);
        if (rec) {
          sockets.set(ws, { rec, n: 0 });
          ws.addEventListener("open", () => update(rec, { status: 101, statusText: "Switching Protocols" }));
          ws.addEventListener("message", (e) => frame(ws, false, e.data));
          ws.addEventListener("error", () => update(rec, { error: "failed" }));
          ws.addEventListener("close", (e) =>
            update(rec, { close: { code: e.code, reason: cut(e.reason, NAME), clean: e.wasClean }, duration: ended(rec) }),
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

  // Everything else, as WebKit timed it; fetch's and XMLHttpRequest's own are above, in full,
  // and a fetch's entry only finishes its own. The ones before this ran come buffered.
  if (timing) {
    const types = PerformanceObserver.supportedEntryTypes;
    const seen = (list) => {
      const out = [];
      for (const e of list.getEntries()) {
        if (e.initiatorType === "xmlhttprequest") continue;
        const size = e.decodedBodySize || e.encodedBodySize || undefined;
        if (e.initiatorType === "fetch") {
          const rec = timed(resolved(e.name), null, clock.timeOrigin + e.startTime);
          if (rec) out.push({ key: rec.key, duration: e.duration, size });
          continue;
        }
        out.push({
          key: `${doc}:${++count}`,
          kind: "resource",
          initiator: e.entryType === "navigation" ? "navigation" : cut(e.initiatorType, WORD),
          url: cut(e.name, URL_TEXT),
          start: clock.timeOrigin + e.startTime,
          duration: e.duration,
          size,
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
