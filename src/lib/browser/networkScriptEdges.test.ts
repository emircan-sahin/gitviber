import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// network.js on the hard cases: odd bodies, aborts, streams the page lets go of, reused
// XMLHttpRequests, busy sockets, cuts at a surrogate pair, and a dev page busy for a quarter of
// an hour. Tests named "BUG:" are marked todo: they show a fault the script still has.
const source = readFileSync(new URL("../../../src-tauri/src/browser/scripts/network.js", import.meta.url), "utf8");

type Rec = Record<string, any>;
type Listener = (e?: any) => void;
const enc = (s: string) => new TextEncoder().encode(s);

/** As the spec has it: open() on a request in flight ends it without any event. */
class FakeXhr {
  readyState = 0;
  status = 0;
  statusText = "";
  responseType = "";
  responseText = "";
  response: unknown = "";
  responseURL = "";
  headers = "";
  sending = false;
  listeners: Record<string, Listener[]> = {};
  addEventListener(type: string, fn: Listener) {
    (this.listeners[type] ??= []).push(fn);
  }
  open(method: string, url: string, async = true) {
    if (!/^[A-Za-z]+$/.test(method)) throw new SyntaxError("bad method");
    // Whatever the last request got is gone.
    Object.assign(this, { readyState: 1, sending: false, async, status: 0, statusText: "", responseText: "", response: "", headers: "" });
    this.responseURL = new URL(url, "http://localhost:5173/app/").href;
  }
  setRequestHeader() {}
  send() {
    if (this.readyState !== 1 || this.sending) throw new Error("InvalidStateError");
    this.sending = true;
    // A synchronous request has its answer, and its events, before send returns.
    if (!(this as any).async) this.finish(200, "sync");
  }
  getAllResponseHeaders() {
    return this.headers;
  }
  getResponseHeader(name: string) {
    const line = this.headers.split("\r\n").find((l) => l.toLowerCase().startsWith(`${name}:`));
    return line ? line.slice(name.length + 1).trim() : null;
  }
  fire(type: string, e: unknown = {}) {
    for (const fn of this.listeners[type] ?? []) fn(e);
  }
  finish(status: number, body: string, headers = "content-type: application/json\r\n", response: unknown = body) {
    Object.assign(this, { readyState: 4, sending: false, status, statusText: "OK", responseText: body, response, headers });
    this.fire("load");
    this.fire("loadend", { loaded: body.length });
  }
}

class FakeSocket extends EventTarget {
  static readonly OPEN = 1;
  url: string;
  protocols: unknown;
  binaryType = "blob";
  sent: unknown[] = [];
  constructor(url: string, protocols?: unknown) {
    super();
    this.url = new URL(url, "http://localhost:5173/").href.replace(/^http/, "ws");
    this.protocols = protocols;
  }
  send(data: unknown) {
    this.sent.push(data);
  }
}

/** A page with network.js on it; `clock` set, Date.now is the test's. */
function page(answer: (input: unknown, init?: any) => Promise<Response> = async () => new Response(""), clock?: { ms: number }) {
  const posts: Rec[][] = [];
  const raw: string[] = [];
  const timers: (() => void)[] = [];
  let observed: ((list: { getEntries: () => unknown[] }) => void) | null = null;
  class Observer {
    static supportedEntryTypes = ["navigation", "resource"];
    constructor(fn: typeof observed) {
      observed = fn;
    }
    observe() {}
  }
  const window: Rec = {
    webkit: {
      messageHandlers: {
        gvNetwork: {
          postMessage: (s: string) => {
            raw.push(s);
            posts.push(JSON.parse(s));
          },
        },
      },
    },
    fetch: answer,
    XMLHttpRequest: class extends FakeXhr {},
    WebSocket: class extends FakeSocket {},
  };
  const date = clock ? { now: () => clock.ms } : Date;
  const run = new Function("window", "document", "PerformanceObserver", "setTimeout", "Date", source);
  run(window, { baseURI: "http://localhost:5173/app/" }, Observer, (fn: () => void) => timers.push(fn), date);
  const entries = () => {
    const all = new Map<string, Rec>();
    for (const u of posts.flat()) {
      if (!u.key) continue;
      const e = all.get(u.key) ?? { frames: [] };
      const { frame, ...rest } = u;
      Object.assign(e, rest);
      if (frame) e.frames.push(frame);
      all.set(u.key, e);
    }
    return [...all.values()];
  };
  return { window, posts, raw, entries, timers, resources: (list: unknown[]) => observed?.({ getEntries: () => list }) };
}

