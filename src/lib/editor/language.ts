// Language detection the way VS Code does it: file name first, then extension, then the
// first line of content. Pure (no DOM, no React) so it runs under `node --test`.
import { bundledLanguagesInfo } from "shiki/langs";
import { basename } from "../path.ts";

/** VS Code's "Ignore" language isn't bundled with Shiki; `ignoreGrammar` defines it. */
export const IGNORE = "ignore";
// VS Code's grammar for .gitignore and friends: comments and negations are all it colors.
export const ignoreGrammar = {
  name: IGNORE,
  scopeName: "source.ignore",
  patterns: [
    { match: "^\\s*#.*", name: "comment.line.number-sign.ignore" },
    { match: "^\\s*!", name: "keyword.operator.negation.ignore" },
  ],
  repository: {},
};
// Config files that are JSON or YAML depending on what the author chose.
const JSON_OR_YAML = "json|yaml";

/** Exact file names (lowercase), from VS Code's built-in language contributions. */
const NAMES: Record<string, string> = {
  dockerfile: "docker",
  containerfile: "docker",
  makefile: "make",
  gnumakefile: "make",
  justfile: "just",
  ".justfile": "just",
  "cmakelists.txt": "cmake",
  jenkinsfile: "groovy",
  "nginx.conf": "nginx",
  ".htaccess": "apache",
  "httpd.conf": "apache",
  codeowners: "codeowners",
  commit_editmsg: "git-commit",
  merge_msg: "git-commit",
  "git-rebase-todo": "git-rebase",
  ssh_config: "ssh-config",
  sshd_config: "ssh-config",

  gemfile: "ruby",
  rakefile: "ruby",
  podfile: "ruby",
  brewfile: "ruby",
  vagrantfile: "ruby",
  fastfile: "ruby",
  appfile: "ruby",
  guardfile: "ruby",
  capfile: "ruby",
  dangerfile: "ruby",
  ".irbrc": "ruby",
  ".pryrc": "ruby",
  snakefile: "python",
  sconstruct: "python",
  sconscript: "python",
  ".pythonrc": "python",
  pipfile: "toml",

  ".bashrc": "shellscript",
  ".bash_profile": "shellscript",
  ".bash_login": "shellscript",
  ".bash_logout": "shellscript",
  ".bash_aliases": "shellscript",
  ".profile": "shellscript",
  ".zshrc": "shellscript",
  ".zshenv": "shellscript",
  ".zprofile": "shellscript",
  ".zlogin": "shellscript",
  ".zlogout": "shellscript",
  ".envrc": "shellscript",
  pkgbuild: "shellscript",
  apkbuild: "shellscript",
  ".vimrc": "viml",
  ".gvimrc": "viml",
  _vimrc: "viml",
  ".luacheckrc": "lua",

  ".gitignore": IGNORE,
  ".dockerignore": IGNORE,
  ".npmignore": IGNORE,
  ".prettierignore": IGNORE,
  ".eslintignore": IGNORE,
  ".stylelintignore": IGNORE,
  ".vscodeignore": IGNORE,
  ".gcloudignore": IGNORE,
  ".slugignore": IGNORE,
  ".gitattributes": "ini",
  ".gitconfig": "ini",
  ".gitmodules": "ini",
  gitconfig: "ini",
  ".editorconfig": "ini",
  ".npmrc": "ini",
  ".pypirc": "ini",
  ".flake8": "ini",
  ".pylintrc": "ini",
  ".coveragerc": "ini",

  ".babelrc": "jsonc",
  ".jshintrc": "jsonc",
  ".jscsrc": "jsonc",
  ".swcrc": "jsonc",
  ".hintrc": "jsonc",
  ".jsbeautifyrc": "jsonc",
  ".ember-cli": "jsonc",
  ".devcontainer.json": "jsonc",
  "devcontainer.json": "jsonc",
  "tsconfig.json": "jsonc",
  "jsconfig.json": "jsonc",
  "babel.config.json": "jsonc",
  "typedoc.json": "jsonc",
  ".watchmanconfig": "json",
  ".arcconfig": "json",
  ".prettierrc": JSON_OR_YAML,
  ".eslintrc": JSON_OR_YAML,
  ".stylelintrc": JSON_OR_YAML,
  ".lintstagedrc": JSON_OR_YAML,
  ".releaserc": JSON_OR_YAML,
  ".mocharc": JSON_OR_YAML,
  ".markdownlintrc": JSON_OR_YAML,
  ".clang-format": "yaml",
  ".clang-tidy": "yaml",
  ".yamllint": "yaml",
  ".condarc": "yaml",

  // Lock files: `.lock` alone says nothing about the format.
  "cargo.lock": "toml",
  "poetry.lock": "toml",
  "uv.lock": "toml",
  "pdm.lock": "toml",
  "yarn.lock": "yaml",
  "podfile.lock": "yaml",
  "pubspec.lock": "yaml",
  "bun.lock": "jsonc",
  "gemfile.lock": "text",

  // Instructions for AI coding agents are markdown without the extension.
  ".cursorrules": "markdown",
  ".windsurfrules": "markdown",
  ".clinerules": "markdown",
  ".roorules": "markdown",
  ".goosehints": "markdown",
};

