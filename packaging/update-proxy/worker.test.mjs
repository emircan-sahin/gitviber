import assert from "node:assert/strict";
import { afterEach, describe, it } from "node:test";
import worker from "./worker.js";

const HOST = "https://updates.example.test";
const REL = "https://github.com/emircan-sahin/gitviber/releases";
const BLOB = "https://release-assets.githubusercontent.com/blob/1234?sig=abc";

const realFetch = globalThis.fetch;
/** Stubs fetch with `routes` (url -> Response or (init) => Response); returns the calls made. */
function stub(routes) {
  const calls = [];
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url: String(url), init });
    const route = routes[String(url)];
    if (!route) throw new Error(`unexpected fetch ${url}`);
    return typeof route === "function" ? route(init) : route.clone();
  };
  return calls;
}
const redirect = (to, status = 302) => new Response(null, { status, headers: { location: to } });
const get = (path, init) => worker.fetch(new Request(`${HOST}${path}`, init), {}, undefined);

const manifest = (urls) => ({
  version: "0.1.9",
  notes: "n",
  pub_date: "2026-10-03T00:00:00Z",
  platforms: Object.fromEntries(Object.entries(urls).map(([key, url]) => [key, { signature: `sig-${key}`, url }])),
});
const good = {
  "darwin-aarch64": `${REL}/download/v0.1.9/GitViber_0.1.9_universal.app.tar.gz`,
  "darwin-x86_64": `${REL}/download/v0.1.9/GitViber_0.1.9_universal.app.tar.gz`,
  "linux-x86_64": `${REL}/download/v0.1.9/GitViber_0.1.9_amd64.AppImage`,
  "linux-x86_64-rpm": `${REL}/download/v0.1.9/GitViber-0.1.9-1.x86_64.rpm`,
};
const json = (value) => new Response(JSON.stringify(value), { headers: { "content-type": "text/plain" } });

afterEach(() => {
  globalThis.fetch = realFetch;
  delete globalThis.caches;
});

describe("latest.json", () => {
  it("points every platform's url at the mirror and keeps the rest", async () => {
    const calls = stub({ [`${REL}/latest/download/latest.json`]: redirect(`${REL}/download/v0.1.9/latest.json`), [`${REL}/download/v0.1.9/latest.json`]: redirect(BLOB), [BLOB]: json(manifest(good)) });
    const res = await get("/latest.json");
    assert.equal(res.status, 200);
    assert.equal(res.headers.get("cache-control"), "public, max-age=300");
    assert.match(res.headers.get("content-type"), /^application\/json/);
    assert.equal(res.headers.get("x-content-type-options"), "nosniff");
    const out = await res.json();
    assert.equal(out.version, "0.1.9");
    assert.equal(out.pub_date, "2026-10-03T00:00:00Z");
    assert.equal(out.platforms["darwin-aarch64"].signature, "sig-darwin-aarch64");
    assert.equal(out.platforms["darwin-x86_64"].url, `${HOST}/download/v0.1.9/GitViber_0.1.9_universal.app.tar.gz`);
    assert.equal(out.platforms["linux-x86_64"].url, `${HOST}/download/v0.1.9/GitViber_0.1.9_amd64.AppImage`);
    assert.equal(out.platforms["linux-x86_64-rpm"].url, `${HOST}/download/v0.1.9/GitViber-0.1.9-1.x86_64.rpm`);
    assert.equal(calls.length, 3);
  });

  it("takes the host from the request", async () => {
    stub({ [`${REL}/latest/download/latest.json`]: json(manifest(good)) });
    const res = await worker.fetch(new Request("https://mirror.other.test/latest.json"), {});
    assert.ok((await res.json()).platforms["linux-x86_64"].url.startsWith("https://mirror.other.test/download/v0.1.9/"));
  });

  for (const [name, url] of Object.entries({
    "another host": "https://evil.example/GitViber.app.tar.gz",
    "another repository": "https://github.com/someone/else/releases/download/v0.1.9/GitViber_0.1.9_universal.app.tar.gz",
    "a lookalike host": "https://github.com.evil.example/emircan-sahin/gitviber/releases/download/v0.1.9/a.dmg",
    "plain http": "http://github.com/emircan-sahin/gitviber/releases/download/v0.1.9/a.dmg",
    "an extra path segment": `${REL}/download/v0.1.9/x/a.dmg`,
    "a query string": `${REL}/download/v0.1.9/a.dmg?x=1`,
    "a traversal": `${REL}/download/v0.1.9/../../../a.dmg`,
    "a wrong tag": `${REL}/download/latest/a.dmg`,
    "a wrong suffix": `${REL}/download/v0.1.9/a.exe`,
    "no url": undefined,
  })) {
    it(`fails with 502, serving nothing, when one url has ${name}`, async () => {
      stub({ [`${REL}/latest/download/latest.json`]: json(manifest({ ...good, "darwin-aarch64-app": url })) });
      const res = await get("/latest.json");
      assert.equal(res.status, 502);
      assert.doesNotMatch(await res.text(), /evil|sig-/);
    });
  }

  it("fails with 502 when there are no platforms or the file isn't a manifest", async () => {
    for (const body of [manifest({}), { version: "1" }, [1], "x"]) {
      stub({ [`${REL}/latest/download/latest.json`]: json(body) });
      assert.equal((await get("/latest.json")).status, 502);
    }
    stub({ [`${REL}/latest/download/latest.json`]: new Response("<html>", { status: 200 }) });
    assert.equal((await get("/latest.json")).status, 502);
  });

  it("passes a missing release on as 404 and other upstream errors as 502", async () => {
    stub({ [`${REL}/latest/download/latest.json`]: new Response("", { status: 404 }) });
    assert.equal((await get("/latest.json")).status, 404);
    stub({ [`${REL}/latest/download/latest.json`]: new Response("", { status: 503 }) });
    assert.equal((await get("/latest.json")).status, 502);
    globalThis.fetch = async () => {
      throw new Error("network down");
    };
    assert.equal((await get("/latest.json")).status, 502);
  });

  it("answers HEAD, and ignores a query string", async () => {
    stub({ [`${REL}/latest/download/latest.json`]: json(manifest(good)) });
    assert.equal((await get("/latest.json?cache=bust", { method: "HEAD" })).status, 200);
  });
});