const settle = async (n = 10) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

/** A stream the test feeds, and whether its source was cancelled. */
function feed() {
  const s = { push: (_: string) => {}, close: () => {}, cancelled: false, stream: null as unknown as ReadableStream<Uint8Array> };
  s.stream = new ReadableStream<Uint8Array>({
    start(c) {
      s.push = (t) => c.enqueue(enc(t));
      s.close = () => c.close();
    },
    cancel() {
      s.cancelled = true;
    },
  });
  return s;
}

test("every kind of body reaches the page's own fetch byte for byte, and is described", async () => {
  const got: string[] = [];
  const p = page(async (input, init) => {
    const url = input instanceof Request ? input : new URL(String(input), "http://localhost:5173/");
    got.push(await new Request(url, init).text());
    return new Response("ok");
  });
  const form = new FormData();
  form.append("a", "1");
  const bodies: [unknown, string][] = [
    ["plain", "plain"],
    [new URLSearchParams({ q: "a b" }), "q=a+b"],
    [new Blob(["blob!"], { type: "text/plain" }), "blob!"],
    [enc("bytes").buffer, "bytes"],
    [new DataView(enc("view").buffer), "view"],
  ];
  for (const [body] of bodies) await p.window.fetch("/b", { method: "POST", body });
  await p.window.fetch(new Request("http://localhost:5173/r", { method: "PUT", body: new Blob(["in a request"]) }));
  const stream = new ReadableStream({
    start(c) {
      c.enqueue(enc("streamed"));
      c.close();
    },
  });
  await p.window.fetch("/s", { method: "POST", body: stream, duplex: "half" });
  await p.window.fetch("/f", { method: "POST", body: form });
  await settle();
  assert.deepEqual(got.slice(0, 7), [...bodies.map(([, t]) => t), "in a request", "streamed"]);
  assert.match(got[7], /name="a"\r\n\r\n1\r\n/);
  const described = p.entries().map((e) => e.requestBody?.text ?? e.requestBody?.type);
  assert.deepEqual(described, ["plain", "q=a+b", "text/plain", "binary", "binary", "in a request", "stream", "a=1"]);
});

test("an abort before the answer, or while the page reads, reaches the page as ever", async () => {
  const p = page(
    (_, init) =>
      new Promise((resolve, reject) => {
        const signal: AbortSignal = init.signal;
        const s = feed();
        signal.addEventListener("abort", () => reject(signal.reason));
        if (init.answer) {
          signal.addEventListener("abort", () => s.stream.cancel(signal.reason).catch(() => {}));
          resolve(new Response(s.stream, { headers: { "content-type": "text/plain" } }));
          s.push("part");
        }
      }),
  );
  const before = new AbortController();
  const one = p.window.fetch("/slow", { signal: before.signal });
  before.abort();
  await assert.rejects(one, (e: any) => e.name === "AbortError");
  const after = new AbortController();
  const response: Response = await p.window.fetch("/stream", { signal: after.signal, answer: true });
  const reader = response.body!.getReader();
  assert.equal(new TextDecoder().decode((await reader.read()).value), "part");
  after.abort();
  await settle();
  const [first, second] = p.entries();
  assert.equal(first.error, "aborted");
  assert.equal(second.status, 200);
});

test("a body the page reads at once, a big one and one already used: the page's bytes are its own", async () => {
  const big = new Uint8Array(12 << 20).map((_, i) => i % 251);
  const used = new Response("used");
  await used.text();
  const answers = [() => new Response(big, { headers: { "content-type": "application/octet-stream" } }), () => used];
  const p = page(async () => answers.shift()!());
  const response: Response = await p.window.fetch("/big.bin");
  const bytes = new Uint8Array(await response.arrayBuffer());
  assert.equal(bytes.length, big.length);
  assert.equal(bytes[big.length - 1], big[big.length - 1]);
  const again: Response = await p.window.fetch("/used");
  assert.equal(again, used, "a response that can't be copied still reaches the page");
  await settle(50);
  const body = p.entries()[0].responseBody;
  assert.deepEqual([body.size, body.cut, body.text], [undefined, true, undefined], "past READ it's left to the page");
});

test("a redirect names where it ended", async () => {
  const p = page(async () => {
    const r = new Response("x");
    Object.defineProperties(r, { redirected: { value: true }, url: { value: "http://localhost:5173/login" } });
    return r;
  });
  await p.window.fetch("/account");
  await settle();
  assert.equal(p.entries()[0].to, "http://localhost:5173/login");
});

