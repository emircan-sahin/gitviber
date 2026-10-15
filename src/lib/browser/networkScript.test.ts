import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// network.js, what an agent's tab records of its page's requests: everything the page sent and
// got, while the page's own fetch, XMLHttpRequest and WebSocket behave exactly as before.
const source = readFileSync(new URL("../../../src-tauri/src/browser/scripts/network.js", import.meta.url), "utf8");

type Rec = Record<string, any>;
type Listener = (e?: any) => void;

class FakeXhr {
  readyState = 0;
  status = 0;
  statusText = "";
  responseType = "";
  responseText = "";
  response: unknown = "";
  responseURL = "";
  headers = "";
  sentHeaders: [string, string][] = [];
  listeners: Record<string, Listener[]> = {};
  static sent: FakeXhr[] = [];
  addEventListener(type: string, fn: Listener) {
    (this.listeners[type] ??= []).push(fn);
  }
  open(_method: string, url: string) {
    this.readyState = 1;
    this.responseURL = url;
  }
  setRequestHeader(name: string, value: string) {
    if (this.readyState !== 1) throw new Error("InvalidStateError");
    this.sentHeaders.push([name, value]);
  }
  send() {
    if (this.readyState !== 1) throw new Error("InvalidStateError");
    FakeXhr.sent.push(this);
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
  finish(status: number, body: string, headers = "content-type: application/json\r\n") {
    Object.assign(this, { readyState: 4, status, statusText: status === 200 ? "OK" : "Error", responseText: body, headers });
    this.fire("load");
    this.fire("loadend", { loaded: body.length });
  }
}

class FakeSocket extends EventTarget {
  static readonly OPEN = 1;
  url: string;
  sent: unknown[] = [];
  constructor(url: string) {
    super();
    this.url = new URL(url, "http://localhost:5173/").href.replace(/^http/, "ws");
  }
  send(data: unknown) {
    this.sent.push(data);
  }
}

/** A page with network.js run on it: `answer` is its own fetch. */
function page(answer: (input: unknown, init?: any) => Promise<Response> = async () => new Response(""), own: Rec = {}) {
  const posts: Rec[][] = [];
  const timers: (() => void)[] = [];
  let observed: ((list: { getEntries: () => unknown[] }) => void) | null = null;
  class Observer {
    static supportedEntryTypes = ["navigation", "resource"];
    constructor(fn: typeof observed) {
      observed = fn;
    }
    observe() {}
  }
  FakeXhr.sent = [];
  const window: Rec = {
    webkit: { messageHandlers: { gvNetwork: { postMessage: (s: string) => void posts.push(JSON.parse(s)) } } },
    fetch: answer,
    XMLHttpRequest: class extends FakeXhr {},
    WebSocket: class extends FakeSocket {},
    ...own,
  };
  const run = new Function("window", "document", "PerformanceObserver", "setTimeout", source);
  run(window, { baseURI: "http://localhost:5173/app/" }, Observer, (fn: () => void) => timers.push(fn));
  /** Each entry as the app puts it together from the posts. */
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
  return { window, posts, entries, timers, resources: (list: unknown[]) => observed?.({ getEntries: () => list }) };
}

const settle = async () => {
  for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));
};

test("a fetch is logged in full and the page still gets its own response, body unread", async () => {
  const p = page(async () => new Response('{"id":5}', { status: 201, statusText: "Created", headers: { "content-type": "application/json" } }));
  const response: Response = await p.window.fetch("../api/items?page=2", {
    method: "post",
    headers: { "Content-Type": "application/json", "X-Trace": "t-1" },
    body: '{"name":"x"}',
  });
  assert.equal(response.bodyUsed, false);
  assert.deepEqual(await response.json(), { id: 5 }, "the page reads its body as ever");
  await settle();
  const [e] = p.entries();
  assert.equal(e.kind, "fetch");
  assert.equal(e.method, "POST");
  assert.equal(e.url, "http://localhost:5173/api/items?page=2");
  assert.deepEqual(e.requestHeaders, [["content-type", "application/json"], ["x-trace", "t-1"]]);
  assert.deepEqual(e.requestBody, { text: '{"name":"x"}', size: 12, cut: false });
  assert.equal(e.status, 201);
  assert.equal(e.statusText, "Created");
  assert.deepEqual(e.responseHeaders, [["content-type", "application/json"]]);
  assert.deepEqual(e.responseBody, { type: "application/json", size: 8, cut: false, text: '{"id":5}' });
  assert.ok(e.duration >= 0 && e.start > 0);
});

