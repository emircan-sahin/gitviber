import assert from "node:assert/strict";
import { test } from "node:test";
import { languageFor, languageLabel } from "./language.ts";

test("well-known file names", () => {
  const cases: [string, string][] = [
    ["Dockerfile", "docker"],
    ["docker/Dockerfile.dev", "docker"],
    ["api.dockerfile", "docker"],
    ["Makefile", "make"],
    ["rules.mk", "make"],
    ["Brewfile", "ruby"],
    ["Gemfile", "ruby"],
    [".gitignore", "ignore"],
    ["web/.dockerignore", "ignore"],
    [".editorconfig", "ini"],
    [".git/config", "ini"],
    [".env", "dotenv"],
    [".env.example", "dotenv"],
    [".envrc", "shellscript"],
    [".zshrc", "shellscript"],
    [".babelrc", "jsonc"],
    ["tsconfig.app.json", "jsonc"],
    [".vscode/settings.json", "jsonc"],
    ["package.json", "json"],
    ["Cargo.lock", "toml"],
    ["yarn.lock", "yaml"],
    [".cursorrules", "markdown"],
    ["CMakeLists.txt", "cmake"],
    ["src/App.tsx", "tsx"],
    ["lib/util.mjs", "javascript"],
    ["README", "text"],
  ];
  for (const [path, lang] of cases) assert.equal(languageFor(path), lang, path);
});

test("JSON-or-YAML config files follow their content", () => {
  assert.equal(languageFor(".prettierrc", '{\n  "semi": false\n}\n'), "jsonc");
  assert.equal(languageFor(".prettierrc", "semi: false\nsingleQuote: true\n"), "yaml");
  assert.equal(languageFor(".eslintrc", "// legacy\n{ \"root\": true }"), "jsonc");
  assert.equal(languageFor(".eslintrc"), "yaml");
});

test("shebangs", () => {
  const cases: [string, string][] = [
    ["#!/usr/bin/env node\nconsole.log(1)", "javascript"],
    ["#!/usr/bin/env -S deno run --allow-net\n", "typescript"],
    ["#!/usr/bin/python3.11\n", "python"],
    ["#!/usr/bin/env FOO=1 python3\n", "python"],
    ["#!/usr/bin/env -u VAR node\n", "javascript"],
    ["#!/usr/bin/env --unset VAR -C /tmp ruby\n", "ruby"],
    ["#!/usr/bin/env --unset=VAR python\n", "python"],
    ["#!/bin/bash\nset -e", "shellscript"],
    ["#!/bin/sh\n", "shellscript"],
    ["#!/usr/bin/env ruby\n", "ruby"],
    ["#!/usr/bin/env Rscript\n", "r"],
    ["#!/usr/bin/env unknown-thing\n", "text"],
  ];
  for (const [text, lang] of cases) assert.equal(languageFor("bin/tool", text), lang, text);
});

test("content sniffing for unknown files", () => {
  assert.equal(languageFor("data", '﻿{"a": 1}'), "json");
  assert.equal(languageFor("data", "[\n  1, 2\n]"), "json");
  assert.equal(languageFor("data", '[{"a": [true, null, -1.5e3]}, "x"]'), "json");
  assert.equal(languageFor("settings", '{\n  // comment\n  "a": 1,\n}'), "jsonc");
  assert.equal(languageFor("settings", '{"url": "http://x//y"}'), "json");
  assert.equal(languageFor("feed", '<?xml version="1.0"?>\n<rss/>'), "xml");
  assert.equal(languageFor("page", "<!DOCTYPE html>\n<html>"), "html");
  assert.equal(languageFor("config", "---\nkey: value\n"), "yaml");
  // The name wins over content, as in VS Code.
  assert.equal(languageFor("notes.md", "#!/bin/sh\n"), "markdown");
});

test("lock files and markdown fences", () => {
  assert.equal(languageFor("composer.lock"), "json");
  assert.equal(languageFor("x.lock"), "json"); // MarkdownView's ```lock fence
  assert.equal(languageFor("yarn.lock"), "yaml");
});

test("text that only starts like JSON stays plain", () => {
  const cases = [
    "[metadata]\nname = x\n",
    "{{ name }}\n",
    '[ -z "$X" ] && exit 0\n',
    "[[ -n $CI ]] || exit 1\n",
    "[1]: http://example.com\n",
    "[2.0.0] - 2024-01-01\n",
    "[]\nmore\n",
    '{"a": 1} trailing',
    "{ key: 1 }\n",
    "[01]\n",
    '["line\nbreak"]',
    "Remember the milk.\n",
  ];
  for (const text of cases) assert.equal(languageFor("NOTES", text), "text", text);
});

test("a head cut off mid-document is still JSON", () => {
  const big = `[${'{"name": "value", "n": 12.5},'.repeat(300)}`;
  assert.equal(languageFor("data", big), "json");
  assert.equal(languageFor("data", `${big.slice(0, 4094)}tr${"x".repeat(100)}`), "json");
});

test("comments can't make the JSON check backtrack", () => {
  for (const unit of ["/* */\n", "/* * / */", "// c\n"]) {
    const text = unit.repeat(40) + "x";
    const t = performance.now();
    assert.equal(languageFor("data", text), "text");
    assert.ok(performance.now() - t < 50, unit);
  }
  assert.equal(languageFor("data", "/* a */ /* b */\n{}"), "jsonc");
  const t = performance.now();
  languageFor("page", "\n---\n".repeat(1000) + " ".repeat(2000));
  assert.ok(performance.now() - t < 50);
});

test("front matter is markdown, a bare YAML document is YAML", () => {
  assert.equal(languageFor("post", "---\ntitle: x\n---\n# Heading\n"), "markdown");
  assert.equal(languageFor("post", "---\r\ntitle: x\r\n---\r\n\r\nText\r\n"), "markdown");
  assert.equal(languageFor("config", "---\nkey: value\nlist:\n  - a\n"), "yaml");
  assert.equal(languageFor("config", "%YAML 1.2\n---\na: 1\n"), "yaml");
});

test("labels", () => {
  assert.equal(languageLabel("text"), "Plain Text");
  assert.equal(languageLabel("ignore"), "Ignore");
  assert.equal(languageLabel("docker"), "Dockerfile");
  assert.equal(languageLabel("shellscript"), "Shell");
});
