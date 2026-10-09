import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// console.js's failed requests: what a page's fetch and XMLHttpRequest say went wrong, and
// nothing of what they carried.
const source = readFileSync(new URL("../../../src-tauri/src/browser/scripts/console.js", import.meta.url), "utf8");

type Listener = () => void;

/** A page with a fetch answering `answers` in turn, and an XMLHttpRequest the test finishes. */
function page(answers: (number | Error)[]) {
  const sent: { level: string; msg: string; stack: string }[] = [];
  const requests: FakeXhr[] = [];
  class FakeXhr {
    status = 0;
    listeners: Record<string, Listener[]> = {};
    addEventListener(type: string, fn: Listener) {
      (this.listeners[type] ??= []).push(fn);
    }
    open(_method: string, _url: string) {}
    send() {
      requests.push(this);
    }
    finish(type: string, status = 0) {
      this.status = status;
      for (const fn of this.listeners[type] ?? []) fn();
    }
  }
  const window = {
    webkit: { messageHandlers: { gvConsole: { postMessage: (s: string) => void sent.push(JSON.parse(s)) } } },
    addEventListener: () => {},
    fetch: (_input: unknown, _init?: unknown) => {
      const answer = answers.shift()!;
      return answer instanceof Error ? Promise.reject(answer) : Promise.resolve({ status: answer });
    },
    XMLHttpRequest: FakeXhr,
  };
  const console = { error: () => {}, warn: () => {} };
  new Function("window", "console", "location", source)(window, console, { href: "http://localhost:5173/app" });
  return { window, sent, requests };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

test("failed fetches log as warnings or errors with the method, the address and the status", async () => {
  const abort = Object.assign(new Error("aborted"), { name: "AbortError" });
  const p = page([200, 404, 500, new TypeError("Load failed"), abort, 302]);
  const f = p.window.fetch as (input: unknown, init?: unknown) => Promise<unknown>;
  await f("/api/ok").catch(() => {});
  await f("/api/missing?x=1", { method: "post", body: "secret" }).catch(() => {});
  await f({ url: "http://localhost:5173/api/boom", method: "PUT" }).catch(() => {});
  await f("https://other.test/data").catch(() => {});
  await f("/api/cancelled").catch(() => {});
  await f("/redirect").catch(() => {});
  await settle();
  assert.deepEqual(
    p.sent.map((s) => [s.level, s.msg]),
    [
      ["warn", "[network] POST /api/missing?… → 404"],
      ["error", "[network] PUT /api/boom → 500"],
      ["error", "[network] GET https://other.test/data failed"],
    ],
  );
  assert.ok(p.sent.every((s) => !s.msg.includes("secret") && s.stack === ""));
});

test("the page still gets its own answer and its own rejection", async () => {
  const p = page([503, new TypeError("Load failed")]);
  const f = p.window.fetch as (input: unknown) => Promise<{ status: number }>;
  assert.equal((await f("/a")).status, 503);
  await assert.rejects(f("/b"), /Load failed/);
});

test("XMLHttpRequest failures log too, and a long address is cut", async () => {
  const p = page([]);
  const Xhr = p.window.XMLHttpRequest;
  for (const [method, url] of [["get", "/api/x"], ["delete", `/api/${"y".repeat(500)}`], ["get", "/api/z"]]) {
    const r = new Xhr();
    r.open(method, url);
    r.send();
  }
  p.requests[0].finish("load", 500);
  p.requests[1].finish("load", 403);
  p.requests[2].finish("load", 200);
  p.requests[2].finish("error");
  assert.equal(p.sent[0].msg, "[network] GET /api/x → 500");
  assert.equal(p.sent[1].level, "warn");
  assert.ok(p.sent[1].msg.length < 330, "cut");
  assert.equal(p.sent[2].msg, "[network] GET /api/z failed");
  assert.equal(p.sent.length, 3);
});

test("a fetch failure the page never handles is still an unhandled rejection", async () => {
  const p = page([new TypeError("Load failed")]);
  const seen: unknown[] = [];
  const on = (e: unknown) => void seen.push(e);
  // The test runner's own listener would count it as this test's failure: set aside meanwhile.
  const runners = process.listeners("unhandledRejection");
  process.removeAllListeners("unhandledRejection");
  process.on("unhandledRejection", on);
  void (p.window.fetch as any)("/x");
  await settle();
  process.off("unhandledRejection", on);
  for (const l of runners) process.on("unhandledRejection", l);
  assert.equal(seen.length, 1);
});

test("an XMLHttpRequest used twice logs its second failure once, under its second address", async () => {
  const p = page([]);
  const r = new (p.window.XMLHttpRequest as any)();
  r.open("GET", "/a"); r.send(); r.finish("load", 200);
  r.open("GET", "/b"); r.send(); r.finish("load", 500);
  assert.deepEqual(p.sent.map((s) => s.msg), ["[network] GET /b → 500"]);
});

test("a token in a failed request's query isn't logged for the agent", async () => {
  const p = page([401]);
  await (p.window.fetch as any)("/api/me?access_token=s3cr3t&x=1").catch(() => {});
  await settle();
  assert.ok(!p.sent[0].msg.includes("s3cr3t"));
  const q = page([500]);
  await (q.window.fetch as any)("https://api.other.test/v1/x?key=k3y").catch(() => {});
  await settle();
  assert.equal(q.sent[0].msg, "[network] GET https://api.other.test/v1/x?… → 500");
});