test("the page gets its own rejection, and an abort is no failure", async () => {
  const abort = Object.assign(new Error("aborted"), { name: "AbortError" });
  const answers = [new TypeError("Load failed"), abort];
  const p = page(async () => {
    throw answers.shift();
  });
  await assert.rejects(p.window.fetch("/a"), (e: unknown) => e instanceof TypeError);
  await assert.rejects(p.window.fetch("/b"), (e: unknown) => e === abort);
  assert.deepEqual(
    p.entries().map((e) => e.error),
    ["failed", "aborted"],
  );
});

test("a big body is kept to its start, its copy let go there, and timing gives the rest", async () => {
  let pulls = 0;
  const chunk = new TextEncoder().encode("é".repeat(1000));
  const big = new ReadableStream<Uint8Array>({
    pull(c) {
      if (++pulls > 40) c.close();
      else c.enqueue(chunk);
    },
  });
  const p = page(async () => new Response(big, { headers: { "content-type": "application/json" } }));
  const response: Response = await p.window.fetch("/big");
  await settle();
  // The page hasn't read: its copy stopped past 8192 characters, and so did the source.
  assert.ok(pulls < 15, `${pulls} pulls`);
  const body = p.entries()[0].responseBody;
  assert.deepEqual([body.cut, body.atLeast, body.text.length], [true, true, 8192]);
  assert.ok(body.text.endsWith("…") && body.size >= 8192);
  assert.equal((await response.text()).length, 40_000, "the page still reads all of it");
  assert.ok(p.entries()[0].duration >= 0, "until the copy let go");
  p.resources([{ entryType: "resource", initiatorType: "fetch", name: "http://localhost:5173/big", startTime: 1, duration: 42, decodedBodySize: 80_000 }]);
  const e = p.entries()[0];
  assert.deepEqual([e.duration, e.size], [42, 80_000]);
  assert.equal(p.entries().length, 1, "a fetch's own timing is no entry of its own");
});

test("a binary body is only its type, never read", async () => {
  const p = page(async () => new Response(new Uint8Array(3000), { headers: { "content-type": "image/png", "content-length": "3000" } }));
  const response: Response = await p.window.fetch("/logo.png");
  assert.equal(response.body!.locked, false);
  await settle();
  assert.deepEqual(p.entries()[0].responseBody, { type: "image/png" });
});

test("a body that may stream on is never copied: the page's stream is its alone", async () => {
  for (const type of ["text/event-stream", "application/x-ndjson", "text/plain"]) {
    let push: (s: string) => void = () => {};
    let cancelled = false;
    const stream = new ReadableStream<Uint8Array>({
      start(c) {
        push = (s) => c.enqueue(new TextEncoder().encode(s));
      },
      cancel() {
        cancelled = true;
      },
    });
    const p = page(async () => new Response(stream, { headers: { "content-type": type } }));
    const response: Response = await p.window.fetch("/events");
    push("data: one\n\n");
    const reader = response.body!.getReader();
    assert.equal(new TextDecoder().decode((await reader.read()).value), "data: one\n\n");
    await settle();
    const e = p.entries()[0];
    assert.deepEqual([e.status, e.responseBody], [200, { type }], type);
    assert.ok(e.duration >= 0, "until its headers");
    await reader.cancel();
    assert.ok(cancelled, `${type}: the page's cancel reaches the source`);
  }
});

test("a Request's body and odd headers reach the page's fetch untouched", async () => {
  let seen: [string, string[]] | null = null;
  const p = page(async (input, init) => {
    seen = [await (input as Request).text(), [...(init?.headers ?? [])].map(String)];
    return new Response("ok");
  });
  const iterator = new Map([["x-one", "1"]]).entries();
  await p.window.fetch(new Request("http://localhost:5173/form", { method: "POST", body: "a=1&b=2" }), { headers: iterator });
  await settle();
  assert.deepEqual(seen, ["a=1&b=2", ["x-one,1"]], "the page's own iterator wasn't used up");
  const e = p.entries()[0];
  assert.equal(e.method, "POST");
  assert.deepEqual(e.requestHeaders, [], "an iterator isn't read");
  assert.equal(e.requestBody.text, "a=1&b=2");
});

