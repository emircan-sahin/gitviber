//! Which language a file is, for the diff and the file view alike: its name, a pattern of its
//! path, its extension, then its start (a shebang, an XML or HTML head, front matter, a JSON
//! document). Names are gpui-component's grammars; a few (`PLAIN`) have none and are only named,
//! and "text" is neither.

use std::borrow::Cow;

use gpui_kit::component::highlighter::{GrammarConfig, LanguageRegistry};

use crate::data::DiffData;
use crate::source::DiffSource;

/// Config files that are JSON or YAML, as their author chose: the text says which.
const JSON_OR_YAML: &str = "json|yaml";

/// Languages without a grammar here, named for the status bar and drawn plain.
const PLAIN: &[&str] = &["dockerfile", "ignore", "ini", "perl", "r"];

/// Exact file names, lowercase.
const NAMES: &[(&str, &str)] = &[
    (".arcconfig", "json"),
    (".babelrc", "json"),
    (".bash_aliases", "bash"),
    (".bash_login", "bash"),
    (".bash_logout", "bash"),
    (".bash_profile", "bash"),
    (".bashrc", "bash"),
    (".clang-format", "yaml"),
    (".clang-tidy", "yaml"),
    (".clinerules", "markdown"),
    (".condarc", "yaml"),
    (".coveragerc", "ini"),
    (".cursorrules", "markdown"),
    (".devcontainer.json", "json"),
    (".dockerignore", "ignore"),
    (".editorconfig", "ini"),
    (".ember-cli", "json"),
    (".envrc", "bash"),
    (".eslintignore", "ignore"),
    (".eslintrc", JSON_OR_YAML),
    (".flake8", "ini"),
    (".gcloudignore", "ignore"),
    (".gitattributes", "ini"),
    (".gitconfig", "ini"),
    (".gitignore", "ignore"),
    (".gitmodules", "ini"),
    (".goosehints", "markdown"),
    (".hintrc", "json"),
    (".irbrc", "ruby"),
    (".jsbeautifyrc", "json"),
    (".jscsrc", "json"),
    (".jshintrc", "json"),
    (".justfile", "make"),
    (".lintstagedrc", JSON_OR_YAML),
    (".luacheckrc", "lua"),
    (".markdownlintrc", JSON_OR_YAML),
    (".mocharc", JSON_OR_YAML),
    (".npmignore", "ignore"),
    (".npmrc", "ini"),
    (".prettierignore", "ignore"),
    (".prettierrc", JSON_OR_YAML),
    (".profile", "bash"),
    (".pryrc", "ruby"),
    (".pylintrc", "ini"),
    (".pypirc", "ini"),
    (".pythonrc", "python"),
    (".releaserc", JSON_OR_YAML),
    (".roorules", "markdown"),
    (".slugignore", "ignore"),
    (".stylelintignore", "ignore"),
    (".stylelintrc", JSON_OR_YAML),
    (".swcrc", "json"),
    (".vscodeignore", "ignore"),
    (".watchmanconfig", "json"),
    (".windsurfrules", "markdown"),
    (".yamllint", "yaml"),
    (".zlogin", "bash"),
    (".zlogout", "bash"),
    (".zprofile", "bash"),
    (".zshenv", "bash"),
    (".zshrc", "bash"),
    ("apkbuild", "bash"),
    ("appfile", "ruby"),
    ("babel.config.json", "json"),
    ("brewfile", "ruby"),
    ("bun.lock", "json"),
    ("capfile", "ruby"),
    ("cargo.lock", "toml"),
    ("cmakelists.txt", "cmake"),
    ("containerfile", "dockerfile"),
    ("dangerfile", "ruby"),
    ("devcontainer.json", "json"),
    ("dockerfile", "dockerfile"),
    ("fastfile", "ruby"),
    ("gemfile", "ruby"),
    ("gemfile.lock", "text"),
    ("gitconfig", "ini"),
    ("gnumakefile", "make"),
    ("guardfile", "ruby"),
    ("jsconfig.json", "json"),
    ("justfile", "make"),
    ("makefile", "make"),
    ("pdm.lock", "toml"),
    ("pipfile", "toml"),
    ("pkgbuild", "bash"),
    ("podfile", "ruby"),
    ("podfile.lock", "yaml"),
    ("poetry.lock", "toml"),
    ("pubspec.lock", "yaml"),
    ("rakefile", "ruby"),
    ("sconscript", "python"),
    ("sconstruct", "python"),
    ("snakefile", "python"),
    ("tsconfig.json", "json"),
    ("typedoc.json", "json"),
    ("uv.lock", "toml"),
    ("vagrantfile", "ruby"),
    ("yarn.lock", "yaml"),
];

