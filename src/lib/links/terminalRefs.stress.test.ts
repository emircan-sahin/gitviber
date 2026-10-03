import { test } from "node:test";
import assert from "node:assert/strict";
import { findTerminalLinks, githubItem } from "./links.ts";

// What a terminal shows that isn't a commit or an issue, and lines too big to look at slowly.
const refs = (line: string) => findTerminalLinks(line).flatMap((l) => (l.kind === "commit" || l.kind === "issue" ? [l.spec] : []));

test("hex that isn't a commit: UUIDs, long hashes, hex numbers, addresses, base64, versions", () => {
  assert.deepEqual(refs("550e8400-e29b-41d4-a716-446655440000"), []);
  assert.deepEqual(refs("{550e8400-e29b-41d4-a716-446655440000}"), []);
  // sha256sum's: longer than any SHA-1, so no part of it either.
  assert.deepEqual(refs("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  file.txt"), []);
  assert.deepEqual(refs("inet6 fe80::1c2a:3bff:fe4d:5e6f%en0 prefixlen 64"), []);
  assert.deepEqual(refs("2001:0db8:85a3:0000:0000:8a2e:0370:7334"), []);
  assert.deepEqual(refs("ether a4:83:e7:12:34:56"), []);
  assert.deepEqual(refs("0xdeadbeef 0Xcafebabe 0x9a1c2e4f"), []);
  assert.deepEqual(refs("PID 48213 started 20261002 took 1234567ms"), []);
  assert.deepEqual(refs("integrity sha512-9a1c2e4f7b3d8e6a0c5b2d4f6e8a0c2b4d6f8a0c2e4b6d8f0a2c4e6b8d0f2a4c6e8=="), []);
  assert.deepEqual(refs("Version 1.2.3-beta.abcdef12"), []);
  assert.deepEqual(refs("#a1b2c3d4 #ffffff"), []);
  assert.deepEqual(refs("9A1C2E4F DEADBEEF"), [], "git prints ids in lower case");
  // An md5 or a container id looks like a SHA: the repo says no (known_commits).
  assert.deepEqual(refs("d41d8cd98f00b204e9800998ecf8427e  empty"), ["d41d8cd98f00b204e9800998ecf8427e"]);
});

test("where a SHA is one: git's own output", () => {
  assert.deepEqual(refs("pick 9a1c2e4 fix the thing"), ["9a1c2e4"]);
  assert.deepEqual(refs("HEAD is now at 9a1c2e4 fix"), ["9a1c2e4"]);
  assert.deepEqual(refs("9a1c2e4 (HEAD -> main, origin/main) fix"), ["9a1c2e4"]);
  assert.deepEqual(refs("Updating 1f3a9c0..7bd4e21"), ["1f3a9c0", "7bd4e21"]);
  assert.deepEqual(refs("+ 1f3a9c0...7bd4e21 main -> main (forced update)"), ["1f3a9c0", "7bd4e21"]);
  assert.deepEqual(refs("CONFLICT (content): Merge conflict in a.ts\nerror: could not apply 9a1c2e4... fix"), ["9a1c2e4"]);
  assert.deepEqual(refs("* 9a1c2e4 - fix (2 days ago)"), ["9a1c2e4"]);
  assert.deepEqual(refs("|\\  "), []);
});

test("#123 next to markdown and code, not inside a URL or a word", () => {
  assert.deepEqual(refs("See #12, (#13), [#14](url), `#15`, issue#16, #17."), ["#12", "#13", "#14", "#15", "#17"]);
  assert.deepEqual(refs("**#18** ~#20~ \"#21\" '#22' <#23>"), ["#18", "#20", "#21", "#22", "#23"]);
  assert.deepEqual(refs("https://github.com/o/r/pull/7#issuecomment-123 and #8"), ["#8"]);
  assert.deepEqual(refs("http://localhost:3000/#12 file.html#3 a.md#L12"), []);
  assert.deepEqual(refs("fixes o/r#99, gh-12, &#123; &#x7b; C#7 F#2"), []);
  assert.deepEqual(refs("#!/bin/sh # 1 ## 2 #0 #01"), []);
});

// A CSS color of digits only is the commonest #123 that isn't one: `git diff` of a stylesheet
// labels and links every `color: #333`.
test("a stylesheet's digit-only colors aren't issues", () => {
  assert.deepEqual(refs("+  color: #333;"), []);
  assert.deepEqual(refs("   background: #666666;"), []);
  assert.deepEqual(refs("border: 1px solid #111"), []);
});

test("a line of 1000 candidates, and lines far too long, are read fast", () => {
  const ids = Array.from({ length: 1000 }, (_, i) => `a${((i * 2654435761) >>> 0).toString(16).padStart(8, "0")}`);
  const line = ids.join(" ");
  let t = performance.now();
  assert.deepEqual(refs(line), ids);
  // A hover looks around the pointer only.
  const near = findTerminalLinks(line, { start: 5000, end: 5001 }).filter((l) => l.kind === "commit");
  assert.ok(near.length > 0 && near.length <= 500, `${near.length} asked about at once`);
  assert.ok(performance.now() - t < 500);
  t = performance.now();
  for (const huge of ["9".repeat(300_000), "a".repeat(300_000), `${"ab12 ".repeat(60_000)}`, `${"#1 ".repeat(100_000)}`]) findTerminalLinks(huge, { start: 1000, end: 1001 });
  assert.ok(performance.now() - t < 1000, "windowed, not the whole line");
});

test("a repo's pull request and issue URLs in odd spellings", () => {
  const repos = ["https://github.com/ada/repo"];
  const url = (spec: string) => githubItem({ spec, kind: "url" }, repos);
  assert.deepEqual(url("https://github.com/ADA/Repo/issues/3"), { number: 3, pull: false, repo: repos[0] });
  assert.deepEqual(url("https://github.com/ada/repo/pull/3/commits/9a1c2e4"), { number: 3, pull: true, repo: repos[0] });
  assert.equal(url("https://github.com/ada/repo/pull/0"), null);
  assert.equal(url("https://github.com/ada/repo/pull/-3"), null);
  assert.equal(url("https://github.com/ada/repo/pull/99999999999"), null);
  assert.equal(url("https://github.com.evil.example/ada/repo/pull/3"), null);
  assert.equal(url("https://gist.github.com/ada/repo/pull/3"), null);
  assert.equal(url("http://github.com/ada/repo/pull/3"), null);
  assert.equal(url("https://github.com/ada/repo/discussions/3"), null);
  assert.equal(githubItem({ spec: "#3", kind: "issue" }, []), null);
});
