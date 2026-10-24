//! Which highlighter language a file is: its name, then its extension, then its first line (a
//! shebang). Names are gpui-component's grammars; "text" has none.

use gpui_kit::component::highlighter::LanguageRegistry;

/// Exact file names, lowercase.
const NAMES: &[(&str, &str)] = &[
    (".bash_aliases", "bash"),
    (".bash_profile", "bash"),
    (".bashrc", "bash"),
    (".clang-format", "yaml"),
    (".cursorrules", "markdown"),
    (".clinerules", "markdown"),
    (".envrc", "bash"),
    (".profile", "bash"),
    (".windsurfrules", "markdown"),
    (".zprofile", "bash"),
    (".zshenv", "bash"),
    (".zshrc", "bash"),
    ("brewfile", "ruby"),
    ("bun.lock", "json"),
    ("cargo.lock", "toml"),
    ("cmakelists.txt", "cmake"),
    ("devcontainer.json", "json"),
    ("gemfile", "ruby"),
    ("gnumakefile", "make"),
    ("jsconfig.json", "json"),
    ("makefile", "make"),
    ("pipfile", "toml"),
    ("pkgbuild", "bash"),
    ("podfile", "ruby"),
    ("podfile.lock", "yaml"),
    ("poetry.lock", "toml"),
    ("rakefile", "ruby"),
    ("tsconfig.json", "json"),
    ("uv.lock", "toml"),
    ("vagrantfile", "ruby"),
    ("yarn.lock", "yaml"),
];

const EXTENSIONS: &[(&str, &str)] = &[
    ("astro", "astro"),
    ("bash", "bash"),
    ("c", "c"),
    ("canvas", "json"),
    ("cc", "cpp"),
    ("cjs", "javascript"),
    ("cmake", "cmake"),
    ("cpp", "cpp"),
    ("cs", "csharp"),
    ("css", "css"),
    ("cts", "typescript"),
    ("cxx", "cpp"),
    ("diff", "diff"),
    ("ejs", "ejs"),
    ("erb", "erb"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("gemspec", "ruby"),
    ("go", "go"),
    ("gql", "graphql"),
    ("graphql", "graphql"),
    ("h", "c"),
    ("hh", "cpp"),
    ("hpp", "cpp"),
    ("htm", "html"),
    ("html", "html"),
    ("hxx", "cpp"),
    ("java", "java"),
    ("js", "javascript"),
    ("json", "json"),
    ("json5", "json"),
    ("jsonc", "json"),
    ("jsx", "javascript"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("lua", "lua"),
    ("markdown", "markdown"),
    ("md", "markdown"),
    ("mdx", "markdown"),
    ("mjs", "javascript"),
    ("mk", "make"),
    ("mts", "typescript"),
    ("patch", "diff"),
    ("php", "php"),
    ("phtml", "php"),
    ("podspec", "ruby"),
    ("proto", "proto"),
    ("py", "python"),
    ("pyi", "python"),
    ("pyw", "python"),
    ("rake", "ruby"),
    ("rb", "ruby"),
    ("rs", "rust"),
    ("sc", "scala"),
    ("scala", "scala"),
    ("scss", "css"),
    ("sh", "bash"),
    ("svelte", "svelte"),
    ("swift", "swift"),
    ("toml", "toml"),
    ("ts", "typescript"),
    ("tsx", "tsx"),
    ("xhtml", "html"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("zig", "zig"),
    ("zsh", "bash"),
];

/// The program a shebang runs, as a language.
fn shebang(first: &str) -> Option<&'static str> {
    let rest = first.strip_prefix("#!")?;
    let mut words = rest.split_whitespace();
    let mut program = words.next()?.rsplit('/').next()?;
    if program == "env" {
        program = words.find(|w| !w.starts_with('-') && !w.contains('='))?;
    }
    let program = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    Some(match program {
        "sh" | "bash" | "zsh" | "dash" | "ksh" => "bash",
        "python" => "python",
        "node" | "deno" | "bun" => "javascript",
        "ruby" => "ruby",
        "php" => "php",
        "lua" => "lua",
        _ => return None,
    })
}

/// The language for `path`, with the file's text to read a shebang from when the name says nothing.
pub fn language_for(path: &str, text: Option<&str>) -> &'static str {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    if let Some((_, lang)) = NAMES.iter().find(|(n, _)| *n == name) {
        return lang;
    }
    if name.starts_with(".env") {
        return "bash";
    }
    if let Some((_, ext)) = name.rsplit_once('.')
        && let Some((_, lang)) = EXTENSIONS.iter().find(|(e, _)| *e == ext)
    {
        return lang;
    }
    text.and_then(|t| t.lines().next())
        .and_then(shebang)
        .unwrap_or("text")
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
        "ejs" => "EJS",
        "elixir" => "Elixir",
        "erb" => "ERB",
        "go" => "Go",
        "graphql" => "GraphQL",
        "html" => "HTML",
        "java" => "Java",
        "javascript" => "JavaScript",
        "json" => "JSON",
        "kotlin" => "Kotlin",
        "lua" => "Lua",
        "make" => "Makefile",
        "markdown" => "Markdown",
        "php" => "PHP",
        "proto" => "Protocol Buffers",
        "python" => "Python",
        "ruby" => "Ruby",
        "rust" => "Rust",
        "scala" => "Scala",
        "svelte" => "Svelte",
        "swift" => "Swift",
        "toml" => "TOML",
        "tsx" => "TSX",
        "typescript" => "TypeScript",
        "yaml" => "YAML",
        "zig" => "Zig",
        other => other,
    }
}

/// Code fence words that aren't a grammar's own name, so ```js and ```sh color too.
const FENCE_ALIASES: &[(&str, &str)] = &[
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

/// Registers the fence aliases with gpui-component's languages, once.
pub fn register_aliases() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let registry = LanguageRegistry::singleton();
        for (alias, name) in FENCE_ALIASES {
            if registry.language(alias).is_none()
                && let Some(config) = registry.language(name)
            {
                registry.register(alias, &config);
            }
        }
    });
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
}