const EXTENSIONS: &[(&str, &str)] = &[
    ("astro", "astro"),
    // Obsidian's Bases.
    ("base", "yaml"),
    ("bash", "bash"),
    ("c", "c"),
    // Obsidian's JSON Canvas.
    ("canvas", "json"),
    ("cc", "cpp"),
    ("cfg", "ini"),
    ("cjs", "javascript"),
    ("cmake", "cmake"),
    ("code-snippets", "json"),
    ("code-workspace", "json"),
    ("conf", "ini"),
    ("containerfile", "dockerfile"),
    ("cpp", "cpp"),
    ("cs", "csharp"),
    ("csproj", "xml"),
    ("css", "css"),
    ("cts", "typescript"),
    ("cxx", "cpp"),
    ("diff", "diff"),
    ("dockerfile", "dockerfile"),
    ("ejs", "ejs"),
    ("entitlements", "xml"),
    ("env", "bash"),
    ("erb", "erb"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("fsproj", "xml"),
    ("gemspec", "ruby"),
    ("geojson", "json"),
    ("gitignore", "ignore"),
    ("go", "go"),
    ("gql", "graphql"),
    ("graphql", "graphql"),
    ("h", "c"),
    ("hh", "cpp"),
    ("hpp", "cpp"),
    ("htm", "html"),
    ("html", "html"),
    ("hxx", "cpp"),
    ("ini", "ini"),
    ("ipynb", "json"),
    ("java", "java"),
    ("js", "javascript"),
    ("json", "json"),
    ("json5", "json"),
    ("jsonc", "json"),
    ("jsonl", "json"),
    ("jsx", "javascript"),
    ("kt", "kotlin"),
    ("ksh", "bash"),
    ("kts", "kotlin"),
    // composer.lock, flake.lock, deno.lock…; the lock files that aren't JSON are named above.
    ("lock", "json"),
    ("lua", "lua"),
    ("mak", "make"),
    ("markdown", "markdown"),
    ("md", "markdown"),
    ("mdx", "markdown"),
    ("mjs", "javascript"),
    ("mk", "make"),
    ("mts", "typescript"),
    ("ndjson", "json"),
    ("patch", "diff"),
    ("php", "php"),
    ("phtml", "php"),
    ("pl", "perl"),
    ("plist", "xml"),
    ("pm", "perl"),
    ("podspec", "ruby"),
    ("proto", "proto"),
    ("py", "python"),
    ("pyi", "python"),
    ("pyw", "python"),
    ("r", "r"),
    ("rake", "ruby"),
    ("rb", "ruby"),
    ("resx", "xml"),
    ("rs", "rust"),
    ("ru", "ruby"),
    ("sc", "scala"),
    ("scala", "scala"),
    ("scss", "css"),
    ("sh", "bash"),
    ("storyboard", "xml"),
    ("svelte", "svelte"),
    ("svg", "xml"),
    ("swift", "swift"),
    ("toml", "toml"),
    ("ts", "typescript"),
    ("tsbuildinfo", "json"),
    ("tsx", "tsx"),
    ("vbproj", "xml"),
    ("vcxproj", "xml"),
    ("webmanifest", "json"),
    ("xaml", "xml"),
    ("xhtml", "html"),
    ("xib", "xml"),
    ("xlf", "xml"),
    ("xliff", "xml"),
    ("xml", "xml"),
    ("xsd", "xml"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("zig", "zig"),
    ("zsh", "bash"),
];

/// A shebang's program, its version digits off, as a language.
const INTERPRETERS: &[(&str, &str)] = &[
    ("ash", "bash"),
    ("bash", "bash"),
    ("bun", "typescript"),
    ("dash", "bash"),
    ("deno", "typescript"),
    ("elixir", "elixir"),
    ("ksh", "bash"),
    ("lua", "lua"),
    ("luajit", "lua"),
    ("make", "make"),
    ("node", "javascript"),
    ("nodejs", "javascript"),
    ("perl", "perl"),
    ("php", "php"),
    ("pypy", "python"),
    ("python", "python"),
    ("rscript", "r"),
    ("ruby", "ruby"),
    ("scala", "scala"),
    ("sh", "bash"),
    ("swift", "swift"),
    ("ts-node", "typescript"),
    ("tsx", "typescript"),
    ("zsh", "bash"),
];

/// What of a file's start is read for its language.
const HEAD: usize = 4096;

fn find(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// The language a file's path says, if any: its name, a pattern, its extension.
fn by_name(path: &str) -> Option<&'static str> {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if let Some(lang) = find(NAMES, name) {
        return Some(lang);
    }
    if name.starts_with("dockerfile.") || name.starts_with("containerfile.") {
        return Some("dockerfile");
    }
    if name == ".env" || name.starts_with(".env.") {
        return Some("bash");
    }
    let git_config = [".git/config", ".config/git/config"];
    if git_config
        .iter()
        .any(|c| lower == *c || lower.ends_with(&format!("/{c}")))
    {
        return Some("ini");
    }
    let (_, ext) = name.rsplit_once('.')?;
    find(EXTENSIONS, ext)
}

/// The language for `path`, with its text (or its start) to read when the name says nothing.
pub fn language_for(path: &str, text: Option<&str>) -> &'static str {
    language_with(path, || text.map(Cow::Borrowed))
}

