import assert from "node:assert/strict";
import { test } from "node:test";
import type { Commit } from "../api/types.ts";
import { editPath, filesSelection, isComparePoint, onDisk, type Selection, selectionKey, selectionPath, vaultEditFile } from "./selection.ts";

test("two working-tree files compared make one tab per pair, the right one's path its own", () => {
  const ab = filesSelection("src/a.ts", "src/b.ts");
  assert.equal(selectionPath(ab), "src/b.ts");
  assert.equal(selectionKey(ab), selectionKey(filesSelection("src/a.ts", "src/b.ts")));
  // Another left side, or the sides swapped, is another comparison.
  assert.notEqual(selectionKey(ab), selectionKey(filesSelection("src/c.ts", "src/b.ts")));
  assert.notEqual(selectionKey(ab), selectionKey(filesSelection("src/b.ts", "src/a.ts")));
  // Not the file's own tab.
  assert.notEqual(selectionKey(ab), selectionKey({ kind: "file", path: "src/b.ts" }));
  assert.equal(onDisk(ab), false);
});

test("names with spaces, unicode and a leading dash keep their sides apart", () => {
  const odd = filesSelection("-dash $x.txt", "ünï code.txt");
  assert.equal(selectionPath(odd), "ünï code.txt");
  assert.ok(selectionKey(odd).includes("-dash $x.txt"));
});

const commit = (sha: string) => ({ sha, shortSha: sha.slice(0, 7), subject: "s", parents: [] }) as unknown as Commit;

test("a commit's whole list is its own tab, apart from the lists Changes keeps", () => {
  const a = selectionKey({ kind: "changes", list: "commit", commit: commit("a".repeat(40)) });
  assert.equal(a, selectionKey({ kind: "changes", list: "commit", commit: commit("a".repeat(40)), url: "https://example.test/c" }));
  assert.notEqual(a, selectionKey({ kind: "changes", list: "commit", commit: commit("b".repeat(40)) }));
  assert.notEqual(a, selectionKey({ kind: "changes", list: "unstaged" }));
  assert.equal(selectionKey({ kind: "changes", list: "branch" }), "changes::All Branch Changes");
  assert.equal(selectionPath({ kind: "changes", list: "commit", commit: commit("a".repeat(40)) }), "Commit aaaaaaa");
});

test("a range's list is told apart by its ends, and by being a PR's", () => {
  const range = { label: "main...feature", base: "1".repeat(40), head: "2".repeat(40) };
  const key = selectionKey({ kind: "changes", list: "range", range });
  assert.notEqual(key, selectionKey({ kind: "changes", list: "range", range: { ...range, head: "3".repeat(40) } }));
  assert.notEqual(key, selectionKey({ kind: "changes", list: "range", range: { ...range, number: 4 } }));
  assert.equal(selectionPath({ kind: "changes", list: "range", range }), "All Changes · main...feature");
  assert.equal(selectionPath({ kind: "changes", list: "range", range: { base: "1".repeat(40), head: "2".repeat(40) } }), "All Changes · 1111111..2222222");
});

test("the Compare screen is one tab, whichever two points it shows", () => {
  const side = (label: string) => ({ ref: `refs/heads/${label}`, label });
  const one = selectionKey({ kind: "compare", base: side("main"), head: side("a"), mergeBase: true });
  assert.equal(one, selectionKey({ kind: "compare", base: side("dev"), head: side("b"), mergeBase: false }));
  assert.equal(selectionPath({ kind: "compare", base: side("main"), head: side("a"), mergeBase: true }), "Compare");
});

test("a stored comparison with a side missing or misshapen isn't taken for one", () => {
  assert.ok(isComparePoint({ ref: "refs/heads/a", label: "a" }));
  for (const bad of [undefined, null, "a", {}, { ref: "refs/heads/a" }, { ref: 1, label: "a" }]) assert.equal(isComparePoint(bad), false);
});

test("a vault note's path comes back out of its edit key, at the vault's root or deep in it", () => {
  for (const vault of ["/Users/x/My Vault", "C:\\Notes", "/v/odd:name"]) {
    for (const path of ["Home.md", "Projects/Alpha/Plan.md", "Odd names/Note #1.md", "a:b.md"]) {
      const key = editPath({ kind: "vault", vault, path })!;
      assert.equal(vaultEditFile(vault, key), path);
    }
  }
});

test("a browser tab is one tab wherever its page goes, and no file", () => {
  const at = (url: string): Selection => ({ kind: "browser", id: "tab-1", url });
  assert.equal(selectionKey(at("http://localhost:5173/")), selectionKey(at("http://localhost:5173/docs")));
  assert.notEqual(selectionKey(at("http://localhost:5173/")), selectionKey({ kind: "browser", id: "tab-2", url: "http://localhost:5173/" }));
  assert.equal(selectionPath(at("http://localhost:5173/")), "localhost:5173");
  // What diffPairs' isFileSelection reads.
  assert.equal("file" in at("http://localhost:5173/"), false);
  assert.equal(editPath(at("http://localhost:5173/")), null);
  assert.equal(onDisk(at("http://localhost:5173/")), false);
});

test("a browser tab's key is its own, whatever its id or the other tabs' paths hold", () => {
  const ids = ["tab-1", "", ":", "::x", "browser::tab-1", "vault:a:b", "a/b", "ü"];
  const keys = ids.map((id) => selectionKey({ kind: "browser", id, url: "about:blank" }));
  assert.equal(new Set(keys).size, ids.length);
  const others: Selection[] = [
    { kind: "vault", vault: "browser", path: ":tab-1" },
    { kind: "vault", vault: "", path: "tab-1" },
  ];
  for (const s of others) assert.equal(keys.includes(selectionKey(s)), false, selectionKey(s));
  // The title is the page's last, and isn't part of who the tab is.
  assert.equal(selectionKey({ kind: "browser", id: "tab-1", url: "about:blank", title: "Docs" }), keys[0]);
  assert.equal(selectionPath({ kind: "browser", id: "tab-1", url: "about:blank" }), "New Tab");
});

test("a browser tab's device is shown with it, not part of who it is", () => {
  const plain: Selection = { kind: "browser", id: "tab-9", url: "http://localhost:5173/" };
  assert.equal(selectionKey({ ...plain, device: { name: "iPhone 16", rotated: true } }), selectionKey(plain));
  assert.equal(selectionPath({ ...plain, device: { name: "Responsive", w: 320, h: 640 } }), "localhost:5173");
});