/** Matched against the whole lowercase path, for names that follow a pattern. */
const PATTERNS: [RegExp, string][] = [
  [/(?:^|\/)(?:docker|container)file\.[^/]*$|\.(?:docker|container)file$/, "docker"],
  [/(?:^|\/)\.env(?:\.[^/]*)?$/, "dotenv"],
  [/(?:^|\/)[jt]sconfig\.[^/]*\.json$/, "jsonc"],
  [/(?:^|\/)\.vscode\/[^/]*\.json$/, "jsonc"],
  [/(?:^|\/)\.(?:git|config\/git)\/config$/, "ini"],
  [/(?:^|\/)\.ssh\/config$/, "ssh-config"],
];

const EXTENSIONS: Record<string, string> = {
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  htm: "html",
  xhtml: "html",
  h: "c",
  hpp: "cpp",
  cc: "cpp",
  m: "objective-c",
  mm: "objective-cpp",
  kt: "kotlin",
  kts: "kotlin",
  py: "python",
  pyi: "python",
  pyw: "python",
  rb: "ruby",
  gemspec: "ruby",
  podspec: "ruby",
  rake: "ruby",
  ru: "ruby",
  ksh: "shellscript",
  psm1: "powershell",
  psd1: "powershell",
  env: "dotenv",
  mk: "make",
  mak: "make",
  cfg: "ini",
  conf: "ini",
  patch: "diff",
  gitignore: IGNORE,
  svg: "xml",
  plist: "xml",
  entitlements: "xml",
  csproj: "xml",
  fsproj: "xml",
  vbproj: "xml",
  vcxproj: "xml",
  xaml: "xml",
  xib: "xml",
  storyboard: "xml",
  resx: "xml",
  xsd: "xml",
  xliff: "xml",
  xlf: "xml",
  ipynb: "json",
  webmanifest: "json",
  geojson: "json",
  tsbuildinfo: "json",
  ndjson: "jsonl",
  "code-workspace": "jsonc",
  "code-snippets": "jsonc",
  gradle: "groovy",
  tf: "hcl",
  // composer.lock, flake.lock, deno.lock…; the non-JSON lock files are listed by name above.
  lock: "json",
  jsonc: "jsonc",
  json5: "json5",
};

const byAlias = new Map<string, string>();
for (const info of bundledLanguagesInfo) {
  byAlias.set(info.id, info.id);
  for (const a of info.aliases ?? []) byAlias.set(a, info.id);
}

/**
 * Shiki language id for a file ("text" when unknown). Pass the file's `text` where it's
 * loaded: extensionless and ambiguous files are then decided by their first few KB.
 */
export function languageFor(path: string, text?: string): string {
  const byName = languageByName(path);
  if (byName && byName !== JSON_OR_YAML) return byName;
  if (text == null) return byName ? "yaml" : "text";
  const head = text.slice(0, HEAD).replace(/^﻿/, "");
  const truncated = text.length > HEAD;
  if (byName) return jsonKind(head, truncated) ? "jsonc" : "yaml";
  return sniff(head, truncated) ?? "text";
}

const HEAD = 4096;

function languageByName(path: string): string | undefined {
  const lower = path.toLowerCase();
  const name = basename(lower);
  if (NAMES[name]) return NAMES[name];
  for (const [re, lang] of PATTERNS) if (re.test(lower)) return lang;
  const dot = name.lastIndexOf(".");
  if (dot < 0) return undefined;
  const ext = name.slice(dot + 1);
  return EXTENSIONS[ext] ?? byAlias.get(ext);
}

// Interpreter (version suffix stripped) → language, for `#!` lines.
const INTERPRETERS: Record<string, string> = {
  node: "javascript",
  nodejs: "javascript",
  deno: "typescript",
  bun: "typescript",
  "ts-node": "typescript",
  tsx: "typescript",
  python: "python",
  pypy: "python",
  sh: "shellscript",
  bash: "shellscript",
  zsh: "shellscript",
  dash: "shellscript",
  ksh: "shellscript",
  ash: "shellscript",
  fish: "fish",
  ruby: "ruby",
  perl: "perl",
  php: "php",
  lua: "lua",
  luajit: "lua",
  rscript: "r",
  pwsh: "powershell",
  osascript: "applescript",
  make: "make",
  awk: "awk",
  gawk: "awk",
  tclsh: "tcl",
  julia: "julia",
  elixir: "elixir",
  swift: "swift",
  groovy: "groovy",
  scala: "scala",
  nu: "nushell",
  runhaskell: "haskell",
};