/// `language_for`, asking for the text only when the name doesn't decide. Text past `HEAD`
/// bytes isn't read; more than that says the start is cut off.
pub fn language_with<'a>(path: &str, text: impl FnOnce() -> Option<Cow<'a, str>>) -> &'static str {
    let named = by_name(path);
    if let Some(lang) = named.filter(|l| *l != JSON_OR_YAML) {
        return lang;
    }
    let Some(text) = text() else {
        return if named.is_some() { "yaml" } else { "text" };
    };
    let mut end = text.len().min(HEAD);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = text[..end].trim_start_matches('\u{feff}');
    let cut = text.len() > HEAD;
    if named.is_some() {
        return if is_json(head, cut) { "json" } else { "yaml" };
    }
    sniff(head, cut).unwrap_or("text")
}

/// The language a file's start says, or None.
fn sniff(head: &str, cut: bool) -> Option<&'static str> {
    if let Some(rest) = head.strip_prefix("#!") {
        return shebang(rest.lines().next().unwrap_or(""));
    }
    let start = head.trim_start();
    let lower: String = start
        .chars()
        .take(32)
        .collect::<String>()
        .to_ascii_lowercase();
    if word(start, "<?xml") || word(&lower, "<svg") {
        return Some("xml");
    }
    let doctype = lower
        .strip_prefix("<!doctype")
        .is_some_and(|r| r.starts_with(char::is_whitespace) && word(r.trim_start(), "html"));
    if doctype || word(&lower, "<html") {
        return Some("html");
    }
    let first = head.split('\n').next().unwrap_or("");
    if word(first, "%YAML") {
        return Some("yaml");
    }
    if is_fence(first) {
        // A closed `---` block with more after it is front matter; else a YAML document.
        return Some(if front_matter(head) {
            "markdown"
        } else {
            "yaml"
        });
    }
    is_json(head, cut).then_some("json")
}