describe("download", () => {
  const FILE = "GitViber_0.1.9_universal.app.tar.gz";
  const source = `${REL}/download/v0.1.9/${FILE}`;
  const bytes = "x".repeat(100);
  const asset = (init = {}) => new Response(bytes, { headers: { "content-length": "100", "content-type": "application/octet-stream", "accept-ranges": "bytes", etag: '"abc"', "set-cookie": "a=b", server: "azure", "content-disposition": "attachment" }, ...init });

  it("streams the file from GitHub through its redirect, with the headers a downloader needs", async () => {
    const calls = stub({ [source]: redirect(BLOB), [BLOB]: asset() });
    const res = await get(`/download/v0.1.9/${FILE}`);
    assert.equal(res.status, 200);
    assert.equal(await res.text(), bytes);
    assert.equal(res.headers.get("content-length"), "100");
    assert.equal(res.headers.get("content-type"), "application/octet-stream");
    assert.equal(res.headers.get("accept-ranges"), "bytes");
    assert.equal(res.headers.get("etag"), '"abc"');
    assert.equal(res.headers.get("cache-control"), "public, max-age=31536000, immutable");
    assert.equal(res.headers.get("x-content-type-options"), "nosniff");
    for (const dropped of ["set-cookie", "server", "content-disposition"]) assert.equal(res.headers.get(dropped), null);
    assert.deepEqual(calls.map((c) => c.url), [source, BLOB]);
    assert.ok(calls.every((c) => c.init.redirect === "manual" && c.init.method === "GET" && c.init.signal));
  });

  it("sends nothing of the caller's to GitHub except the range", async () => {
    const calls = stub({ [source]: asset() });
    await get(`/download/v0.1.9/${FILE}`, { headers: { cookie: "s=1", authorization: "Bearer t", "x-forwarded-for": "1.2.3.4" } });
    assert.deepEqual(Object.keys(calls[0].init.headers), ["user-agent"]);
  });

  it("forwards a Range and returns the 206 as it is", async () => {
    const calls = stub({ [source]: redirect(BLOB), [BLOB]: new Response("xxxxx", { status: 206, headers: { "content-length": "5", "content-range": "bytes 0-4/100", "content-type": "application/octet-stream" } }) });
    const res = await get(`/download/v0.1.9/${FILE}`, { headers: { range: "bytes=0-4" } });
    assert.equal(res.status, 206);
    assert.equal(res.headers.get("content-range"), "bytes 0-4/100");
    assert.equal(res.headers.get("content-length"), "5");
    assert.equal(res.headers.get("accept-ranges"), "bytes");
    assert.deepEqual(calls.map((c) => c.init.headers.range), ["bytes=0-4", "bytes=0-4"]);
  });

  it("refuses a range it can't read without asking GitHub", async () => {
    for (const range of ["bytes=0-1,5-9", "items=0-4", "bytes=a-b"]) {
      const calls = stub({});
      const res = await get(`/download/v0.1.9/${FILE}`, { headers: { range } });
      assert.equal(res.status, 416, range);
      assert.equal(calls.length, 0);
    }
  });

  it("passes a missing file on as 404, 416 as it is, and server trouble as 502, without GitHub's page", async () => {
    for (const [status, expected] of [[404, 404], [416, 416], [403, 403], [429, 429], [500, 502], [503, 502], [204, 502]]) {
      stub({ [source]: new Response(status === 204 ? null : "<html>secret page</html>", { status }) });
      const res = await get(`/download/v0.1.9/${FILE}`);
      assert.equal(res.status, expected, `upstream ${status}`);
      assert.doesNotMatch(await res.text(), /secret/);
      assert.equal(res.headers.get("cache-control"), "no-store");
    }
  });

  it("answers 502 when GitHub can't be reached", async () => {
    globalThis.fetch = async () => {
      throw new Error("down");
    };
    assert.equal((await get(`/download/v0.1.9/${FILE}`)).status, 502);
  });

  it("gives up on a GitHub that doesn't answer, but doesn't cut a download short", async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    globalThis.fetch = (_url, init) => new Promise((_, reject) => init.signal.addEventListener("abort", () => reject(new Error("aborted"))));
    const pending = get(`/download/v0.1.9/${FILE}`);
    await new Promise((resolve) => setImmediate(resolve));
    t.mock.timers.tick(15_000);
    assert.equal((await pending).status, 502);

    let signal;
    globalThis.fetch = async (_url, init) => ((signal = init.signal), asset());
    const res = await get(`/download/v0.1.9/${FILE}`);
    t.mock.timers.tick(60_000);
    assert.equal(signal.aborted, false);
    assert.equal(await res.text(), bytes);
  });

  it("answers HEAD with a HEAD to GitHub and no body", async () => {
    const calls = stub({ [source]: new Response(null, { headers: { "content-length": "100", "content-type": "application/octet-stream" } }) });
    const res = await get(`/download/v0.1.9/${FILE}`, { method: "HEAD" });
    assert.equal(res.status, 200);
    assert.equal(res.headers.get("content-length"), "100");
    assert.equal(await res.text(), "");
    assert.equal(calls[0].init.method, "HEAD");
  });

  it("refuses other methods", async () => {
    for (const method of ["POST", "PUT", "DELETE", "PATCH", "OPTIONS"]) {
      const calls = stub({});
      const res = await get(`/download/v0.1.9/${FILE}`, { method });
      assert.equal(res.status, 405, method);
      assert.equal(res.headers.get("allow"), "GET, HEAD");
      assert.equal(calls.length, 0);
    }
  });

  it("never follows a redirect off GitHub, or one that doesn't end", async () => {
    for (const to of ["https://evil.example/x", "http://github.com/x", "https://githubusercontent.com.evil.example/x", "https://evilgithubusercontent.com/x", "ftp://github.com/x"]) {
      const calls = stub({ [source]: redirect(to) });
      assert.equal((await get(`/download/v0.1.9/${FILE}`)).status, 502, to);
      assert.equal(calls.length, 1);
    }
    const calls = stub({ [source]: redirect(source) });
    assert.equal((await get(`/download/v0.1.9/${FILE}`)).status, 502);
    assert.equal(calls.length, 5);
  });

  it("follows a relative redirect and the content hosts GitHub uses", async () => {
    const other = "https://objects.githubusercontent.com/a/b";
    stub({ [source]: redirect(`/emircan-sahin/gitviber/releases/download/v0.1.9/again-${FILE}`, 301), [`${REL}/download/v0.1.9/again-${FILE}`]: redirect(other, 307), [other]: asset() });
    assert.equal((await get(`/download/v0.1.9/${FILE}`)).status, 200);
  });

  it("builds the upstream url from the validated parts only", async () => {
    const calls = stub({ [source]: asset() });
    await get(`/download/v0.1.9/${FILE}?url=https://evil.example&x=1#frag`);
    assert.deepEqual(calls.map((c) => c.url), [source]);
  });

  describe("rejects with 404, without asking GitHub", () => {
    const names = {
      "a bare download": "/download",
      "no file": "/download/v0.1.9/",
      "no tag": `/download/${FILE}`,
      "an extra segment": `/download/v0.1.9/x/${FILE}`,
      "a traversal": `/download/v0.1.9/../${FILE}`,
      "a traversal to another tag": "/download/v0.1.9/..%2Fv0.1.8%2Fx.dmg",
      "an encoded slash": "/download/v0.1.9/a%2Fb.dmg",
      "an encoded slash in the tag": "/download/v0.1.9%2Fx/a.dmg",
      "an upper-case encoded slash": "/download/v0.1.9/a%2fb.dmg",
      "an encoded dot": "/download/v0.1.9/%2e%2e.dmg",
      "an encoded dot pair before the suffix": "/download/v0.1.9/a%2e%2e.dmg",
      "a double encoded slash": "/download/v0.1.9/a%252fb.dmg",
      "a backslash": "/download/v0.1.9/a%5Cb.dmg",
      "a raw backslash": "/download/v0.1.9/a\\b.dmg",
      "a double dot": "/download/v0.1.9/a..b.dmg",
      "a double dot in the tag": "/download/v1.2.3-rc..1/a.dmg",
      "a percent sign": "/download/v0.1.9/a%20b.dmg",
      "a space": "/download/v0.1.9/a b.dmg",
      "a null byte": "/download/v0.1.9/a%00.dmg",
      "a newline": "/download/v0.1.9/a%0A.dmg",
      "a tag without v": `/download/0.1.9/${FILE}`,
      "a tag with a minor only": `/download/v0.1/${FILE}`,
      "a tag with four numbers": `/download/v0.1.9.1/${FILE}`,
      "a tag with a leading zero word": `/download/vx.1.9/${FILE}`,
      "a tag named latest": `/download/latest/${FILE}`,
      "a tag with an empty prerelease": `/download/v0.1.9-/${FILE}`,
      "a tag with a slash-like prerelease": `/download/v0.1.9-rc_1/${FILE}`,
      "a branch name": `/download/main/${FILE}`,
      "an unknown suffix": "/download/v0.1.9/GitViber.exe",
      "a suffix only": "/download/v0.1.9/.dmg",
      "a suffix that isn't one": "/download/v0.1.9/GitViber.dmg.exe",
      "a close suffix": "/download/v0.1.9/GitViber.AppImages",
      "a dotfile": "/download/v0.1.9/.hidden",
      "a name that only looks like SHA256SUMS": "/download/v0.1.9/SHA256SUMS2",
      "a lower-case SHA256SUMS": "/download/v0.1.9/sha256sums",
      "a name that only ends like latest.json": "/download/v0.1.9/xlatest.json.bak",
      "a name 129 characters long": `/download/v0.1.9/${"a".repeat(125)}.dmg`,
      "a very long name": `/download/v0.1.9/${"a".repeat(5000)}.dmg`,
      "a very long tag": `/download/v0.1.9-${"a".repeat(5000)}/a.dmg`,
      "a trailing slash": `/download/v0.1.9/${FILE}/`,
      "a root path": "/",
      "an unknown path": "/releases/latest",
      "an upper-case route": `/Download/v0.1.9/${FILE}`,
      "a latest.json with a suffix": "/latest.json/x",
      "a latest.json in another case": "/Latest.json",
    };
    for (const [name, path] of Object.entries(names)) {
      it(name, async () => {
        const calls = stub({});
        const res = await get(path);
        assert.equal(res.status, 404, path);
        assert.equal(calls.length, 0);
      });
    }

    it("a target given as an absolute url or a protocol-relative one", async () => {
      const calls = stub({});
      for (const path of [`/https://github.com/emircan-sahin/gitviber/releases/download/v0.1.9/${FILE}`, `//evil.example/download/v0.1.9/${FILE}`]) {
        assert.equal((await get(path)).status, 404);
      }
      assert.equal(calls.length, 0);
    });
  });

  it("serves every known kind of file, and the tag shapes the releases use", async () => {
    for (const file of ["GitViber_0.1.9_universal.app.tar.gz", "GitViber_0.1.9_universal.app.tar.gz.sig", "GitViber_0.1.9_universal.dmg", "GitViber_0.1.9_amd64.AppImage", "GitViber_0.1.9_amd64.AppImage.sig", "GitViber_0.1.9_amd64.deb", "GitViber_0.1.9_amd64.deb.sig", "GitViber-0.1.9-1.x86_64.rpm", "GitViber-0.1.9-1.x86_64.rpm.sig", "SHA256SUMS", "latest.json"]) {
      for (const tag of ["v0.1.9", "v10.20.30", "v0.1.1-rc.1"]) {
        const calls = stub({ [`${REL}/download/${tag}/${file}`]: asset() });
        const res = await get(`/download/${tag}/${file}`);
        assert.equal(res.status, 200, `${tag}/${file}`);
        assert.equal(calls.length, 1);
      }
    }
  });

  describe("cache", () => {
    const fakeCache = (hit) => {
      const puts = [];
      const matches = [];
      globalThis.caches = { default: { match: async (req) => (matches.push(req), hit), put: async (req, res) => void puts.push([req, res]) } };
      return { puts, matches };
    };
    const ctx = () => {
      const pending = [];
      return { pending, waitUntil: (p) => pending.push(p) };
    };

    it("serves a hit without asking GitHub", async () => {
      const { matches } = fakeCache(new Response("cached", { status: 200 }));
      const calls = stub({});
      const res = await get(`/download/v0.1.9/${FILE}?x=1`);
      assert.equal(await res.text(), "cached");
      assert.equal(calls.length, 0);
      assert.equal(matches[0].url, `${HOST}/download/v0.1.9/${FILE}`);
    });

    it("passes a Range on to the cache lookup", async () => {
      const { matches } = fakeCache(undefined);
      stub({ [source]: asset() });
      await get(`/download/v0.1.9/${FILE}`, { headers: { range: "bytes=1-2" } });
      assert.equal(matches[0].headers.get("range"), "bytes=1-2");
    });

    it("stores a whole 200 under the canonical url, and nothing else", async () => {
      const { puts } = fakeCache(undefined);
      stub({ [source]: asset() });
      const c = ctx();
      const res = await worker.fetch(new Request(`${HOST}/download/v0.1.9/${FILE}?x=1`), {}, c);
      assert.equal(await res.text(), bytes);
      await Promise.all(c.pending);
      assert.equal(puts.length, 1);
      assert.equal(puts[0][0].url, `${HOST}/download/v0.1.9/${FILE}`);
      assert.equal(await puts[0][1].text(), bytes);
      assert.equal(puts[0][1].headers.get("cache-control"), "public, max-age=31536000, immutable");

      for (const [init, upstreamRes] of [
        [{ headers: { range: "bytes=0-4" } }, new Response("xxxxx", { status: 206, headers: { "content-length": "5" } })],
        [{ method: "HEAD" }, new Response(null, { headers: { "content-length": "100" } })],
        [{}, new Response("nope", { status: 404 })],
        [{}, new Response(bytes, { status: 200 })],
      ]) {
        stub({ [source]: upstreamRes });
        const c2 = ctx();
        await worker.fetch(new Request(`${HOST}/download/v0.1.9/${FILE}`, init), {}, c2);
        assert.equal(c2.pending.length, 0, JSON.stringify(init));
      }
    });

    it("works when the cache or the context is missing, or the cache breaks", async () => {
      stub({ [source]: asset() });
      assert.equal((await get(`/download/v0.1.9/${FILE}`)).status, 200);
      globalThis.caches = { default: { match: async () => Promise.reject(new Error("x")), put: async () => Promise.reject(new Error("x")) } };
      stub({ [source]: asset() });
      const c = ctx();
      assert.equal((await worker.fetch(new Request(`${HOST}/download/v0.1.9/${FILE}`), {}, c)).status, 200);
      await Promise.all(c.pending);
    });
  });
});