/** Language from the start of a file's content, or undefined when nothing is recognizable. */
function sniff(head: string, truncated: boolean): string | undefined {
  const shebang = /^#!\s*(\S+)([^\n]*)/.exec(head);
  if (shebang) {
    const prog = shebang[1].slice(shebang[1].lastIndexOf("/") + 1);
    const cmd = prog === "env" ? envCommand(shebang[2]) : prog;
    return INTERPRETERS[cmd.toLowerCase().replace(/[\d.]+$/, "")];
  }
  const start = head.trimStart();
  if (/^<\?xml\b/.test(start) || /^<svg\b/i.test(start)) return "xml";
  if (/^<!doctype\s+html\b|^<html\b/i.test(start)) return "html";
  const first = head.split("\n", 1)[0];
  if (/^%YAML\b/.test(first)) return "yaml";
  // A closed `---` block with more after it is front matter (markdown); otherwise a YAML document.
  if (/^---[ \t]*\r?$/.test(first)) return /\n---[ \t]*\r?\n\s*\S/.test(head) ? "markdown" : "yaml";
  return jsonKind(head, truncated);
}

// env options that take a value: `env -u VAR node`, `env -C dir python`.
const ENV_VALUE_OPTIONS = new Set(["-u", "--unset", "-C", "--chdir", "-P"]);

/** The program `#!/usr/bin/env …` runs: the first word that isn't an option or an assignment. */
function envCommand(args: string) {
  const words = args.split(/\s+/).filter(Boolean);
  for (let i = 0; i < words.length; i++) {
    if (ENV_VALUE_OPTIONS.has(words[i])) i++;
    else if (!words[i].startsWith("-") && !words[i].includes("=")) return words[i];
  }
  return "";
}

// One JSON(C) token per match. Every alternative is linear: comments can't run past `*/`,
// strings past a quote or newline. An unterminated comment or string may run to the end of the head.
const JSON_TOKEN = /\s+|\/\/[^\n]*|\/\*(?:[^*]|\*(?!\/))*(?:\*\/|$)|"(?:[^"\\\n]|\\.)*(?:"|$)|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null|[{}[\]:,]/y;

/**
 * "json" or "jsonc" (it has comments) when `head` is the start of an object or array document,
 * checked token by token against JSON's grammar, so `[ -z "$X" ]`, `[section]`, `[1]: url`
 * and `{{ template }}` don't pass. A head cut off mid-document still counts.
 */
function jsonKind(head: string, truncated: boolean): "json" | "jsonc" | undefined {
  // What may come next: a top-level value, an array item or `]`, a key or `}`, `:`, or `,`/close.
  let expect: "doc" | "value" | "item" | "key" | "colon" | "next" = "doc";
  const stack: string[] = [];
  let comments = false;
  for (let i = 0; i < head.length; ) {
    JSON_TOKEN.lastIndex = i;
    const t = JSON_TOKEN.exec(head)?.[0];
    // A token cut off by the head limit (`tru`, `1.`) isn't a reason to reject.
    if (!t) return truncated && expect !== "doc" && head.length - i < 8 ? (comments ? "jsonc" : "json") : undefined;
    i += t.length;
    const c = t[0];
    if (c === "/") comments = true;
    if (c === "/" || /\s/.test(c)) continue;
    if (expect !== "doc" && !stack.length) return undefined; // content after the document
    const close = c === "}" || c === "]";
    if (close && (expect === "next" || expect === (c === "}" ? "key" : "item"))) {
      if (stack.pop() !== (c === "}" ? "{" : "[")) return undefined;
      expect = "next";
    } else if (expect === "colon") {
      if (c !== ":") return undefined;
      expect = "value";
    } else if (expect === "next") {
      if (c !== ",") return undefined;
      expect = stack[stack.length - 1] === "{" ? "key" : "item";
    } else if (expect === "key") {
      if (c !== '"') return undefined;
      expect = "colon";
    } else if (c === "{" || c === "[") {
      stack.push(c);
      expect = c === "{" ? "key" : "item";
    } else if (expect === "doc" || close || c === ":" || c === ",") {
      return undefined;
    } else {
      expect = "next"; // a string, number or literal
    }
  }
  return expect === "doc" ? undefined : comments ? "jsonc" : "json";
}

const LABELS: Record<string, string> = { text: "Plain Text", [IGNORE]: "Ignore" };
const names = new Map(bundledLanguagesInfo.map((l) => [l.id, l.name]));

/** Human name for a language id, as VS Code shows it in the status bar. */
export function languageLabel(lang: string): string {
  return LABELS[lang] ?? names.get(lang) ?? lang;
}