test("fetch keeps its name and length, and form bodies show their fields", async () => {
  const p = page();
  assert.equal(p.window.fetch.name, "fetch");
  assert.equal(p.window.fetch.length, 1);
  const form = new FormData();
  form.append("name", "x");
  form.append("file", new Blob(["abc"]), "a.txt");
  await p.window.fetch("/upload", { method: "POST", body: form });
  await p.window.fetch("/q", { method: "POST", body: new URLSearchParams({ q: "a b" }) });
  await p.window.fetch("/bin", { method: "PUT", body: new Uint8Array(10) });
  assert.deepEqual(
    p.entries().map((e) => e.requestBody.text ?? e.requestBody.type),
    ["name=x&file=[file a.txt, 3 bytes]", "q=a+b", "binary"],
  );
});

test("an XMLHttpRequest is logged with its headers, its body and how it ended", () => {
  const p = page();
  const r = new p.window.XMLHttpRequest();
  r.open("put", "/api/x");
  r.setRequestHeader("X-Token", "abc");
  r.send('{"a":1}');
  r.responseURL = "http://localhost:5173/api/x/1";
  r.finish(200, '{"ok":true}', "content-type: application/json\r\nx-id: 7\r\n");
  const again = r;
  again.open("get", "/api/y");
  again.send();
  again.fire("abort");
  again.fire("loadend", { loaded: 0 });
  const [one, two] = p.entries();
  assert.equal(one.url, "http://localhost:5173/api/x");
  assert.equal(one.to, "http://localhost:5173/api/x/1");
  assert.deepEqual(one.requestHeaders, [["x-token", "abc"]]);
  assert.equal(one.requestBody.text, '{"a":1}');
  assert.deepEqual(one.responseHeaders, [["content-type", "application/json"], ["x-id", "7"]]);
  assert.deepEqual(one.responseBody, { text: '{"ok":true}', size: 11, cut: false, type: "application/json" });
  assert.equal(one.status, 200);
  assert.equal(two.method, "GET");
  assert.equal(two.error, "aborted");
  assert.equal(two.status, undefined);
});

test("an XMLHttpRequest to an address with a fragment isn't taken for a redirect", () => {
  const p = page();
  const r = new p.window.XMLHttpRequest();
  r.open("GET", "/api/y#top");
  r.send();
  r.responseURL = "http://localhost:5173/api/y";
  r.finish(200, "{}");
  const [e] = p.entries();
  assert.equal(e.url, "http://localhost:5173/api/y");
  assert.equal(e.to, undefined);
});

test("an XMLHttpRequest the page misuses throws as ever and logs nothing", () => {
  const p = page();
  const r = new p.window.XMLHttpRequest();
  assert.throws(() => r.send(), /InvalidStateError/);
  assert.throws(() => r.setRequestHeader("a", "b"), /InvalidStateError/);
  assert.equal(p.entries().length, 0);
  assert.equal(p.window.XMLHttpRequest.prototype.open.name, "open");
});

test("a WebSocket is the page's own class still, and its frames and close are logged", () => {
  const p = page();
  const Socket = p.window.WebSocket;
  const ws = new Socket("/live");
  assert.ok(ws instanceof Socket && ws instanceof FakeSocket);
  assert.equal(ws.constructor, Socket);
  assert.equal(Socket.OPEN, 1);
  assert.throws(() => Socket("/x"), TypeError);
  ws.dispatchEvent(new Event("open"));
  ws.send("hello");
  ws.send(new Uint8Array(16));
  ws.dispatchEvent(new MessageEvent("message", { data: "x".repeat(2000) }));
  ws.dispatchEvent(Object.assign(new Event("close"), { code: 1006, reason: "gone", wasClean: false }));
  assert.deepEqual(ws.sent.length, 2, "the page's sends went out");
  const [e] = p.entries();
  assert.equal(e.kind, "ws");
  assert.equal(e.url, "ws://localhost:5173/live");
  assert.equal(e.status, 101);
  assert.deepEqual(
    e.frames.map((f: Rec) => [f.n, f.out, f.size, f.text?.length]),
    [
      [1, true, 5, 5],
      [2, true, 16, undefined],
      [3, false, 2000, 1024],
    ],
  );
  assert.deepEqual(e.close, { code: 1006, reason: "gone", clean: false });
});