/// Whether `s` starts with `w` as a whole word.
fn word(s: &str, w: &str) -> bool {
    s.strip_prefix(w).is_some_and(|r| {
        !r.chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

/// `---`, then maybe spaces and a carriage return.
fn is_fence(line: &str) -> bool {
    line.strip_prefix("---").is_some_and(|r| {
        r.trim_end_matches('\r')
            .trim_matches([' ', '\t'])
            .is_empty()
    })
}

/// A `---` line after the first with something past it.
fn front_matter(head: &str) -> bool {
    let Some(last) = head.rfind(|c: char| !c.is_whitespace()) else {
        return false;
    };
    let mut at = head.find('\n').map_or(head.len(), |i| i + 1);
    while at < head.len() {
        let end = head[at..].find('\n').map(|i| at + i);
        let Some(end) = end else { return false };
        if is_fence(&head[at..end]) {
            return end < last;
        }
        at = end + 1;
    }
    false
}

/// The program a shebang runs (`#!` taken off), as a language.
fn shebang(line: &str) -> Option<&'static str> {
    let mut words = line.split_whitespace();
    let program = words.next()?.rsplit('/').next()?;
    let command = if program == "env" {
        env_command(words)?
    } else {
        program
    };
    let command = command.to_ascii_lowercase();
    find(
        INTERPRETERS,
        command.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.'),
    )
}

/// What `env` runs: the first word that isn't an option, an option's value or an assignment.
fn env_command<'a>(mut words: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    while let Some(w) = words.next() {
        if ["-u", "--unset", "-C", "--chdir", "-P"].contains(&w) {
            words.next();
        } else if !w.starts_with('-') && !w.contains('=') {
            return Some(w);
        }
    }
    None
}

/// One JSON (or JSON with comments) token at the start of `s`: its length, or None.
fn json_token(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let first = *b.first()?;
    let ws = s.len() - s.trim_start().len();
    if ws > 0 {
        return Some(ws);
    }
    match first {
        b'/' if b.get(1) == Some(&b'/') => Some(s.find('\n').unwrap_or(s.len())),
        b'/' if b.get(1) == Some(&b'*') => Some(s[2..].find("*/").map_or(s.len(), |i| i + 4)),
        b'"' => {
            let mut i = 1;
            while i < b.len() {
                match b[i] {
                    b'"' => return Some(i + 1),
                    b'\n' => return None,
                    b'\\' if b.get(i + 1).is_some_and(|&c| c != b'\n') => i += 2,
                    b'\\' => return None,
                    _ => i += 1,
                }
            }
            // A string the head cut off.
            Some(b.len())
        }
        b'-' | b'0'..=b'9' => {
            let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
            let mut i = usize::from(first == b'-');
            match b.get(i) {
                Some(b'0') => i += 1,
                Some(b'1'..=b'9') => i += digits(i),
                _ => return None,
            }
            if b.get(i) == Some(&b'.') && digits(i + 1) > 0 {
                i += 1 + digits(i + 1);
            }
            if matches!(b.get(i), Some(b'e' | b'E')) {
                let sign = usize::from(matches!(b.get(i + 1), Some(b'+' | b'-')));
                let n = digits(i + 1 + sign);
                if n > 0 {
                    i += 1 + sign + n;
                }
            }
            Some(i)
        }
        b'{' | b'}' | b'[' | b']' | b':' | b',' => Some(1),
        _ => ["true", "false", "null"]
            .into_iter()
            .find(|w| s.starts_with(w))
            .map(str::len),
    }
}

