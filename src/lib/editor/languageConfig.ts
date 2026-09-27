// What typing follows in a language (VS Code's language-configuration.json): its comments for ⌘/,
// the brackets and quotes that close themselves, where Enter indents. Shiki colors the code but
// carries none of it, so it comes from Monaco's own language definitions, loaded as files open, and
// a table of comment styles for the languages Monaco lacks.
import * as monaco from "monaco-editor/editor/editor.api";

type Definition = () => Promise<{ conf: monaco.languages.LanguageConfiguration }>;

const typescript: Definition = () => import("monaco-editor/languages/definitions/typescript/typescript");
const javascript: Definition = () => import("monaco-editor/languages/definitions/javascript/javascript");
const cpp: Definition = () => import("monaco-editor/languages/definitions/cpp/cpp");
const html: Definition = () => import("monaco-editor/languages/definitions/html/html");
const shell: Definition = () => import("monaco-editor/languages/definitions/shell/shell");
// `#` comments, brackets and quotes: config files Monaco has no language for.
const ini: Definition = () => import("monaco-editor/languages/definitions/ini/ini");
const java: Definition = () => import("monaco-editor/languages/definitions/java/java");
const objectiveC: Definition = () => import("monaco-editor/languages/definitions/objective-c/objective-c");
const css: Definition = () => import("monaco-editor/languages/definitions/css/css");
const sql: Definition = () => import("monaco-editor/languages/definitions/sql/sql");
const hcl: Definition = () => import("monaco-editor/languages/definitions/hcl/hcl");
const systemverilog: Definition = () => import("monaco-editor/languages/definitions/systemverilog/systemverilog");
const xml: Definition = () => import("monaco-editor/languages/definitions/xml/xml");
const markdown: Definition = () => import("monaco-editor/languages/definitions/markdown/markdown");
const lua: Definition = () => import("monaco-editor/languages/definitions/lua/lua");
// Monaco's JSON mode keeps its configuration inside the mode, with the JSON worker.
const json: Definition = async () => ({
  conf: {
    wordPattern: /(-?\d*\.\d\w*)|([^[{\]}:",\s]+)/g,
    comments: { lineComment: "//", blockComment: ["/*", "*/"] },
    brackets: [
      ["{", "}"],
      ["[", "]"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}", notIn: ["string"] },
      { open: "[", close: "]", notIn: ["string"] },
      { open: '"', close: '"', notIn: ["string"] },
    ],
  },
});

/**
 * By Shiki's language id: Monaco's language of that name, or the nearest one. Written out, as the
 * bundler splits each into a file of its own only from a literal path.
 */
const DEFINITIONS: Record<string, Definition> = {
  typescript,
  tsx: typescript,
  "angular-ts": typescript,
  "glimmer-ts": typescript,
  "ts-tags": typescript,
  "vue-vine": typescript,
  javascript,
  jsx: javascript,
  "glimmer-js": javascript,
  c: cpp,
  glsl: cpp,
  hlsl: cpp,
  "objective-cpp": objectiveC,
  groovy: java,
  "nextflow-groovy": java,
  html,
  vue: html,
  "vue-html": html,
  svelte: html,
  astro: html,
  "angular-html": html,
  marko: html,
  "html-derivative": html,
  erb: html,
  json,
  jsonc: json,
  json5: json,
  jsonl: json,
  hjson: json,
  shellscript: shell,
  docker: shell,
  make: shell,
  just: shell,
  fish: shell,
  ini,
  toml: ini,
  dotenv: ini,
  ignore: ini,
  codeowners: ini,
  "ssh-config": ini,
  "git-commit": ini,
  desktop: ini,
  systemd: ini,
  nginx: ini,
  apache: ini,
  cmake: ini,
  nix: ini,
  terraform: hcl,
  proto: () => import("monaco-editor/languages/definitions/protobuf/protobuf"),
  rst: () => import("monaco-editor/languages/definitions/restructuredtext/restructuredtext"),
  "system-verilog": systemverilog,
  verilog: systemverilog,
  mipsasm: () => import("monaco-editor/languages/definitions/mips/mips"),
  dax: () => import("monaco-editor/languages/definitions/msdax/msdax"),
  plsql: sql,
  postcss: css,
  abap: () => import("monaco-editor/languages/definitions/abap/abap"),
  apex: () => import("monaco-editor/languages/definitions/apex/apex"),
  bat: () => import("monaco-editor/languages/definitions/bat/bat"),
  bicep: () => import("monaco-editor/languages/definitions/bicep/bicep"),
  clojure: () => import("monaco-editor/languages/definitions/clojure/clojure"),
  coffee: () => import("monaco-editor/languages/definitions/coffee/coffee"),
  cpp,
  csharp: () => import("monaco-editor/languages/definitions/csharp/csharp"),
  css,
  cypher: () => import("monaco-editor/languages/definitions/cypher/cypher"),
  dart: () => import("monaco-editor/languages/definitions/dart/dart"),
  elixir: () => import("monaco-editor/languages/definitions/elixir/elixir"),
  fsharp: () => import("monaco-editor/languages/definitions/fsharp/fsharp"),
  go: () => import("monaco-editor/languages/definitions/go/go"),
  graphql: () => import("monaco-editor/languages/definitions/graphql/graphql"),
  handlebars: () => import("monaco-editor/languages/definitions/handlebars/handlebars"),
  hcl,
  java,
  julia: () => import("monaco-editor/languages/definitions/julia/julia"),
  kotlin: () => import("monaco-editor/languages/definitions/kotlin/kotlin"),
  less: () => import("monaco-editor/languages/definitions/less/less"),
  liquid: () => import("monaco-editor/languages/definitions/liquid/liquid"),
  lua,
  luau: lua,
  markdown,
  mdc: markdown,
  mdx: () => import("monaco-editor/languages/definitions/mdx/mdx"),
  "objective-c": objectiveC,
  pascal: () => import("monaco-editor/languages/definitions/pascal/pascal"),
  perl: () => import("monaco-editor/languages/definitions/perl/perl"),
  php: () => import("monaco-editor/languages/definitions/php/php"),
  powerquery: () => import("monaco-editor/languages/definitions/powerquery/powerquery"),
  powershell: () => import("monaco-editor/languages/definitions/powershell/powershell"),
  pug: () => import("monaco-editor/languages/definitions/pug/pug"),
  python: () => import("monaco-editor/languages/definitions/python/python"),
  r: () => import("monaco-editor/languages/definitions/r/r"),
  razor: () => import("monaco-editor/languages/definitions/razor/razor"),
  ruby: () => import("monaco-editor/languages/definitions/ruby/ruby"),
  rust: () => import("monaco-editor/languages/definitions/rust/rust"),
  scala: () => import("monaco-editor/languages/definitions/scala/scala"),
  scheme: () => import("monaco-editor/languages/definitions/scheme/scheme"),
  scss: () => import("monaco-editor/languages/definitions/scss/scss"),
  solidity: () => import("monaco-editor/languages/definitions/solidity/solidity"),
  sparql: () => import("monaco-editor/languages/definitions/sparql/sparql"),
  sql,
  swift: () => import("monaco-editor/languages/definitions/swift/swift"),
  tcl: () => import("monaco-editor/languages/definitions/tcl/tcl"),
  twig: () => import("monaco-editor/languages/definitions/twig/twig"),
  typespec: () => import("monaco-editor/languages/definitions/typespec/typespec"),
  vb: () => import("monaco-editor/languages/definitions/vb/vb"),
  wgsl: () => import("monaco-editor/languages/definitions/wgsl/wgsl"),
  xml,
  xsl: xml,
  yaml: () => import("monaco-editor/languages/definitions/yaml/yaml"),
};

type Comments = monaco.languages.CommentRule;
const byStyle = (comments: Comments, ids: string) => ids.split(" ").map((id) => [id, comments] as const);

/** Comments for ⌘/ where the language above has none, or there's no language above. */
const COMMENTS: Record<string, Comments> = Object.fromEntries([
  ...byStyle(
    { lineComment: "#" },
    "awk berry bird2 crystal fluent gdscript gherkin git-rebase gn gnuplot http hurl hxml imba julia mojo nim nushell org po polar puppet raku rbs riscv rosmsg talonscript tasl tcl turtle vyper",
  ),
  ...byStyle(
    { lineComment: "//", blockComment: ["/*", "*/"] },
    "actionscript-3 c3 cadence chapel codeql d dream-maker gdshader genie hack haxe jison jsonnet kdl move nextflow odin openscad pkl qml ron sass shaderlab soy stata stylus templ typst v vala wit zenscript",
  ),
  ...byStyle({ lineComment: "//" }, "asciidoc ballerina bsl cairo cue gleam kusto moonbit prisma sdbl smithy zig"),
  ...byStyle({ lineComment: "--" }, "ada applescript elm haskell lean purescript surrealql vhdl"),
  ...byStyle({ lineComment: "%" }, "bibtex erlang latex matlab prolog tex"),
  ...byStyle({ lineComment: ";" }, "ahk ahk2 asm beancount common-lisp emacs-lisp fennel gdresource hy llvm logo nsis racket reg"),
  ...byStyle({ lineComment: ";;" }, "clarity wasm"),
  ...byStyle({ blockComment: ["(*", "*)"] }, "coq ocaml wolfram"),
  ...byStyle({ blockComment: ["{{--", "--}}"] }, "blade edge"),
  ["viml", { lineComment: '"' }],
  ["mermaid", { lineComment: "%%" }],
  ["fortran-free-form", { lineComment: "!" }],
  ["cobol", { lineComment: "*>" }],
  ["jinja", { blockComment: ["{#", "#}"] }],
  ["wikitext", { blockComment: ["<!--", "-->"] }],
]);

// Monaco's plain text one: brackets close themselves, quotes only surround a selection.
const PLAIN: monaco.languages.LanguageConfiguration = {
  brackets: [
    ["(", ")"],
    ["[", "]"],
    ["{", "}"],
  ],
  surroundingPairs: ["{}", "[]", "()", "<>", '""', "''", "``"].map(([open, close]) => ({ open, close })),
};

const configured = new Set<string>();
const commented = new Set<string>();

/** Whether ⌘/ has comments to toggle in `lang` (configured); else the key stays the app's. */
export const hasComments = (lang: string) => commented.has(lang);

/** Gives `lang` (registered with Monaco) its typing rules, once. */
export async function configure(lang: string) {
  if (configured.has(lang)) return;
  configured.add(lang);
  const definition = DEFINITIONS[lang];
  let conf = PLAIN;
  try {
    if (definition) conf = (await definition()).conf ?? PLAIN;
  } catch {
    // Its file didn't load (an update replaced the build): typing as in plain text.
  }
  const comments = COMMENTS[lang] ?? conf.comments;
  if (comments?.lineComment || comments?.blockComment) commented.add(lang);
  monaco.languages.setLanguageConfiguration(lang, { ...conf, comments });
}