test("everything else comes from resource timing, in batches, without fetch's twice", () => {
  const p = page();
  const list = [
    { entryType: "navigation", initiatorType: "navigation", name: "http://localhost:5173/", startTime: 0, duration: 120, decodedBodySize: 900 },
    { entryType: "resource", initiatorType: "fetch", name: "http://localhost:5173/api", startTime: 5, duration: 9, transferSize: 10 },
    ...Array.from({ length: 150 }, (_, i) => ({
      entryType: "resource",
      initiatorType: "script",
      name: `http://localhost:5173/src/m${i}.ts`,
      startTime: 10 + i,
      duration: 3,
      transferSize: 0,
      encodedBodySize: 400,
      responseStatus: 200,
    })),
  ];
  p.resources(list);
  assert.equal(p.posts.length, 2, "two posts of at most a hundred");
  const entries = p.entries();
  assert.equal(entries.length, 151);
  assert.deepEqual(
    [entries[0].initiator, entries[0].size, entries[1].initiator, entries[1].size, entries[1].status],
    ["navigation", 900, "script", 400, 200],
  );
  assert.ok(!entries.some((e) => e.url.endsWith("/api")));
});

test("a page requesting in a loop is cut off at a hundred a second, and the rest counted", async () => {
  const p = page();
  for (let i = 0; i < 130; i++) void p.window.fetch(`/poll/${i}`);
  await settle();
  assert.equal(p.entries().length, 100);
  assert.equal(p.timers.length, 1);
  p.timers[0]();
  assert.deepEqual(p.posts.at(-1), [{ skipped: 30 }]);
});

test("without its handler, or run twice, it changes nothing", () => {
  const fetch0 = async () => new Response("");
  const bare: Rec = { fetch: fetch0 };
  new Function("window", "document", source)(bare, {});
  assert.equal(bare.fetch, fetch0);
  const p = page();
  const once = p.window.fetch;
  new Function("window", "document", source)(p.window, {});
  assert.equal(p.window.fetch, once);
});

test("with the console on too, as the app adds them, both see a failed request", async () => {
  const p = page(async () => new Response("no", { status: 500 }));
  const lines: Rec[] = [];
  p.window.webkit.messageHandlers.gvConsole = { postMessage: (s: string) => void lines.push(JSON.parse(s)) };
  p.window.addEventListener = () => {};
  const consoleJs = readFileSync(new URL("../../../src-tauri/src/browser/scripts/console.js", import.meta.url), "utf8");
  new Function("window", "console", "location", consoleJs)(p.window, { error() {}, warn() {} }, { href: "http://localhost:5173/" });
  const response: Response = await p.window.fetch("/api/boom");
  assert.equal(await response.text(), "no");
  await settle();
  assert.equal(lines[0].msg, "[network] GET /api/boom → 500");
  assert.equal(p.entries()[0].status, 500);
});

test("a frame's requests say which frame, each as it starts; the page's own say nothing", async () => {
  const framed = page(async () => new Response("ok"), { top: {}, location: { origin: "https://pay.example.test" } });
  await framed.window.fetch("/charge", { method: "POST", body: "x" });
  await settle();
  const starts = framed.posts.flat().filter((u) => u.kind);
  assert.ok(starts.length > 0 && starts.every((u) => u.inFrame === "https://pay.example.test"));
  assert.ok(framed.posts.flat().filter((u) => !u.kind).every((u) => !("inFrame" in u)), "updates carry no tag");
  const own = page(async () => new Response("ok"));
  await own.window.fetch("/x");
  await settle();
  assert.ok(own.posts.flat().every((u) => !("inFrame" in u)));
});