/// Whether `head` starts an object or array document, checked token by token against JSON's
/// grammar, so `[ -z "$X" ]`, `[section]`, `[1]: url` and `{{ template }}` don't pass. A head
/// `cut` off mid-document still counts. Comments are allowed: one grammar colors both.
fn is_json(head: &str, cut: bool) -> bool {
    #[derive(PartialEq)]
    enum Want {
        Doc,
        Value,
        Item,
        Key,
        Colon,
        Next,
    }
    let mut want = Want::Doc;
    let mut stack = Vec::new();
    let mut i = 0;
    while i < head.len() {
        let rest = &head[i..];
        let Some(n) = json_token(rest) else {
            // A token the head cut off (`tru`, `1.`) isn't a reason to say no.
            return cut && want != Want::Doc && rest.len() < 8;
        };
        i += n;
        let c = rest.as_bytes()[0];
        if c == b'/' || rest.starts_with(char::is_whitespace) {
            continue;
        }
        if want != Want::Doc && stack.is_empty() {
            // Something after the document.
            return false;
        }
        let close = c == b'}' || c == b']';
        let item = if c == b'}' { Want::Key } else { Want::Item };
        if close && (want == Want::Next || want == item) {
            if stack.pop() != Some(if c == b'}' { b'{' } else { b'[' }) {
                return false;
            }
            want = Want::Next;
        } else if want == Want::Colon {
            if c != b':' {
                return false;
            }
            want = Want::Value;
        } else if want == Want::Next {
            if c != b',' {
                return false;
            }
            want = if stack.last() == Some(&b'{') {
                Want::Key
            } else {
                Want::Item
            };
        } else if want == Want::Key {
            if c != b'"' {
                return false;
            }
            want = Want::Colon;
        } else if c == b'{' || c == b'[' {
            stack.push(c);
            want = if c == b'{' { Want::Key } else { Want::Item };
        } else if want == Want::Doc || close || c == b':' || c == b',' {
            return false;
        } else {
            // A string, a number or a literal.
            want = Want::Next;
        }
    }
    want != Want::Doc
}

/// A language's name for people (the status bar): "Rust", "Plain Text".
pub fn language_label(lang: &str) -> &str {
    match lang {
        "text" => "Plain Text",
        "astro" => "Astro",
        "bash" => "Shell",
        "c" => "C",
        "cmake" => "CMake",
        "cpp" => "C++",
        "csharp" => "C#",
        "css" => "CSS",
        "diff" => "Diff",
        "dockerfile" => "Dockerfile",
        "ejs" => "EJS",
        "elixir" => "Elixir",
        "erb" => "ERB",
        "go" => "Go",
        "graphql" => "GraphQL",
        "html" => "HTML",
        "ignore" => "Ignore",
        "ini" => "INI",
        "java" => "Java",
        "javascript" => "JavaScript",
        "json" => "JSON",
        "kotlin" => "Kotlin",
        "lua" => "Lua",
        "make" => "Makefile",
        "markdown" => "Markdown",
        "perl" => "Perl",
        "php" => "PHP",
        "proto" => "Protocol Buffers",
        "python" => "Python",
        "r" => "R",
        "ruby" => "Ruby",
        "rust" => "Rust",
        "scala" => "Scala",
        "svelte" => "Svelte",
        "swift" => "Swift",
        "toml" => "TOML",
        "tsx" => "TSX",
        "typescript" => "TypeScript",
        "xml" => "XML",
        "yaml" => "YAML",
        "zig" => "Zig",
        other => other,
    }
}

/// Code fence words that aren't a grammar's own name, so ```js and ```sh color too; and the
/// languages above that only borrow a grammar.
const ALIASES: &[(&str, &str)] = &[
    ("js", "javascript"),
    ("jsx", "javascript"),
    ("mjs", "javascript"),
    ("node", "javascript"),
    ("ts", "typescript"),
    ("py", "python"),
    ("python3", "python"),
    ("golang", "go"),
    ("rb", "ruby"),
    ("rs", "rust"),
    ("shell", "bash"),
    ("zsh", "bash"),
    ("console", "bash"),
    ("shellscript", "bash"),
    ("yml", "yaml"),
    ("htm", "html"),
    ("xml", "html"),
    ("svg", "html"),
    ("kt", "kotlin"),
    ("cs", "csharp"),
    ("c#", "csharp"),
    ("hpp", "cpp"),
    ("cc", "cpp"),
    ("h", "c"),
    ("md", "markdown"),
    ("patch", "diff"),
    ("ex", "elixir"),
    ("gql", "graphql"),
];