test(
  "BUG: the page cancelling an endless body of its own still lets go of the connection",
  { todo: "network.js's copy keeps reading: the source is never cancelled and the page's cancel() never settles" },
  async () => {
    for (const type of ["application/x-ndjson", "text/event-stream"]) {
      const s = feed();
      const p = page(async () => new Response(s.stream, { headers: { "content-type": type } }));
      const response: Response = await p.window.fetch("/chat/stream");
      s.push('{"token":"hi"}\n');
      const reader = response.body!.getReader();
      await reader.read();
      let settled = false;
      reader.cancel().then(() => (settled = true));
      await settle();
      assert.equal(settled, true, `${type}: the page's cancel() settles`);
      assert.equal(s.cancelled, true, `${type}: the server's stream is let go`);
    }
  },
);

test("XMLHttpRequest bodies of each responseType, and a synchronous one", () => {
  const p = page();
  const answers: [string, unknown, string][] = [
    ["arraybuffer", new ArrayBuffer(10), "application/octet-stream"],
    ["blob", new Blob(["abc"]), "image/png"],
    ["json", { a: [1, 2] }, "application/json"],
    ["json", null, "application/json"],
    ["document", {}, "text/html"],
  ];
  for (const [type, response, ct] of answers) {
    const r = new p.window.XMLHttpRequest();
    r.open("GET", `/x/${type}`);
    r.responseType = type;
    r.send();
    r.finish(200, "", `content-type: ${ct}\r\n`, response);
  }
  const sync = new p.window.XMLHttpRequest();
  sync.open("GET", "/sync", false);
  sync.send();
  const bodies = p.entries().map((e) => [e.responseBody?.text ?? null, e.responseBody?.type, e.status]);
  assert.deepEqual(bodies, [
    [null, "application/octet-stream", 200],
    [null, "image/png", 200],
    ['{"a":[1,2]}', "application/json", 200],
    ["null", "application/json", 200],
    [null, "text/html", 200],
    ["sync", "application/json", 200],
  ]);
});

test(
  "BUG: an XMLHttpRequest opened again while in flight logs its new request, not the old one's",
  { todo: "open() ends a request in flight without loadend: `sending` keeps the old entry, the new send is skipped" },
  () => {
    const p = page();
    const r = new p.window.XMLHttpRequest();
    r.open("GET", "/search?q=a");
    r.send();
    // A typeahead reusing its object: the spec's open() drops the first request without an event.
    r.open("GET", "/search?q=ab");
    r.send();
    r.finish(200, '["ab"]');
    const entries = p.entries();
    assert.equal(entries.length, 2, "both requests are logged");
    assert.equal(entries[0].status, undefined, "the first never got an answer");
    assert.equal(entries[1].url, "http://localhost:5173/search?q=ab");
    assert.equal(entries[1].status, 200);
  },
);

test(
  "BUG: a poll that opens its XMLHttpRequest again from its own load handler is logged each time",
  { todo: "loadend comes after the page's load handler re-opened it: the first entry reads the reset object, the next is never logged" },
  () => {
    const p = page();
    const r = new p.window.XMLHttpRequest();
    let polls = 0;
    const poll = () => {
      r.open("GET", `/api/poll?n=${++polls}`);
      r.send();
    };
    // The classic long poll: handle the answer, ask again on the same object.
    r.addEventListener("load", () => polls < 3 && poll());
    poll();
    r.finish(200, '{"n":1}');
    r.finish(200, '{"n":2}');
    r.finish(200, '{"n":3}');
    const entries = p.entries();
    assert.deepEqual(
      entries.map((e) => [new URL(e.url).search, e.status, e.responseBody?.text]),
      [
        ["?n=1", 200, '{"n":1}'],
        ["?n=2", 200, '{"n":2}'],
        ["?n=3", 200, '{"n":3}'],
      ],
    );
  },
);

