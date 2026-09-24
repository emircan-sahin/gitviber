import assert from "node:assert/strict";
import { test } from "node:test";
import { findLinks, findTerminalLinks, indexFiles, join, loadAliases, parseJsonc, resolveLink, resolveTerminalLink, splitPosition } from "./links.ts";

const specs = (line: string, lang: string) => findLinks(line, lang).map((l) => [line.slice(l.start, l.end), l.kind]);

test("finds JS and TS module specifiers", () => {
  assert.deepEqual(specs(`import { a } from "./a";`, "typescript"), [["./a", "module"]]);
  assert.deepEqual(specs(`export * from '../b.js'`, "javascript"), [["../b.js", "module"]]);
  assert.deepEqual(specs(`} from "@/lib/utils";`, "tsx"), [["@/lib/utils", "module"]]);
  assert.deepEqual(specs(`import "./side-effect.css";`, "typescript"), [["./side-effect.css", "module"]]);
  assert.deepEqual(specs(`const m = await import("./lazy"), r = require('fs');`, "javascript"), [
    ["./lazy", "module"],
    ["fs", "module"],
  ]);
  assert.deepEqual(specs(`vi.mock("./api", () => ({}))`, "typescript"), [["./api", "module"]]);
  assert.deepEqual(specs(`const x = "not a path";`, "typescript"), []);
});

test("finds CSS, Rust, Python and markdown links", () => {
  assert.deepEqual(specs(`@import "tailwindcss";`, "css"), [["tailwindcss", "style"]]);
  assert.deepEqual(specs(`  background: url("../img/bg.png") no-repeat;`, "css"), [["../img/bg.png", "style"]]);
  assert.deepEqual(specs(`@use 'sass:math';`, "scss"), [["sass:math", "style"]]);
  assert.deepEqual(specs(`pub(crate) mod scenario_tests;`, "rust"), [["scenario_tests", "rust"]]);
  assert.deepEqual(specs(`mod tests {`, "rust"), []);
  assert.deepEqual(specs(`from ..core.models import User`, "python"), [["..core.models", "python"]]);
  assert.deepEqual(specs(`import os.path, pkg.util as u  # comment, x`, "python"), [
    ["os.path", "python"],
    ["pkg.util", "python"],
  ]);
  assert.deepEqual(specs(`See [the guide](docs/guide.md#setup "Guide") and ![logo](<assets/logo.png>).`, "markdown"), [
    ["docs/guide.md#setup", "doc"],
    ["assets/logo.png", "doc"],
  ]);
  assert.deepEqual(specs(`[ref]: ./CONTRIBUTING.md`, "markdown"), [["./CONTRIBUTING.md", "doc"]]);
  assert.deepEqual(specs(`<img src="assets/icon.png" width="64">`, "markdown"), [["assets/icon.png", "doc"]]);
  assert.deepEqual(specs(`path = "./fixtures/a.json"`, "toml"), [["./fixtures/a.json", "path"]]);
});

test("join stays inside the repo", () => {
  assert.equal(join("src/lib", "../features/./A.tsx"), "src/features/A.tsx");
  assert.equal(join("src", "../.."), null);
  assert.equal(join("", "a//b/"), "a/b");
});

const index = indexFiles([
  "src/lib/utils.ts",
  "src/lib/api.ts",
  "src/lib/monaco.ts",
  "src/lib/editor/index.tsx",
  "src/lib/types.d.ts",
  "src/lib/data.json",
  "src/features/A.tsx",
  "src/styles/_mixins.scss",
  "src/styles/base.css",
  "src/img/bg.png",
  "src-tauri/src/lib.rs",
  "src-tauri/src/git.rs",
  "src-tauri/src/git/blame.rs",
  "src-tauri/src/net/mod.rs",
  "app/core/__init__.py",
  "app/core/models.py",
  "app/api/views.py",
  "docs/guide.md",
  "docs/my file.md",
  "README.md",
]);
const resolve = (spec: string, kind: Parameters<typeof resolveLink>[0]["kind"], from: string, aliases = [{ pattern: "@/*", targets: ["src/*"] }]) => {
  const t = resolveLink({ start: 0, end: spec.length, spec, kind }, from, index, aliases);
  return t && "path" in t ? t.path : null;
};

test("resolves modules: extensions, index files, emitted names, aliases", () => {
  assert.equal(resolve("./utils", "module", "src/lib/api.ts"), "src/lib/utils.ts");
  assert.equal(resolve("./editor", "module", "src/lib/api.ts"), "src/lib/editor/index.tsx");
  assert.equal(resolve("./types", "module", "src/lib/api.ts"), "src/lib/types.d.ts");
  assert.equal(resolve("./data.json", "module", "src/lib/api.ts"), "src/lib/data.json");
  assert.equal(resolve("./utils.js", "module", "src/lib/api.ts"), "src/lib/utils.ts");
  assert.equal(resolve("../lib/monaco?worker", "module", "src/features/A.tsx"), "src/lib/monaco.ts");
  assert.equal(resolve("@/lib/utils", "module", "src/features/A.tsx"), "src/lib/utils.ts");
  assert.equal(resolve("@/features/A", "module", "src/lib/api.ts"), "src/features/A.tsx");
  assert.equal(resolve("react", "module", "src/lib/api.ts"), null);
  assert.equal(resolve("./missing", "module", "src/lib/api.ts"), null);
  assert.equal(resolve("../../../../x", "module", "src/lib/api.ts"), null);
});

