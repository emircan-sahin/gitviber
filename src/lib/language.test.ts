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
  assert.equal(languageFor("settings", '{\n  // comment\n  "a": 1\n}'), "jsonc");
  assert.equal(languageFor("x.lock", '{\n  "version": 3\n}'), "json");
  assert.equal(languageFor("feed", '<?xml version="1.0"?>\n<rss/>'), "xml");
  assert.equal(languageFor("page", "<!DOCTYPE html>\n<html>"), "html");
  assert.equal(languageFor("config", "---\nkey: value\n"), "yaml");
  // Not JSON: an INI section, a template, prose.
  assert.equal(languageFor("setup", "[metadata]\nname = x\n"), "text");
  assert.equal(languageFor("tpl", "{{ name }}\n"), "text");
  assert.equal(languageFor("NOTES", "Remember the milk.\n"), "text");
  // The name wins over content, as in VS Code.
  assert.equal(languageFor("notes.md", "#!/bin/sh\n"), "markdown");
});

test("labels", () => {
  assert.equal(languageLabel("text"), "Plain Text");
  assert.equal(languageLabel("ignore"), "Ignore");
  assert.equal(languageLabel("docker"), "Dockerfile");
  assert.equal(languageLabel("shellscript"), "Shell");
});