test("a WebSocket subclass, its protocols, binary frames and a burst all behave as the page's own", () => {
  const p = page();
  const Base: typeof FakeSocket = p.window.WebSocket;
  class Live extends Base {
    hello() {
      this.send("hi");
    }
  }
  const ws = new Live("/hmr", ["vite-hmr"]);
  assert.ok(ws instanceof Live && ws instanceof p.window.WebSocket);
  assert.deepEqual(ws.protocols, ["vite-hmr"]);
  assert.equal(p.window.WebSocket.prototype.constructor, p.window.WebSocket);
  assert.equal(p.window.WebSocket.length, 1);
  ws.binaryType = "arraybuffer";
  ws.hello();
  ws.dispatchEvent(new MessageEvent("message", { data: new ArrayBuffer(64) }));
  ws.dispatchEvent(new MessageEvent("message", { data: new Blob(["12345"]) }));
  for (let i = 0; i < 500; i++) ws.dispatchEvent(new MessageEvent("message", { data: `{"type":"update","n":${i}}` }));
  const [e] = p.entries();
  assert.deepEqual(ws.sent, ["hi"]);
  assert.deepEqual(
    e.frames.slice(0, 3).map((f: Rec) => [f.n, f.out, f.size]),
    [
      [1, true, 2],
      [2, false, 64],
      [3, false, 5],
    ],
  );
  // One second's room is a hundred posts, the socket's own start among them.
  assert.equal(e.frames.length, 99);
  assert.equal(e.frames.at(-1).n, 99);
});

test(
  "BUG: a chatty socket doesn't keep the page's requests out of the log",
  { todo: "frames spend the same hundred-a-second room as requests, so a ticker socket hides every fetch" },
  async () => {
    const p = page();
    const ws = new p.window.WebSocket("/ticker");
    for (let i = 0; i < 150; i++) ws.dispatchEvent(new MessageEvent("message", { data: `tick ${i}` }));
    await p.window.fetch("/api/save", { method: "POST", body: "{}" });
    await settle();
    assert.ok(
      p.entries().some((e) => e.url.endsWith("/api/save")),
      "the save is logged",
    );
  },
);

test("the limit is a hundred a second, counted again each second, every skip reported once", async () => {
  const clock = { ms: 5_000_000 };
  const p = page(undefined, clock);
  for (let i = 0; i < 130; i++) void p.window.fetch(`/a/${i}`);
  clock.ms += 999;
  void p.window.fetch("/still-full");
  clock.ms += 1;
  for (let i = 0; i < 120; i++) void p.window.fetch(`/b/${i}`);
  await settle();
  assert.equal(p.entries().length, 200);
  assert.equal(p.timers.length, 1, "one report while skips are pending");
  p.timers[0]();
  assert.deepEqual(p.posts.at(-1), [{ skipped: 51 }]);
  clock.ms += 1000;
  for (let i = 0; i < 101; i++) void p.window.fetch(`/c/${i}`);
  assert.equal(p.timers.length, 2);
  p.timers[1]();
  assert.deepEqual(p.posts.at(-1), [{ skipped: 1 }]);
});

test("cuts never split a surrogate pair, whatever falls on the edge", async () => {
  const p = page(async (_, init) => new Response(init?.body ?? ""));
  for (const pad of [0, 1]) {
    const body = "a".repeat(8190 + pad) + "😀".repeat(10);
    await p.window.fetch("/emoji", { method: "POST", body, headers: { "content-type": "text/plain" } });
  }
  const ws = new p.window.WebSocket("/e");
  for (const pad of [0, 1]) ws.dispatchEvent(new MessageEvent("message", { data: "b".repeat(1022 + pad) + "😀😀" }));
  ws.dispatchEvent(Object.assign(new Event("close"), { code: 1000, reason: "c".repeat(254) + "😀😀", wasClean: true }));
  await settle();
  for (const s of p.raw) assert.ok(!/\\ud[89ab][0-9a-f]{2}(?!\\ud[c-f])/i.test(s), "no lone surrogate in a post");
  const texts = p
    .entries()
    .flatMap((e) => [e.requestBody?.text, e.responseBody?.text, e.close?.reason, ...e.frames.map((f: Rec) => f.text)])
    .filter((t): t is string => typeof t === "string");
  assert.ok(texts.length >= 7);
  const lone = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/;
  for (const t of texts) assert.ok(!lone.test(t), JSON.stringify(t.slice(-4)));
});

test(
  "BUG: a page's own lone surrogate doesn't cost it its entry",
  { todo: "posted as \\ud83d, which the app's JSON parser refuses: the whole start of the request is lost" },
  async () => {
    const p = page();
    // A body cut mid-emoji by the page's own code, as happens.
    await p.window.fetch("/save", { method: "POST", body: '{"note":"hi \uD83D"}' });
    for (const s of p.raw) assert.ok(!/\\ud[89ab][0-9a-f]{2}(?!\\ud[c-f])/i.test(s), s);
  },
);

