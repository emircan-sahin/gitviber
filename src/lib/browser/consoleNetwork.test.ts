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
      ["warn", "[network] POST /api/missing?x=1 → 404"],
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
