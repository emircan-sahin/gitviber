import assert from "node:assert/strict";
import { test } from "node:test";
import { BLANK, isPageUrl, normalizeUrl, pageHost, pageLabel } from "./url.ts";

test("a port, this machine and the LAN load over http; a name with a dot over https", () => {
  const cases: [string, string][] = [
    ["3000", "http://localhost:3000/"],
    [":5173", "http://localhost:5173/"],
    ["  5173 ", "http://localhost:5173/"],
    ["localhost", "http://localhost/"],
    ["localhost:5173/docs?x=1#top", "http://localhost:5173/docs?x=1#top"],
    ["app.localhost:3000", "http://app.localhost:3000/"],
    ["127.0.0.1:8080", "http://127.0.0.1:8080/"],
    ["192.168.1.20:3000", "http://192.168.1.20:3000/"],
    ["[::1]:4000", "http://[::1]:4000/"],
    ["example.dev", "https://example.dev/"],
    ["example.dev:8443/a", "https://example.dev:8443/a"],
    ["docs.example.com/guide", "https://docs.example.com/guide"],
    ["http://example.com", "http://example.com/"],
    ["HTTPS://Example.com/A", "https://example.com/A"],
    [BLANK, BLANK],
  ];
  for (const [typed, url] of cases) assert.equal(normalizeUrl(typed), url, typed);
});

test("no search, and nothing but web pages", () => {
  for (const typed of [
    "",
    "   ",
    "react hooks",
    "foo",
    "0",
    "70000",
    "javascript:alert(1)",
    "file:///etc/passwd",
    "data:text/html,hi",
    "mailto:a@example.com",
    "tauri://localhost/",
    "ftp://example.com/",
    "about:srcdoc",
    "999.1.1.1",
    "http://",
  ]) {
    assert.equal(normalizeUrl(typed), null, typed);
  }
});

test("a stored tab's address loads again only when it's a web page", () => {
  assert.equal(isPageUrl("http://localhost:5173/"), true);
  assert.equal(isPageUrl(BLANK), true);
  assert.equal(isPageUrl("javascript:alert(1)"), false);
  assert.equal(isPageUrl("file:///tmp/x.html"), false);
  assert.equal(isPageUrl(42), false);
});

test("a page is named by its address without the scheme", () => {
  assert.equal(pageLabel("http://localhost:5173/"), "localhost:5173");
  assert.equal(pageLabel("https://example.dev/docs/"), "example.dev/docs/");
  assert.equal(pageLabel("http://localhost:5173/a?b=1"), "localhost:5173/a?b=1");
  assert.equal(pageLabel(BLANK), "New Tab");
  assert.equal(pageHost("http://localhost:5173/a"), "localhost:5173");
  assert.equal(pageHost(BLANK), "");
});
