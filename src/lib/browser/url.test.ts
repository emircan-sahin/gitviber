import assert from "node:assert/strict";
import { test } from "node:test";
import { BLANK, isFrameable, isPageUrl, normalizeUrl, pageHost, pageLabel } from "./url.ts";

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

test("odd but valid addresses: IPv6, unicode hosts, case, stray spaces", () => {
  const cases: [string, string][] = [
    ["LOCALHOST", "http://localhost/"],
    ["App.LocalHost:3000", "http://app.localhost:3000/"],
    ["[::1]", "http://[::1]/"],
    ["[::1]:3000/a?b#c", "http://[::1]:3000/a?b#c"],
    ["0.0.0.0:3000", "http://0.0.0.0:3000/"],
    ["10.0.0.1", "http://10.0.0.1/"],
    // Port 80 is http's own: the URL parser writes none, and a stored tab reopens only as written.
    ["00080", "http://localhost/"],
    ["65535", "http://localhost:65535/"],
    ["\tlocalhost:5173\n", "http://localhost:5173/"],
    ["bücher.example", "https://xn--bcher-kva.example/"],
    ["Example.COM/Path", "https://example.com/Path"],
    ["example.com?q=1", "https://example.com/?q=1"],
    ["example.com#top", "https://example.com/#top"],
    ["HTTP://LOCALHOST:3000", "http://localhost:3000/"],
    ["http://example.com\\@evil.example", "http://example.com/@evil.example"],
  ];
  for (const [typed, url] of cases) assert.equal(normalizeUrl(typed), url, typed);
});

test("nothing that isn't a web page gets through, however it's spelled", () => {
  for (const typed of [
    "JavaScript:alert(1)",
    "javascript://example.com/%0aalert(1)",
    "JAVASCRIPT:alert(1)",
    "vbscript:msgbox(1)",
    "view-source:http://example.com",
    "blob:https://example.com/1",
    "data:,x",
    "file:/etc/hosts",
    "FILE:///etc/hosts",
    "x-apple.systempreferences:com.apple.preference",
    "vscode://file/tmp/x",
    "ABOUT:BLANK",
    "about:blank#x",
    "http:",
    "https://",
    "http:///",
    "http:example.com",
    "//example.com",
    "/path",
    "?q",
    "#x",
    "::1",
    "example.com:abc",
    "example.com:8443:1",
    "localhost:",
    "localhost:99999",
    "1.2.3.4.5",
    "[fe80::1%en0]:3000",
    "127.0.0.1:8080/a b",
  ]) {
    assert.equal(normalizeUrl(typed), null, typed);
  }
});

// A cheap fuzz: whatever comes out loads as typed again and is a page a stored tab may reopen.
test("what normalizeUrl returns is a web page, and normalizing it again changes nothing", () => {
  const parts = ["localhost", "127.0.0.1", "[::1]", "example.com", "a.b", ":", "5173", "/", "?", "#", "@", ".", "-", "_", "%", "\\", "http://", "https://", "javascript:", "file:", "data:", "ü", "例", " ", "\t", "0", "65536", "user:pw@", "about:blank"];
  let seed = 7;
  const rand = (n: number) => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff), seed % n);
  for (let i = 0; i < 5000; i++) {
    let typed = "";
    for (let j = rand(6) + 1; j > 0; j--) typed += parts[rand(parts.length)];
    const url = normalizeUrl(typed);
    if (url === null) continue;
    assert.ok(isPageUrl(url), `${JSON.stringify(typed)} -> ${url}`);
    assert.equal(normalizeUrl(url), url, JSON.stringify(typed));
    assert.ok(url === BLANK || /^https?:\/\/[^/]/.test(url), `${JSON.stringify(typed)} -> ${url}`);
  }
});