test("resolves styles, docs, plain paths", () => {
  assert.equal(resolve("base.css", "style", "src/styles/app.css"), "src/styles/base.css");
  assert.equal(resolve("mixins", "style", "src/styles/app.scss"), "src/styles/_mixins.scss");
  assert.equal(resolve("../img/bg.png", "style", "src/styles/app.css"), "src/img/bg.png");
  assert.equal(resolve("tailwindcss", "style", "src/index.css"), null);
  assert.equal(resolve("data:image/png;base64,x", "style", "src/index.css"), null);
  assert.equal(resolve("docs/guide.md#setup", "doc", "README.md"), "docs/guide.md");
  assert.equal(resolve("../README.md", "doc", "docs/guide.md"), "README.md");
  assert.equal(resolve("/README.md", "doc", "docs/guide.md"), "README.md");
  assert.equal(resolve("my%20file.md", "doc", "docs/guide.md"), "docs/my file.md");
  assert.equal(resolve("https://example.com/README.md", "doc", "README.md"), null);
  assert.equal(resolve("./guide.md", "path", "docs/x.toml"), "docs/guide.md");
  assert.equal(resolve("./guide", "path", "docs/x.toml"), null);
});

test("resolves Rust modules and Python imports", () => {
  assert.equal(resolve("git", "rust", "src-tauri/src/lib.rs"), "src-tauri/src/git.rs");
  assert.equal(resolve("net", "rust", "src-tauri/src/lib.rs"), "src-tauri/src/net/mod.rs");
  assert.equal(resolve("blame", "rust", "src-tauri/src/git.rs"), "src-tauri/src/git/blame.rs");
  assert.equal(resolve("missing", "rust", "src-tauri/src/lib.rs"), null);
  assert.equal(resolve(".models", "python", "app/core/__init__.py"), "app/core/models.py");
  assert.equal(resolve("..core.models", "python", "app/api/views.py"), "app/core/models.py");
  assert.equal(resolve("..core", "python", "app/api/views.py"), "app/core/__init__.py");
  assert.equal(resolve("app.core.models", "python", "app/api/views.py"), "app/core/models.py");
  assert.equal(resolve("os.path", "python", "app/api/views.py"), null);
});

test("tsconfig aliases: the nearest folder's configs, through extends", async () => {
  const configs: Record<string, string> = {
    "tsconfig.json": `{ "files": [], "references": [{ "path": "./tsconfig.app.json" }] }`,
    "tsconfig.app.json": `{
      // Vite's layout
      "compilerOptions": { "paths": { "@/*": ["./src/*"], }, /* trailing comma */ },
    }`,
    "web/tsconfig.json": `{ "extends": "./tsconfig.base", "compilerOptions": { "strict": true } }`,
    "web/tsconfig.base.json": `{ "compilerOptions": { "baseUrl": "app", "paths": { "~/*": ["*"], "cfg": ["config/index.ts"] } } }`,
  };
  const idx = indexFiles([...Object.keys(configs), "src/a.ts", "web/app/b.ts"]);
  const read = async (p: string) => configs[p] ?? null;
  assert.deepEqual(await loadAliases("src/a.ts", idx, read), [{ pattern: "@/*", targets: ["src/*"] }]);
  assert.deepEqual(await loadAliases("web/app/b.ts", idx, read), [
    { pattern: "~/*", targets: ["web/app/*"] },
    { pattern: "cfg", targets: ["web/app/config/index.ts"] },
  ]);
  assert.deepEqual(await loadAliases("a.ts", indexFiles(["a.ts"]), read), []);
});

test("parseJsonc keeps comment-like text inside strings", () => {
  assert.deepEqual(parseJsonc(`{ "a": "http://x/*y*/", // c\n "b": [1,], }`), { a: "http://x/*y*/", b: [1] });
  assert.equal(parseJsonc(`{ nope`), null);
});