/// Registers the aliases with gpui-component's languages, and the `PLAIN` ones as text, once.
pub fn register_aliases() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let registry = LanguageRegistry::singleton();
        for (alias, name) in ALIASES {
            if registry.language(alias).is_none()
                && let Some(config) = registry.language(name)
            {
                registry.register(alias, &config);
            }
        }
        for name in PLAIN {
            if registry.language(name).is_none() {
                registry.register(name, &GrammarConfig::plain(*name));
            }
        }
    });
}

/// The start of a rope as `language_with` reads it: its first chunk when that covers it.
fn rope_head(rope: &gpui_kit::base::input::Rope) -> Cow<'_, str> {
    let mut chunks = rope.chunks();
    let first = chunks.next().unwrap_or("");
    if first.len() >= rope.len().min(HEAD + 1) {
        return Cow::Borrowed(first);
    }
    let mut head = first.to_string();
    for chunk in chunks {
        head.push_str(chunk);
        if head.len() > HEAD {
            break;
        }
    }
    Cow::Owned(head)
}

/// The language a diff of `source` is colored as: its host's word for it, else what the shown
/// side's path and start say.
pub(crate) fn of_source<'a>(source: &'a DiffSource, data: &DiffData) -> &'a str {
    if let Some(lang) = &source.language {
        return lang;
    }
    let (path, side) = if data.new.exists {
        (source.path.as_str(), &data.new)
    } else {
        (source.original_path(), &data.old)
    };
    language_with(path, || Some(rope_head(&side.rope)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_name_extension_then_shebang() {
        assert_eq!(language_for("crates/core/src/patch.rs", None), "rust");
        assert_eq!(language_for("web/App.TSX", None), "tsx");
        assert_eq!(language_for("Makefile", None), "make");
        assert_eq!(language_for("x/Cargo.lock", None), "toml");
        assert_eq!(language_for(".env.local", None), "bash");
        assert_eq!(
            language_for("bin/tool", Some("#!/usr/bin/env -S python3 -u\nprint()")),
            "python"
        );
        assert_eq!(language_for("bin/run", Some("#!/bin/zsh\n")), "bash");
        assert_eq!(language_for("README", Some("hello")), "text");
        assert_eq!(language_for("notes.unknown", None), "text");
        assert_eq!(language_label(language_for("a.tsx", None)), "TSX");
        assert_eq!(language_label("text"), "Plain Text");
    }

    // The old app's cases. Its JSONC is "json" here (one grammar reads comments too), its dotenv
    // and shell scripts "bash".
    #[test]
    fn well_known_file_names() {
        let cases = [
            ("Dockerfile", "dockerfile"),
            ("docker/Dockerfile.dev", "dockerfile"),
            ("api.dockerfile", "dockerfile"),
            ("Makefile", "make"),
            ("rules.mk", "make"),
            ("Brewfile", "ruby"),
            ("Gemfile", "ruby"),
            (".gitignore", "ignore"),
            ("web/.dockerignore", "ignore"),
            (".editorconfig", "ini"),
            (".git/config", "ini"),
            ("sub/.git/config", "ini"),
            (".env", "bash"),
            (".env.example", "bash"),
            (".envrc", "bash"),
            (".zshrc", "bash"),
            (".babelrc", "json"),
            ("tsconfig.app.json", "json"),
            (".vscode/settings.json", "json"),
            ("package.json", "json"),
            ("Cargo.lock", "toml"),
            ("yarn.lock", "yaml"),
            (".cursorrules", "markdown"),
            ("CMakeLists.txt", "cmake"),
            ("src/App.tsx", "tsx"),
            ("Boards/Plan.canvas", "json"),
            ("Views/Reading.base", "yaml"),
            ("lib/util.mjs", "javascript"),
            ("README", "text"),
            ("composer.lock", "json"),
            // A markdown ```lock fence.
            ("x.lock", "json"),
            ("Gemfile.lock", "text"),
            ("C:\\repo\\Dockerfile", "dockerfile"),
        ];
        for (path, lang) in cases {
            assert_eq!(language_for(path, None), lang, "{path}");
        }
        assert_eq!(
            language_label(language_for("Dockerfile", None)),
            "Dockerfile"
        );
        assert_eq!(language_label("ignore"), "Ignore");
        assert_eq!(language_label("bash"), "Shell");
    }

    #[test]
    fn json_or_yaml_config_files_follow_their_content() {
        assert_eq!(
            language_for(".prettierrc", Some("{\n  \"semi\": false\n}\n")),
            "json"
        );
        assert_eq!(
            language_for(".prettierrc", Some("semi: false\nsingleQuote: true\n")),
            "yaml"
        );
        assert_eq!(
            language_for(".eslintrc", Some("// legacy\n{ \"root\": true }")),
            "json"
        );
        assert_eq!(language_for(".eslintrc", None), "yaml");
    }

    #[test]
    fn shebangs() {
        let cases = [
            ("#!/usr/bin/env node\nconsole.log(1)", "javascript"),
            ("#!/usr/bin/env -S deno run --allow-net\n", "typescript"),
            ("#!/usr/bin/python3.11\n", "python"),
            ("#!/usr/bin/env FOO=1 python3\n", "python"),
            ("#!/usr/bin/env -u VAR node\n", "javascript"),
            ("#!/usr/bin/env --unset VAR -C /tmp ruby\n", "ruby"),
            ("#!/usr/bin/env --unset=VAR python\n", "python"),
            ("#!/bin/bash\nset -e", "bash"),
            ("#!/bin/sh\n", "bash"),
            ("#! /bin/sh\r\n", "bash"),
            ("#!/usr/bin/env ruby\n", "ruby"),
            ("#!/usr/bin/env Rscript\n", "r"),
            ("#!/usr/bin/env unknown-thing\n", "text"),
            ("#!/usr/bin/env\n", "text"),
        ];
        for (text, lang) in cases {
            assert_eq!(language_for("bin/tool", Some(text)), lang, "{text}");
        }
    }

    #[test]
    fn content_sniffing_for_unknown_files() {
        let cases = [
            ("\u{feff}{\"a\": 1}", "json"),
            ("[\n  1, 2\n]", "json"),
            ("[{\"a\": [true, null, -1.5e3]}, \"x\"]", "json"),
            // Comments and trailing commas, as tsconfig files have them.
            ("{\n  // comment\n  \"a\": 1,\n}", "json"),
            ("[1,]", "json"),
            ("{\"url\": \"http://x//y\"}", "json"),
            ("<?xml version=\"1.0\"?>\n<rss/>", "xml"),
            ("<svg xmlns=\"http://www.w3.org/2000/svg\"/>", "xml"),
            ("<!DOCTYPE html>\n<html>", "html"),
            ("  <html lang=\"en\">", "html"),
            ("<htmlx>", "text"),
            ("---\nkey: value\n", "yaml"),
        ];
        for (text, lang) in cases {
            assert_eq!(language_for("data", Some(text)), lang, "{text}");
        }
        // The name wins over the content.
        assert_eq!(language_for("notes.md", Some("#!/bin/sh\n")), "markdown");
        assert_eq!(language_for("a.yaml", Some("{\"a\": 1}")), "yaml");
    }

    #[test]
    fn text_that_only_starts_like_json_stays_plain() {
        let cases = [
            "[metadata]\nname = x\n",
            "{{ name }}\n",
            "[ -z \"$X\" ] && exit 0\n",
            "[[ -n $CI ]] || exit 1\n",
            "[1]: http://example.com\n",
            "[2.0.0] - 2024-01-01\n",
            "[]\nmore\n",
            "{\"a\": 1} trailing",
            "{ key: 1 }\n",
            "[01]\n",
            "[\"line\nbreak\"]",
            "Remember the milk.\n",
            "\"just a string\"",
            "{\"a\" 1}",
            "[1}",
            "-",
        ];
        for text in cases {
            assert_eq!(language_for("NOTES", Some(text)), "text", "{text}");
        }
    }

    #[test]
    fn a_head_cut_off_mid_document_is_still_json() {
        let big = format!("[{}", "{\"name\": \"value\", \"n\": 12.5},".repeat(300));
        assert_eq!(language_for("data", Some(&big)), "json");
        let cut = format!("{}tr{}", &big[..4094], "x".repeat(100));
        assert_eq!(language_for("data", Some(&cut)), "json");
        // Only a cut can leave a token unfinished.
        assert_eq!(language_for("data", Some("[1, tr")), "text");
        // A head cut inside a character.
        let wide = format!("[\"{}\"]", "é".repeat(3000));
        assert_eq!(language_for("data", Some(&wide)), "json");
    }

    #[test]
    fn comments_cant_make_the_json_check_slow() {
        for unit in ["/* */\n", "/* * / */", "// c\n"] {
            let text = format!("{}x", unit.repeat(40));
            assert_eq!(language_for("data", Some(&text)), "text", "{unit}");
        }
        assert_eq!(language_for("data", Some("/* a */ /* b */\n{}")), "json");
        assert_eq!(language_for("data", Some("/* open")), "text");
        let t = std::time::Instant::now();
        let fences = format!("{}{}", "\n---\n".repeat(1000), " ".repeat(2000));
        language_for("page", Some(&fences));
        let dashes = format!("---\n{}", "---x\n".repeat(1000));
        language_for("page", Some(&dashes));
        assert!(t.elapsed() < std::time::Duration::from_millis(50));
    }

    #[test]
    fn front_matter_is_markdown_a_bare_yaml_document_is_yaml() {
        let cases = [
            ("---\ntitle: x\n---\n# Heading\n", "markdown"),
            ("---\r\ntitle: x\r\n---\r\n\r\nText\r\n", "markdown"),
            ("---\ntitle: x\n---\n\n", "yaml"),
            ("---\nkey: value\nlist:\n  - a\n", "yaml"),
            ("%YAML 1.2\n---\na: 1\n", "yaml"),
            ("--- \t\nk: v\n", "yaml"),
        ];
        for (text, lang) in cases {
            assert_eq!(language_for("post", Some(text)), lang, "{text:?}");
        }
    }

    #[test]
    fn the_text_is_read_only_when_the_name_says_nothing() {
        let asked = std::cell::Cell::new(false);
        let ask = || {
            asked.set(true);
            Some(Cow::Borrowed("{}"))
        };
        assert_eq!(language_with("a.rs", ask), "rust");
        assert!(!asked.get());
        assert_eq!(language_with("data", ask), "json");
        assert!(asked.get());
    }

    #[test]
    fn every_language_is_one_the_highlighter_knows() {
        register_aliases();
        let registry = LanguageRegistry::singleton();
        let all = NAMES
            .iter()
            .chain(EXTENSIONS)
            .chain(INTERPRETERS)
            .map(|(_, l)| *l)
            .filter(|l| *l != JSON_OR_YAML);
        for lang in all {
            assert!(registry.language(lang).is_some(), "{lang}");
            assert_ne!(language_label(lang), lang, "{lang} has a label");
        }
    }

    #[test]
    fn a_diff_names_its_shown_side() {
        use crate::source::Compare;
        let source = DiffSource::new(Compare::Unstaged, "bin/run").old_path(Some("run.rb".into()));
        let data = DiffData::from_texts("puts 1\n", "#!/bin/zsh\necho\n", &source);
        assert_eq!(of_source(&source, &data), "bash");
        let mut gone = DiffData::from_texts("puts 1\n", "", &source);
        gone.new.exists = false;
        assert_eq!(of_source(&source, &gone), "ruby");
        assert_eq!(of_source(&source.clone().language("rust"), &data), "rust");
        // A long file's start is gathered over its chunks.
        let long = format!("[{}]", "1, ".repeat(3000) + "1");
        let data = DiffData::from_texts("", &long, &DiffSource::new(Compare::Unstaged, "data"));
        assert_eq!(
            of_source(&DiffSource::new(Compare::Unstaged, "data"), &data),
            "json"
        );
    }
}