test("a stored tab's address is refused unless it's exactly as the parser writes it", () => {
  for (const url of ["http://localhost", "HTTP://localhost/", "about:blank#x", " http://localhost/", "", "about:srcdoc", "data:text/html,x"]) {
    assert.equal(isPageUrl(url), false, url);
  }
  for (const url of [null, undefined, {}, [], 0, true, new URL("http://localhost/")]) assert.equal(isPageUrl(url), false, String(url));
  assert.equal(isPageUrl("http://[::1]:3000/"), true);
});

test("labels and hosts of unusual pages", () => {
  assert.equal(pageLabel(""), "New Tab");
  assert.equal(pageLabel("http://[::1]:3000/"), "[::1]:3000");
  assert.equal(pageLabel("https://example.com//"), "example.com//");
  assert.equal(pageHost("http://[::1]:3000/a"), "[::1]:3000");
  assert.equal(pageHost("not a url"), "");
  assert.equal(pageHost(""), "");
});

test("no user name in an address, no port 0, and 127.x shorthand is this machine", () => {
  for (const typed of ["localhost:3000@evil.example", "user:pw@example.com", "http://user:pw@localhost:3000/", "https://user@example.com/", "localhost:0", ":0", "http://localhost:0/"]) {
    assert.equal(normalizeUrl(typed), null, typed);
  }
  assert.equal(isPageUrl("http://user:pw@localhost:3000/"), false);
  assert.equal(normalizeUrl("127.1"), "http://127.0.0.1/");
  assert.equal(normalizeUrl("127.1:5173/a"), "http://127.0.0.1:5173/a");
  assert.equal(normalizeUrl("127.0.1"), "http://127.0.0.1/");
});

test("a frame on Linux holds only localhost and 127.0.0.1, as the app's CSP allows", () => {
  for (const url of ["http://localhost:5173/", "http://127.0.0.1:3000/a", "https://localhost:5173/", "https://127.0.0.1:8443/"]) assert.equal(isFrameable(url), true, url);
  for (const url of ["http://[::1]:3000/", "http://app.localhost/", "http://example.com/", BLANK, "nonsense"]) assert.equal(isFrameable(url), false, url);
});

test("a Linux frame takes what the CSP's frame-src lets in, however the address is spelled", () => {
  // frame-src http(s)://localhost:* http(s)://127.0.0.1:*: any port, the default one too.
  for (const url of ["http://localhost/", "http://LOCALHOST:3000/a?b#c", "http://127.0.0.1/", "http://127.1:8080/", "http://0x7f.0.0.1:5173/"]) {
    assert.equal(isFrameable(url), true, url);
  }
  for (const url of [
    "http://localhost.:3000/",
    "http://127.0.0.2:3000/",
    "http://127.0.0.1.nip.io/",
    "http://localhost.example.com/",
    "http://0.0.0.0:3000/",
    "http://[::ffff:127.0.0.1]:3000/",
    "ws://localhost:3000/",
    "file:///tmp/x.html",
    "javascript:alert(1)",
    "data:text/html,x",
    "",
  ]) {
    assert.equal(isFrameable(url), false, url);
  }
});

test("what the address bar makes of a port is always framable on Linux", () => {
  for (const typed of ["3000", ":5173", "localhost:8080/docs", "127.0.0.1:4000", "65535"]) {
    const url = normalizeUrl(typed);
    assert.ok(url && isFrameable(url), `${typed} -> ${url}`);
  }
});

test("a port with a path, query or hash is this machine's page there", () => {
  const cases: [string, string][] = [
    ["5199/device.html", "http://localhost:5199/device.html"],
    [":5199/x?y#z", "http://localhost:5199/x?y#z"],
    ["3000?q=1", "http://localhost:3000/?q=1"],
    ["3000#top", "http://localhost:3000/#top"],
    ["8080/", "http://localhost:8080/"],
  ];
  for (const [typed, url] of cases) assert.equal(normalizeUrl(typed), url, typed);
  for (const typed of ["0/x", "70000/x", "5199x", "5199 /x"]) assert.equal(normalizeUrl(typed), null, typed);
});
