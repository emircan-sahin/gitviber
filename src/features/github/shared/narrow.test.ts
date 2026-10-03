import assert from "node:assert/strict";
import { test } from "node:test";
import type { Narrow } from "../../../lib/api/github.ts";
import { emptyText, isNarrowed, narrowKey, NO_CHOICE, parseChoice, scopesFor, withChoice } from "./narrow.ts";

const narrow = (n: Partial<Narrow>): Narrow => ({ scope: null, draft: null, labels: [], ...n });

test("pull requests offer a review chip, issues don't", () => {
  assert.deepEqual(scopesFor("pulls").map((s) => s.id), ["created", "assigned", "mentioned", "reviewRequested"]);
  assert.deepEqual(scopesFor("issues").map((s) => s.id), ["created", "assigned", "mentioned"]);
});

test("a saved choice is read back for its own kind", () => {
  const saved = { pulls: { scope: "reviewRequested", draft: true }, issues: { scope: "assigned" } };
  assert.deepEqual(parseChoice(saved, "pulls"), { scope: "reviewRequested", draft: true });
  assert.deepEqual(parseChoice(saved, "issues"), { scope: "assigned", draft: null });
});

test("anything unreadable, or not offered for the kind, is All", () => {
  for (const bad of [null, undefined, "x", 3, [], { pulls: "created" }, { pulls: { scope: "everyone", draft: "yes" } }]) {
    assert.deepEqual(parseChoice(bad, "pulls"), NO_CHOICE);
  }
  // An issue list was never asked for review requests or drafts: a hand-edited entry can't start it.
  assert.deepEqual(parseChoice({ issues: { scope: "reviewRequested", draft: true } }, "issues"), NO_CHOICE);
});

test("saving one kind's choice keeps the other's", () => {
  const first = withChoice(undefined, "pulls", { scope: "created", draft: null });
  const both = withChoice(first, "issues", { scope: "mentioned", draft: null });
  assert.deepEqual(parseChoice(both, "pulls"), { scope: "created", draft: null });
  assert.deepEqual(parseChoice(both, "issues"), { scope: "mentioned", draft: null });
  assert.deepEqual(withChoice("junk", "pulls", NO_CHOICE), { pulls: NO_CHOICE });
});

test("the cache key tells narrowings apart, and is empty for none", () => {
  assert.equal(narrowKey(narrow({})), "");
  assert.notEqual(narrowKey(narrow({ scope: "created" })), narrowKey(narrow({ scope: "assigned" })));
  assert.notEqual(narrowKey(narrow({ draft: true })), narrowKey(narrow({ draft: false })));
  assert.notEqual(narrowKey(narrow({ draft: false })), narrowKey(narrow({})));
  assert.notEqual(narrowKey(narrow({ scope: "created" })), narrowKey(narrow({ scope: "created", labels: ["bug"] })));
  // Both labels mean the same list whichever was picked first.
  assert.equal(narrowKey(narrow({ labels: ["bug", "ui"] })), narrowKey(narrow({ labels: ["ui", "bug"] })));
});

test("a narrowing is anything that cuts the list", () => {
  assert.equal(isNarrowed(narrow({})), false);
  assert.equal(isNarrowed(narrow({ draft: false })), true);
  assert.equal(isNarrowed(narrow({ labels: ["bug"] })), true);
});

test("an empty list names the filter", () => {
  assert.equal(emptyText("pulls", "open", narrow({})), "No open pull requests.");
  assert.equal(emptyText("pulls", "all", narrow({})), "No pull requests.");
  assert.equal(emptyText("pulls", "open", narrow({ scope: "assigned" })), "No open pull requests assigned to you.");
  assert.equal(emptyText("pulls", "closed", narrow({ scope: "reviewRequested", draft: false })), "No closed non-draft pull requests waiting for your review.");
  assert.equal(emptyText("issues", "open", narrow({ scope: "mentioned", labels: ["bug"] })), "No open issues that mention you with this label.");
  assert.equal(emptyText("issues", "all", narrow({ labels: ["bug", "ui"] })), "No issues with all these labels.");
  assert.equal(emptyText("pulls", "open", narrow({ draft: true })), "No open draft pull requests.");
});

test("labels that hold the separators don't share a cache key with other picks", () => {
  assert.notEqual(narrowKey(narrow({ labels: ["a,b"] })), narrowKey(narrow({ labels: ["a", "b"] })));
  assert.notEqual(narrowKey(narrow({ labels: ["a|b"] })), narrowKey(narrow({ labels: ["a", "b"] })));
  assert.notEqual(narrowKey(narrow({ labels: ["|draft|"] })), narrowKey(narrow({ draft: true, labels: [""] })));
  // The list's own key ends in its page: a name ending like one mustn't read as the next page.
  assert.notEqual(`pulls:origin:open:${narrowKey(narrow({ labels: ["x:1"] }))}:1`, `pulls:origin:open:${narrowKey(narrow({ labels: ["x"] }))}:1:1`);
});

test("labels with quotes, emoji and colons keep their own empty-list wording", () => {
  assert.equal(emptyText("issues", "open", narrow({ labels: ['say "hi"', "🐛 crash", "type: bug"] })), "No open issues with all these labels.");
});

test("a draft flag that isn't a boolean is dropped, the scope beside it kept", () => {
  assert.deepEqual(parseChoice({ pulls: { scope: "created", draft: "true" } }, "pulls"), { scope: "created", draft: null });
  assert.deepEqual(parseChoice({ pulls: { scope: "created", draft: 1 } }, "pulls"), { scope: "created", draft: null });
});