test(
  "BUG: a fetch of a long data: URL is posted with its address cut",
  { todo: "the address goes out whole; past 1 MiB the app drops the post, and with it the entry" },
  async () => {
    const p = page();
    await p.window.fetch(`data:image/png;base64,${"A".repeat(1_500_000)}`);
    for (const s of p.raw) assert.ok(s.length < 1 << 20, `a post of ${s.length} characters`);
  },
);

test("a busy dev page for fifteen minutes: bounded posts, the page's answers intact, nothing thrown", async () => {
  const clock = { ms: 1_700_000_000_000 };
  let served = 0;
  const p = page(async (input) => {
    served++;
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("/events")) {
      const s = feed();
      s.push("data: hello\n\n");
      return new Response(s.stream, { headers: { "content-type": "text/event-stream" } });
    }
    const headers = Object.fromEntries(Array.from({ length: 120 }, (_, i) => [`x-h${i}`, "v".repeat(3000)]));
    return new Response(JSON.stringify({ url, items: Array.from({ length: 50 }, (_, i) => ({ i, name: "é😀".repeat(20) })) }), {
      headers: { "content-type": "application/json", ...headers },
    });
  }, clock);
  // The first load: the page and its modules, by the hundred.
  const module = (i: number, t: number) => ({
    entryType: "resource",
    initiatorType: i % 7 ? "script" : "css",
    name: `http://localhost:5173/src/components/Thing${i}.tsx?t=${t}&import`,
    startTime: t,
    duration: 4,
    transferSize: 0,
    encodedBodySize: 1200,
    responseStatus: 200,
  });
  p.resources([
    { entryType: "navigation", initiatorType: "navigation", name: "http://localhost:5173/", startTime: 0, duration: 80, transferSize: 900 },
    ...Array.from({ length: 640 }, (_, i) => module(i, 10 + i)),
  ]);
  const hmr = new p.window.WebSocket("/?token=t-1", "vite-hmr");
  hmr.dispatchEvent(new Event("open"));
  const sse: Response = await p.window.fetch("/events");
  const checks: Promise<void>[] = [];
  for (let s = 0; s < 900; s++) {
    clock.ms += 1000;
    // A poll a second, the page reading every answer.
    checks.push(
      p.window.fetch(`/api/poll?n=${s}`, { headers: { authorization: "Bearer t" } }).then(async (r: Response) => {
        assert.equal((await r.json()).url, `/api/poll?n=${s}`);
      }),
    );
    if (s % 5 === 0) {
      const x = new p.window.XMLHttpRequest();
      x.open("POST", "/api/metrics");
      x.send(JSON.stringify({ s }));
      x.finish(204, "");
    }
    if (s % 30 === 0) {
      // A save: an HMR update and the modules it loads again.
      for (let k = 0; k < 20; k++) hmr.dispatchEvent(new MessageEvent("message", { data: `{"type":"update","n":${s}}` }));
      p.resources(Array.from({ length: 40 }, (_, i) => module(i, s * 1000 + i)));
      hmr.send('{"type":"ping"}');
    }
    if (s % 100 === 0) await settle();
  }
  await Promise.all(checks);
  await settle(30);
  assert.equal(served, 901);
  const all = p.entries();
  assert.equal(all.filter((e) => e.kind === "fetch").length, 901);
  assert.equal(all.filter((e) => e.kind === "xhr").length, 180);
  assert.equal(all.filter((e) => e.kind === "resource").length, 641 + 30 * 40);
  const ws = all.find((e) => e.kind === "ws")!;
  assert.equal(ws.frames.length, 30 * 21);
  for (const s of p.raw) assert.ok(s.length < 1 << 20, `a post of ${s.length} characters`);
  const poll = all.find((e) => e.url.endsWith("/api/poll?n=899"))!;
  assert.equal(poll.responseHeaders.length, 100, "headers kept to a hundred");
  assert.ok(poll.responseHeaders.every(([, v]: string[]) => v.length <= 2048));
  assert.ok(poll.responseBody.text.length <= 8192 && poll.responseBody.size > 0);
  const events = all.find((e) => e.url.endsWith("/events"))!;
  assert.equal(events.duration, undefined, "the event stream is still open");
  assert.equal(p.timers.length, 0, "nothing skipped at one request a second");
  // Not awaited: the copy still reading keeps the cancel from settling (the BUG above).
  void sse.body!.cancel().catch(() => {});
});
