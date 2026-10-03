// Link resolution in vaults as people keep them: big, with odd names and the same name in many folders.
import assert from "node:assert/strict";
import { test } from "node:test";
import { linkIndex, resolveLink } from "./links.ts";

test("a 20,000-note vault indexes and resolves every link quickly", () => {
  const files = Array.from({ length: 20000 }, (_, i) => `Area ${i % 40}/Topic ${i % 400}/Note ${i}.md`);
  const start = performance.now();
  const index = linkIndex(files);
  for (let i = 0; i < 20000; i++) assert.equal(resolveLink(index, `Note ${i}`, "Home.md"), files[i]);
  const took = performance.now() - start;
  assert.ok(took < 1000, `${Math.round(took)} ms`);
  // The same list again is the same index (it's built once per file list).
  assert.equal(linkIndex(files), index);
});

test("odd names: #, ^, emoji, spaces, quotes, Unicode forms", () => {
  const index = linkIndex(["Notes/C# basics.md", "Notes/x^y.md", "🚀 Launch.md", "My Note.md", "Résumé.md", "Öğrenci Notları/İstanbul.md", "It's $5.md"]);
  assert.equal(resolveLink(index, "C# basics", "Home.md"), "Notes/C# basics.md");
  assert.equal(resolveLink(index, "x^y", "Home.md"), "Notes/x^y.md");
  assert.equal(resolveLink(index, "🚀 launch", "Home.md"), "🚀 Launch.md");
  assert.equal(resolveLink(index, "  My Note  ", "Home.md"), "My Note.md");
  assert.equal(resolveLink(index, "Résumé", "Home.md"), "Résumé.md");
  assert.equal(resolveLink(index, "Öğrenci Notları/İstanbul", "Home.md"), "Öğrenci Notları/İstanbul.md");
  assert.equal(resolveLink(index, "It's $5", "Home.md"), "It's $5.md");
});

test("a link can't climb out of the vault or name a folder", () => {
  const index = linkIndex(["Home.md", "a/b.md"]);
  assert.equal(resolveLink(index, "../../../etc/passwd", "a/b.md"), null);
  assert.equal(resolveLink(index, "../../Home", "a/b.md"), "Home.md");
  assert.equal(resolveLink(index, "/Home", "a/b.md"), "Home.md");
  assert.equal(resolveLink(index, "a", "Home.md"), null);
  assert.equal(resolveLink(index, "..", "a/b.md"), null);
});

test("the same name in many folders: the vault's top, then the note's own folder, then the shortest path", () => {
  const files = Array.from({ length: 2000 }, (_, i) => `Docs/Section ${i}/index.md`);
  const index = linkIndex(files);
  assert.equal(resolveLink(index, "index", "Docs/Section 7/Intro.md"), "Docs/Section 7/index.md");
  assert.equal(resolveLink(index, "index", "Elsewhere/x.md"), "Docs/Section 0/index.md");
  assert.equal(resolveLink(linkIndex([...files, "index.md"]), "index", "Docs/Section 7/Intro.md"), "index.md");
  assert.equal(resolveLink(index, "Section 1999/index", "Elsewhere/x.md"), "Docs/Section 1999/index.md");
});