test("finds URLs, without the punctuation around them", () => {
  assert.deepEqual(specs(`// see https://github.com/a/b/issues/40.`, "typescript"), [["https://github.com/a/b/issues/40", "url"]]);
  assert.deepEqual(specs(`(docs: https://en.wikipedia.org/wiki/Tree_(graph_theory))`, "text"), [["https://en.wikipedia.org/wiki/Tree_(graph_theory)", "url"]]);
  assert.deepEqual(specs(`[GitHub CLI](https://cli.github.com), ok`, "markdown"), [["https://cli.github.com", "url"]]);
  assert.deepEqual(specs(`import x from "https://esm.sh/react";`, "typescript"), [["https://esm.sh/react", "url"]]);
  assert.deepEqual(specs(`const u = "http://localhost:1420/?fixture";`, "typescript"), [["http://localhost:1420/?fixture", "url"]]);
});

test("finds paths with a line and column", () => {
  assert.deepEqual(specs(`// moved to src/lib/api.ts:42, see there.`, "typescript"), [["src/lib/api.ts:42", "file"]]);
  assert.deepEqual(specs(`Fixed in src/lib/api.ts:42:7 and README.md:3.`, "markdown"), [
    ["src/lib/api.ts:42:7", "file"],
    ["README.md:3", "file"],
  ]);
  assert.deepEqual(specs(`src/a.ts(12,5): error TS2304`, "text"), [["src/a.ts(12,5)", "file"]]);
  assert.deepEqual(specs(`[the loop](docs/guide.md#L10-L20)`, "markdown"), [["docs/guide.md#L10-L20", "doc"]]);
  // Member access isn't a file; a bare name.ext only counts with a line (or in the terminal).
  assert.deepEqual(specs(`const p = e.target.position; api.ts`, "typescript"), []);
  assert.deepEqual(specs(`see api.ts:12`, "typescript"), [["api.ts:12", "file"]]);
  assert.deepEqual(splitPosition("src/a.ts:12:5"), { path: "src/a.ts", line: 12, column: 5 });
  assert.deepEqual(splitPosition("src/a.ts(12,5)"), { path: "src/a.ts", line: 12, column: 5 });
  assert.deepEqual(splitPosition("a.md#L10C3-L20"), { path: "a.md", line: 10, column: 3 });
  assert.deepEqual(splitPosition("src/a.ts"), { path: "src/a.ts" });
});

test("resolves paths with positions, from the file's folder or the root", () => {
  const at = (spec: string, kind: "file" | "doc" | "url", from: string, root = "") => resolveLink({ start: 0, end: spec.length, spec, kind }, from, index, [], root);
  assert.deepEqual(at("src/lib/api.ts:42:7", "file", "docs/guide.md"), { path: "src/lib/api.ts", line: 42, column: 7 });
  assert.deepEqual(at("utils.ts:3", "file", "src/lib/api.ts"), { path: "src/lib/utils.ts", line: 3, column: undefined });
  assert.deepEqual(at("/repo/README.md:1", "file", "src/lib/api.ts", "/repo"), { path: "README.md", line: 1, column: undefined });
  assert.equal(at("/elsewhere/README.md", "file", "src/lib/api.ts", "/repo"), null);
  assert.equal(at("src/lib/nope.ts:1", "file", "README.md"), null);
  assert.deepEqual(at("guide.md#L10", "doc", "docs/x.md"), { path: "docs/guide.md", line: 10, column: undefined });
  assert.deepEqual(at("https://x.com/a", "url", "README.md"), { url: "https://x.com/a" });
});

test("terminal output: URLs, paths from the shell's folder, bare names", () => {
  const term = (line: string) => findTerminalLinks(line).map((l) => [line.slice(l.start, l.end), l.kind]);
  assert.deepEqual(term(`  ➜  Local:   http://localhost:1420/`), [["http://localhost:1420/", "url"]]);
  assert.deepEqual(term(`\tmodified:   src/lib/api.ts`), [["src/lib/api.ts", "file"]]);
  assert.deepEqual(term(`src/lib/api.ts:42:7: error: nope`), [["src/lib/api.ts:42:7", "file"]]);
  assert.deepEqual(term(`README.md  package.json  src`), [
    ["README.md", "file"],
    ["package.json", "file"],
  ]);
  assert.deepEqual(term(`  --> src-tauri/src/git.rs:1465:5`), [["src-tauri/src/git.rs:1465:5", "file"]]);
  assert.deepEqual(term(`v1.2.3 released, 3.5s`), []);
  const open = (spec: string, cwd: string | null, root = "/repo") =>
    resolveTerminalLink({ start: 0, end: spec.length, spec, kind: spec.startsWith("http") ? "url" : "file" }, cwd, index, root);
  assert.deepEqual(open("api.ts:4", "src/lib"), { path: "src/lib/api.ts", line: 4, column: undefined });
  assert.deepEqual(open("src/lib/api.ts", "src-tauri"), { path: "src/lib/api.ts", line: undefined, column: undefined });
  assert.deepEqual(open("/repo/docs/guide.md", null), { path: "docs/guide.md", line: undefined, column: undefined });
  assert.equal(open("src/lib/api.ts", null), null, "a shell outside the repo: its relative paths aren't the repo's");
  assert.equal(open("package.json", ""), null);
});
