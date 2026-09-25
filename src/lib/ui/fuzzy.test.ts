import assert from "node:assert/strict";
import { test } from "node:test";
import { fuzzyMatch, matchPath, prepareQuery } from "./fuzzy.ts";

const score = (q: string, text: string) => fuzzyMatch(prepareQuery(q), text)?.score ?? null;
const rank = (q: string, texts: string[], match = fuzzyMatch) =>
  texts
    .map((t) => ({ t, m: match(prepareQuery(q), t) }))
    .filter((x) => x.m)
    .sort((a, b) => b.m!.score - a.m!.score)
    .map((x) => x.t);

test("characters in order, ignoring case and spaces", () => {
  assert.notEqual(score("fe", "Focus Explorer"), null);
  assert.notEqual(score("focus explorer", "View: Focus Explorer"), null);
  assert.notEqual(score("GTPSH", "Git: Push"), null);
  assert.equal(score("ef", "Focus Explorer"), null);
  assert.equal(score("pushx", "Git: Push"), null);
  assert.deepEqual(fuzzyMatch("", "anything"), { score: 0, hits: [] });
});

test("word starts and runs rank first", () => {
  assert.deepEqual(rank("fe", ["Toggle Blame", "Refresh", "Focus Explorer"]), ["Focus Explorer", "Refresh"]);
  assert.deepEqual(rank("push", ["Git: Pull and Rebase and Squash", "Git: Push"]), ["Git: Push", "Git: Pull and Rebase and Squash"]);
  assert.deepEqual(rank("tt", ["Toggle Terminal", "Settings Tab"]), ["Toggle Terminal", "Settings Tab"]);
});

test("hits mark the matched characters", () => {
  assert.deepEqual(fuzzyMatch("fe", "Focus Explorer")?.hits, [0, 6]);
  assert.deepEqual(fuzzyMatch("pu", "Git: Push")?.hits, [5, 6]);
});

test("paths: the file name wins", () => {
  assert.deepEqual(rank("fuzzy", ["src/fuzzy-old/index.ts", "src/lib/fuzzy.ts"], matchPath), ["src/lib/fuzzy.ts", "src/fuzzy-old/index.ts"]);
  assert.deepEqual(matchPath("fz", "src/lib/fuzzy.ts")?.hits, [8, 10]);
  // Falls back to the whole path for a query that names folders.
  assert.notEqual(matchPath(prepareQuery("lib fuz"), "src/lib/fuzzy.ts"), null);
  assert.equal(matchPath("zzz", "src/lib/fuzzy.ts"), null);
});
