//! Go to Definition without language servers. tree-sitter reads what each name in a file defines
//! (definitions/*.scm); a name resolves to the definition in scope where it's used, else to the
//! top-level ones in other files `git grep` finds it in. By name, so it can offer several; there's
//! no index to build or keep, and a commit's tree reads the same as the worktree.

use crate::{fs, git};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// The file, by its path in `rev` (the worktree when none).
    pub path: String,
    /// Its text as shown: a diff's old side isn't the file on disk.
    pub text: String,
    /// 1-based line and 0-based UTF-16 column, as the editor counts.
    pub line: u32,
    pub column: u32,
    /// A commit's tree to look in (see git::validate_tree_rev); none: the worktree.
    pub rev: Option<String>,
}

/// Where a name is defined: its line (1-based) and columns (0-based, UTF-16).
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub end_column: u32,
}

/// The error of a lookup a newer one stopped; nobody is waiting for it.
pub const CANCELLED: &str = "definitions:cancelled";
static LATEST: AtomicU64 = AtomicU64::new(0);
/// Other files read at most, the likeliest first: a name like `new` is in most of a Rust repo.
const MAX_FILES: usize = 150;
const MAX_RESULTS: usize = 50;
/// Past this a file is generated or minified, and not read for definitions.
const MAX_BYTES: usize = 1024 * 1024;
/// `git cat-file --batch` gets its whole list before its output is read: under a pipe's buffer.
const MAX_BATCH_INPUT: usize = 48 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lang {
    Rust,
    JavaScript,
    TypeScript,
    Tsx,
    Python,
    Go,
}

impl Lang {
    fn of(path: &str) -> Option<Lang> {
        let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "rs" => Lang::Rust,
            "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "py" | "pyi" => Lang::Python,
            "go" => Lang::Go,
            _ => return None,
        })
    }

    /// Files a name used in this language can be defined in, as pathspecs.
    fn family(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &["*.rs"],
            Lang::JavaScript | Lang::TypeScript | Lang::Tsx => &[
                "*.ts", "*.tsx", "*.mts", "*.cts", "*.js", "*.jsx", "*.mjs", "*.cjs",
            ],
            Lang::Python => &["*.py", "*.pyi"],
            Lang::Go => &["*.go"],
        }
    }
}

/// A language's parser and query, and how its patterns bind names.
struct Spec {
    language: Language,
    query: Query,
    /// Leaves a pattern binds (`a` in `let (a, b)`).
    bindings: &'static [&'static str],
    /// Parts of a pattern that bind nothing: a default value, a type, an object key.
    skip_fields: &'static [&'static str],
    skip_kinds: &'static [&'static str],
    /// Rust: a capitalized name in a pattern is a variant or a constant (`None`), not a binding.
    capitalized_binds: bool,
}

fn spec(lang: Lang) -> &'static Spec {
    static SPECS: [OnceLock<Spec>; 6] = [const { OnceLock::new() }; 6];
    SPECS[lang as usize].get_or_init(|| {
        let (language, source): (Language, &str) = match lang {
            Lang::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                include_str!("definitions/rust.scm"),
            ),
            Lang::JavaScript => (
                tree_sitter_javascript::LANGUAGE.into(),
                include_str!("definitions/javascript.scm"),
            ),
            Lang::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                include_str!("definitions/typescript.scm"),
            ),
            Lang::Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                include_str!("definitions/typescript.scm"),
            ),
            Lang::Python => (
                tree_sitter_python::LANGUAGE.into(),
                include_str!("definitions/python.scm"),
            ),
            Lang::Go => (
                tree_sitter_go::LANGUAGE.into(),
                include_str!("definitions/go.scm"),
            ),
        };
        // Every query is compiled by the tests: one that doesn't is a bug, not an input.
        let query = Query::new(&language, source)
            .unwrap_or_else(|e| panic!("definitions query for {lang:?}: {e}"));
        const JS: &[&str] = &[
            "member_expression",
            "subscript_expression",
            "computed_property_name",
        ];
        let (bindings, skip_fields, skip_kinds): (&[&str], &[&str], &[&str]) = match lang {
            Lang::Rust => (
                &["identifier", "shorthand_field_identifier"],
                &["type"],
                &["scoped_identifier"],
            ),
            Lang::JavaScript => (
                &["identifier", "shorthand_property_identifier_pattern"],
                &["right", "key"],
                JS,
            ),
            Lang::TypeScript | Lang::Tsx => (
                &["identifier", "shorthand_property_identifier_pattern"],
                &["right", "key", "type"],
                JS,
            ),
            Lang::Python => (&["identifier"], &["type"], &["attribute", "subscript"]),
            Lang::Go => (
                &["identifier"],
                &[],
                &["selector_expression", "index_expression"],
            ),
        };
        Spec {
            language,
            query,
            bindings,
            skip_fields,
            skip_kinds,
            capitalized_binds: lang != Lang::Rust,
        }
    })
}

/// Ordered: a name captured twice keeps the later kind (a Rust method is a function and a member).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Def,
    /// Reached through something else (`x.len`, `Self::new`), so in reach from anywhere, but only so.
    Member,
    /// Bound by an import: the definition is in another file.
    Import,
}

struct Def<'t> {
    node: Node<'t>,
    kind: Kind,
    /// Where it's visible (None: the whole file), and how many scopes deep that is.
    scope: Option<Node<'t>>,
    depth: usize,
}

impl Def<'_> {
    /// Visible from other files, as a top-level item or a member.
    fn exported(&self) -> bool {
        self.kind == Kind::Member || (self.kind == Kind::Def && self.scope.is_none())
    }
}

fn text<'s>(n: Node, src: &'s str) -> &'s str {
    &src[n.byte_range()]
}

fn parse(spec: &Spec, src: &str) -> Option<Tree> {
    let mut p = Parser::new();
    p.set_language(&spec.language).ok()?;
    p.parse(src, None)
}

/// Every name the file defines, and its imports of everything in a module (`use git::*`).
/// The queries capture `@def` (a name defined in its scope), `@member`, `@import`, `@pattern`
/// (a pattern whose names it binds), `@scope` and `@glob`.
fn defs<'t>(spec: &Spec, tree: &'t Tree, src: &str) -> (Vec<Def<'t>>, Vec<Node<'t>>) {
    let names = spec.query.capture_names();
    let mut scopes = HashSet::new();
    let mut globs = vec![];
    let mut found: HashMap<usize, (Node<'t>, Kind)> = HashMap::new();
    let mut add = |n: Node<'t>, kind: Kind| {
        let e = found.entry(n.id()).or_insert((n, kind));
        e.1 = e.1.max(kind);
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&spec.query, tree.root_node(), src.as_bytes());
    while let Some(m) = matches.next() {
        for c in m.captures {
            match names[c.index as usize] {
                "def" => add(c.node, Kind::Def),
                "member" => add(c.node, Kind::Member),
                "import" => add(c.node, Kind::Import),
                "pattern" => bindings(spec, c.node, src, &mut |n| add(n, Kind::Def)),
                "scope" => {
                    scopes.insert(c.node.id());
                }
                "glob" => globs.push(c.node),
                _ => {}
            }
        }
    }
    let mut defs: Vec<Def> = found
        .into_values()
        .map(|(node, kind)| {
            let (scope, depth) = if kind == Kind::Member {
                (None, 0)
            } else {
                scope_of(node, &scopes)
            };
            Def {
                node,
                kind,
                scope,
                depth,
            }
        })
        .collect();
    defs.sort_by_key(|d| d.node.start_byte());
    (defs, globs)
}

/// The names a pattern binds: `a`, `b` and `c` in `let (a, Some(b), Foo { c, .. })`.
fn bindings<'t>(spec: &Spec, n: Node<'t>, src: &str, add: &mut dyn FnMut(Node<'t>)) {
    if spec.bindings.contains(&n.kind()) {
        let capitalized = text(n, src).starts_with(|c: char| c.is_uppercase());
        if spec.capitalized_binds || !capitalized {
            add(n);
        }
        return;
    }
    if spec.skip_kinds.contains(&n.kind()) {
        return;
    }
    let mut cursor = n.walk();
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        let skipped = cursor
            .field_name()
            .is_some_and(|f| spec.skip_fields.contains(&f));
        if cursor.node().is_named() && !skipped {
            bindings(spec, cursor.node(), src, add);
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// The innermost scope around `n`, and how many there are. A function's name belongs to the
/// scope the function is in, not to the function.
fn scope_of<'t>(n: Node<'t>, scopes: &HashSet<usize>) -> (Option<Node<'t>>, usize) {
    let (mut scope, mut depth, mut at) = (None, 0, n);
    while let Some(p) = at.parent() {
        let named_by = p
            .child_by_field_name("name")
            .is_some_and(|c| c.id() == n.id());
        if scopes.contains(&p.id()) && !named_by {
            scope.get_or_insert(p);
            depth += 1;
        }
        at = p;
    }
    (scope, depth)
}

/// Leaves that name something.
const REFERENCES: &[&str] = &[
    "identifier",
    "type_identifier",
    "field_identifier",
    "property_identifier",
    "private_property_identifier",
    "shorthand_property_identifier",
    "shorthand_property_identifier_pattern",
    "shorthand_field_identifier",
    "package_identifier",
];
/// Names that are always a member of something (`x.len`, `this.state`).
const MEMBERS: &[&str] = &[
    "field_identifier",
    "property_identifier",
    "private_property_identifier",
];
/// Paths whose later parts are reached through the first (`git::run`, `api.readFile`).
const PATHS: &[&str] = &[
    "scoped_identifier",
    "scoped_type_identifier",
    "field_expression",
    "member_expression",
    "attribute",
    "selector_expression",
    "qualified_type",
    "nested_type_identifier",
    "nested_identifier",
];
/// What holds statements: the statement an import is in stops below one.
const BODIES: &[&str] = &[
    "source_file",
    "program",
    "module",
    "block",
    "statement_block",
    "declaration_list",
];

/// Declarations and the part of each their names aren't bound in yet: `let x = x + 1` reads
/// the `x` from before.
const OWN_VALUES: &[(&str, &str)] = &[
    ("let_declaration", "value"),
    ("let_condition", "value"),
    ("for_expression", "value"),
    ("variable_declarator", "value"),
    ("for_in_statement", "right"),
    ("assignment", "right"),
    ("for_statement", "right"),
    ("for_in_clause", "right"),
    ("short_var_declaration", "right"),
    ("range_clause", "right"),
];

/// Whether `at` is in the value of the declaration that binds `d`.
fn in_own_value(d: &Def, at: usize) -> bool {
    let mut p = d.node.parent();
    while let Some(decl) = p.filter(|p| d.scope.is_none_or(|s| s.id() != p.id())) {
        if let Some((_, field)) = OWN_VALUES.iter().find(|(kind, _)| *kind == decl.kind()) {
            return decl
                .child_by_field_name(field)
                .is_some_and(|v| v.start_byte() <= at && at < v.end_byte());
        }
        p = decl.parent();
    }
    false
}

fn reference_at(tree: &Tree, at: usize) -> Option<Node<'_>> {
    // The column right after a name is on it too, as the editor has it.
    [at, at.saturating_sub(1)].into_iter().find_map(|b| {
        let n = tree.root_node().descendant_for_byte_range(b, b)?;
        (REFERENCES.contains(&n.kind()) && n.start_byte() <= at && at <= n.end_byte()).then_some(n)
    })
}

/// `git` in `git::run`, `api` in `api.readFile`: what `n` is reached through.
fn qualifier(n: Node<'_>) -> Option<Node<'_>> {
    let p = n.parent().filter(|p| PATHS.contains(&p.kind()))?;
    let first = p.named_child(0)?;
    (first.id() != n.id()).then_some(first)
}

/// The definition `name` means where it's used, unqualified: the innermost in scope there, and
/// one before over one after (a later `let` shadows only what follows it).
fn in_scope<'a, 't>(defs: &'a [Def<'t>], name: &str, at: usize, src: &str) -> Option<&'a Def<'t>> {
    defs.iter()
        .filter(|d| d.kind != Kind::Member && text(d.node, src) == name)
        .filter(|d| {
            d.scope
                .is_none_or(|s| s.start_byte() <= at && at < s.end_byte())
        })
        .filter(|d| !in_own_value(d, at))
        .max_by_key(|d| {
            let start = d.node.start_byte() as isize;
            let before = start <= at as isize;
            (before, d.depth, if before { start } else { -start })
        })
}

/// The statement an import is in: what the other file is named by (`use crate::git::run`).
fn statement<'s>(n: Node, src: &'s str) -> &'s str {
    let mut at = n;
    while let Some(p) = at.parent().filter(|p| !BODIES.contains(&p.kind())) {
        at = p;
    }
    let s = text(at, src);
    let mut end = s.len().min(1000);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn utf16(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

fn location(path: &str, src: &str, n: Node) -> Location {
    let start = n.start_byte();
    let line_start = start - n.start_position().column;
    let column = utf16(&src[line_start..start]);
    let one_line = n.end_position().row == n.start_position().row;
    Location {
        path: path.to_string(),
        line: n.start_position().row as u32 + 1,
        column,
        end_column: column + if one_line { utf16(text(n, src)) } else { 0 },
    }
}

/// The byte at a 1-based line and 0-based UTF-16 column.
fn byte_at(src: &str, line: u32, column: u32) -> Option<usize> {
    let line_start = if line <= 1 {
        0
    } else {
        src.match_indices('\n').nth(line as usize - 2)?.0 + 1
    };
    let mut units = 0;
    for (i, c) in src[line_start..].char_indices() {
        if units >= column || c == '\n' {
            return Some(line_start + i);
        }
        units += c.len_utf16() as u32;
    }
    Some(src.len())
}

pub fn find(repo: &Path, req: &Request) -> Result<Vec<Location>, String> {
    let id = LATEST.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(rev) = &req.rev {
        git::validate_tree_rev(rev)?;
    }
    let Some(lang) = Lang::of(&req.path) else {
        return Ok(vec![]);
    };
    let src = req.text.as_str();
    match resolve(lang, &req.path, src, req.line, req.column) {
        None => Ok(vec![]),
        Some(Answer::Here(found)) => Ok(found),
        Some(Answer::Elsewhere { name, hint, import }) => {
            let found = search(repo, req, lang, name, &hint, id)?;
            Ok(if found.is_empty() {
                import.into_iter().collect()
            } else {
                found
            })
        }
    }
}

enum Answer<'s> {
    Here(Vec<Location>),
    /// In another file, likely one `hint` names; else the import that brought it in, if any.
    Elsewhere {
        name: &'s str,
        hint: String,
        import: Option<Location>,
    },
}

/// What the name at a line and column is, as far as its own file tells.
fn resolve<'s>(lang: Lang, path: &str, src: &'s str, line: u32, column: u32) -> Option<Answer<'s>> {
    let spec = spec(lang);
    let tree = parse(spec, src)?;
    let node = reference_at(&tree, byte_at(src, line, column)?)?;
    let name = text(node, src);
    let (defs, globs) = defs(spec, &tree, src);
    let here = |d: &Def| location(path, src, d.node);
    let import = |d: &Def| Answer::Elsewhere {
        name,
        hint: statement(d.node, src).to_string(),
        import: Some(here(d)),
    };

    // On a definition: that one, or what an import brings in.
    if let Some(d) = defs.iter().find(|d| d.node.id() == node.id()) {
        return Some(match d.kind {
            Kind::Import => import(d),
            _ => Answer::Here(vec![here(d)]),
        });
    }
    let through = qualifier(node);
    if through.is_some() || MEMBERS.contains(&node.kind()) {
        // `api.readFile` with `api` imported: in the file it comes from.
        let imported = through
            .and_then(|q| in_scope(&defs, text(q, src), q.start_byte(), src))
            .filter(|d| d.kind == Kind::Import);
        if let Some(d) = imported {
            return Some(Answer::Elsewhere {
                name,
                hint: statement(d.node, src).to_string(),
                import: None,
            });
        }
        let found: Vec<_> = defs
            .iter()
            .filter(|d| d.exported() && text(d.node, src) == name)
            .map(here)
            .collect();
        return Some(if found.is_empty() {
            Answer::Elsewhere {
                name,
                hint: node.parent().map_or("", |p| text(p, src)).to_string(),
                import: None,
            }
        } else {
            Answer::Here(found)
        });
    }
    Some(match in_scope(&defs, name, node.start_byte(), src) {
        Some(d) if d.kind == Kind::Import => import(d),
        Some(d) => Answer::Here(vec![here(d)]),
        // Maybe from a module imported whole.
        None => Answer::Elsewhere {
            name,
            hint: globs
                .iter()
                .map(|g| statement(*g, src))
                .collect::<Vec<_>>()
                .join("\n"),
            import: None,
        },
    })
}

/// `name`'s top-level definitions and members in the other files that mention it. Files `hint`
/// names (the import, the path it's reached through) go first, and when they define it, alone.
fn search(
    repo: &Path,
    req: &Request,
    lang: Lang,
    name: &str,
    hint: &str,
    id: u64,
) -> Result<Vec<Location>, String> {
    let rev = req.rev.as_deref();
    let words: HashSet<String> = hint
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let hinted = |path: &str| file_names(path, lang).iter().any(|n| words.contains(n));
    let mut paths = mentions(repo, rev, lang, name)?;
    paths.retain(|p| *p != req.path);
    paths.sort_by_cached_key(|p| {
        (
            !hinted(p),
            std::cmp::Reverse(shared_dirs(p, &req.path)),
            p.clone(),
        )
    });
    paths.truncate(MAX_FILES);

    let mut found = vec![];
    // Hinted files come first: what they define, if anything, is the answer.
    let mut by_hint = false;
    for (path, text) in read_all(repo, rev, &paths)? {
        if LATEST.load(Ordering::SeqCst) != id {
            return Err(CANCELLED.into());
        }
        let hinted = hinted(&path);
        if by_hint && !hinted {
            break;
        }
        let Some(spec) = Lang::of(&path).map(spec) else {
            continue;
        };
        let Some(tree) = parse(spec, &text) else {
            continue;
        };
        found.extend(
            defs(spec, &tree, &text)
                .0
                .iter()
                .filter(|d| d.exported() && self::text(d.node, &text) == name)
                .map(|d| location(&path, &text, d.node)),
        );
        by_hint = hinted && !found.is_empty();
        if found.len() >= MAX_RESULTS {
            found.truncate(MAX_RESULTS);
            break;
        }
    }
    Ok(found)
}

/// What a file is imported by: its name, or its folder's for `mod.rs`, `index.ts` and the like,
/// and in Go (a package is a folder) both.
fn file_names(path: &str, lang: Lang) -> Vec<String> {
    let p = Path::new(path);
    let stem = p
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.split('.').next().unwrap_or(n).to_lowercase())
        .unwrap_or_default();
    let dir = p
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|d| d.to_str())
        .map(str::to_lowercase);
    let generic = ["mod", "lib", "main", "index", "__init__"].contains(&stem.as_str());
    match dir {
        Some(dir) if generic || lang == Lang::Go => vec![stem, dir],
        _ => vec![stem],
    }
}

fn shared_dirs(a: &str, b: &str) -> usize {
    let (a, b) = (Path::new(a).parent(), Path::new(b).parent());
    match (a, b) {
        (Some(a), Some(b)) => a
            .components()
            .zip(b.components())
            .take_while(|(x, y)| x == y)
            .count(),
        _ => 0,
    }
}

/// Files of `lang`'s family that have `name` as a whole word: tracked and untracked ones in
/// the worktree (as the explorer lists), or a commit's.
fn mentions(repo: &Path, rev: Option<&str>, lang: Lang, name: &str) -> Result<Vec<String>, String> {
    let mut args = vec!["grep", "-l", "-z", "-I", "-w", "-F"];
    if rev.is_none() {
        args.push("--untracked");
    }
    args.extend(["-e", name]);
    args.extend(rev);
    args.push("--");
    args.extend(lang.family());
    let mut cmd = git::command(repo, &args);
    // `*.rs` is a glob here, at any depth.
    cmd.env_remove("GIT_LITERAL_PATHSPECS");
    // 1: no file has it.
    let out = git::exec(cmd, "git grep", &[1], None, Some(Duration::from_secs(10)))?;
    let prefix = rev.map(|r| format!("{r}:")).unwrap_or_default();
    Ok(out
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p))
        .map(|p| p.strip_prefix(&prefix).unwrap_or(&p).to_string())
        .collect())
}

/// The files' texts, skipping any that are binary, too large or gone.
fn read_all(
    repo: &Path,
    rev: Option<&str>,
    paths: &[String],
) -> Result<Vec<(String, String)>, String> {
    let Some(rev) = rev else {
        return Ok(paths
            .iter()
            .filter_map(|p| {
                let f = fs::read_file(repo, p);
                let ok = f.exists && !f.binary && !f.too_large && f.text.len() <= MAX_BYTES;
                ok.then(|| (p.clone(), f.text))
            })
            .collect());
    };
    let mut input = String::new();
    let mut asked = vec![];
    for p in paths {
        let line = format!("{rev}:{p}\n");
        // A name with a newline would split the request.
        if p.contains('\n') || input.len() + line.len() > MAX_BATCH_INPUT {
            continue;
        }
        input.push_str(&line);
        asked.push(p);
    }
    let cmd = git::command(repo, &["cat-file", "--batch"]);
    let out = git::exec(
        cmd,
        "git cat-file",
        &[],
        Some(input.as_bytes()),
        Some(Duration::from_secs(10)),
    )?;
    // Each answer: `<oid> blob <size>\n<content>\n`, or `<spec> missing\n`.
    let mut texts = vec![];
    let mut rest = out.as_slice();
    for p in asked {
        let Some(nl) = rest.iter().position(|&b| b == b'\n') else {
            break;
        };
        let header = String::from_utf8_lossy(&rest[..nl]).into_owned();
        rest = &rest[nl + 1..];
        let mut parts = header.split(' ');
        let (Some(_), Some(kind), Some(size)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Ok(size) = size.parse::<usize>() else {
            continue;
        };
        let Some(content) = rest.get(..size) else {
            break;
        };
        rest = rest.get(size + 1..).unwrap_or_default();
        if kind == "blob" && size <= MAX_BYTES && !git::is_binary(content) {
            texts.push((p.clone(), String::from_utf8_lossy(content).into_owned()));
        }
    }
    Ok(texts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `resolve` says of the name after the `|` in `marked`: `line:column` of each
    /// definition in the file, or `elsewhere` with the hint and the import's place.
    fn at(path: &str, marked: &str) -> Vec<String> {
        let i = marked.find('|').expect("a | marks the name");
        let src = marked.replacen('|', "", 1);
        let before = &src[..i];
        let line = before.matches('\n').count() as u32 + 1;
        let column = utf16(&before[before.rfind('\n').map_or(0, |n| n + 1)..]);
        let pos = |l: &Location| format!("{}:{}", l.line, l.column);
        match resolve(Lang::of(path).unwrap(), path, &src, line, column).expect("a name there") {
            Answer::Here(found) => found.iter().map(pos).collect(),
            Answer::Elsewhere { name, hint, import } => {
                let import = import.as_ref().map_or("-".into(), pos);
                vec![format!("elsewhere {name} [{hint}] {import}")]
            }
        }
    }

    #[test]
    fn every_query_compiles() {
        for lang in [
            Lang::Rust,
            Lang::JavaScript,
            Lang::TypeScript,
            Lang::Tsx,
            Lang::Python,
            Lang::Go,
        ] {
            spec(lang);
        }
    }

    #[test]
    fn rust_locals_follow_scope_and_shadowing() {
        let f = "fn f(a: u8, (b, c): (u8, u8)) {\n    let x = a;\n";
        assert_eq!(
            at("a.rs", &format!("{f}    let x = x + |b;\n}}\n")),
            ["1:13"]
        );
        assert_eq!(
            at("a.rs", &format!("{f}    let x = |x + 1;\n}}\n")),
            ["2:8"]
        );
        assert_eq!(
            at("a.rs", &format!("{f}    let x = x + 1;\n    |x;\n}}\n")),
            ["3:8"]
        );
        assert_eq!(
            at("a.rs", &format!("{f}    for (i, v) in z {{ |v; }}\n}}\n")),
            ["3:12"]
        );
        let src = "fn f(o: Option<u8>) {\n    if let Some(w) = o { |w; }\n}\n";
        assert_eq!(at("a.rs", src), ["2:16"]);
        let src = "fn f(o: Option<u8>) {\n    match o { Some(n) => { |n; } _ => {} }\n}\n";
        assert_eq!(at("a.rs", src), ["2:19"]);
        // A variant in a pattern binds nothing.
        let src = "fn f(o: Option<u8>) {\n    match o { None => { |None; } _ => {} }\n}\n";
        assert_eq!(at("a.rs", src), ["elsewhere None [] -"]);
    }

    #[test]
    fn rust_items_members_and_imports() {
        let src = "use crate::git::{self, FileText as FT, run};\nfn g() { |run(); }\n";
        assert_eq!(
            at("a.rs", src),
            ["elsewhere run [use crate::git::{self, FileText as FT, run};] 1:39"]
        );
        let src = "struct S { field: u8 }\nimpl S { fn get(&self) -> u8 { self.|field } }\n";
        assert_eq!(at("a.rs", src), ["1:11"]);
        let src = "struct S;\nimpl S { fn new() -> Self { S } }\nfn g() { S::|new(); }\n";
        assert_eq!(at("a.rs", src), ["2:12"]);
        // A method isn't what a bare name means.
        let src = "struct S;\nimpl S { fn new() -> Self { S } }\nfn g() { |new(); }\n";
        assert_eq!(at("a.rs", src), ["elsewhere new [] -"]);
        let src = "use a::b::FileText as |FT;\n";
        assert_eq!(
            at("a.rs", src),
            ["elsewhere FT [use a::b::FileText as FT;] 1:22"]
        );
        let src = "mod util;\nfn g() { util::|x(); }\n";
        assert_eq!(at("a.rs", src), ["elsewhere x [mod util;] -"]);
        let src = "fn g() { git::|run(); }\n";
        assert_eq!(at("a.rs", src), ["elsewhere run [git::run] -"]);
    }

    #[test]
    fn a_whole_module_import_hints_where_to_look() {
        let src = "use crate::{fs as vfs, git::*};\nfn f() { |run(); }\n";
        assert_eq!(
            at("a.rs", src),
            ["elsewhere run [use crate::{fs as vfs, git::*};] -"]
        );
        let src = "from pkg.util import *\n|helper()\n";
        assert_eq!(
            at("a.py", src),
            ["elsewhere helper [from pkg.util import *] -"]
        );
    }

    #[test]
    fn typescript_state_params_and_members() {
        let src = "import { useState } from \"react\";\nexport function View({ open: shown, rest = 1 }: Props) {\n  const [open, setOpen] = useState(false);\n  return shown && |open;\n}\n";
        assert_eq!(at("a.tsx", src), ["3:9"]);
        let src = "function View({ open: shown }: Props) {\n  return |shown;\n}\n";
        assert_eq!(at("a.tsx", src), ["1:22"]);
        let src = "import { useState } from \"react\";\nconst x = |useState(0);\n";
        assert_eq!(
            at("a.ts", src),
            ["elsewhere useState [import { useState } from \"react\";] 1:9"]
        );
        let src = "import { api } from \"@/lib/api\";\napi.|readFile(p);\n";
        assert_eq!(
            at("a.ts", src),
            ["elsewhere readFile [import { api } from \"@/lib/api\";] -"]
        );
        let src = "interface Props { links?: string }\nfunction f(props: Props) { return props.|links; }\n";
        assert_eq!(at("a.ts", src), ["1:18"]);
        let src = "class K { field = 1; constructor() { this.other = 2; } m() { return this.|other; } }\n";
        assert_eq!(at("a.ts", src), ["1:42"]);
        let src = "type T = { a: number };\nconst v: |T = { a: 1 };\n";
        assert_eq!(at("a.ts", src), ["1:5"]);
        let src = "function f<G>(g: |G) {}\n";
        assert_eq!(at("a.ts", src), ["1:11"]);
    }

    #[test]
    fn javascript_patterns() {
        let f = "function f(x, { y, z: w }, [m, n = 3], ...r) {\n";
        assert_eq!(
            at("a.js", &format!("{f}  for (const k of ks) {{ |k; }}\n}}\n")),
            ["2:13"]
        );
        assert_eq!(at("a.jsx", &format!("{f}  return |w + n;\n}}\n")), ["1:22"]);
        assert_eq!(at("a.js", &format!("{f}  return w + |n;\n}}\n")), ["1:31"]);
        assert_eq!(at("a.js", &format!("{f}  return |r;\n}}\n")), ["1:42"]);
    }

    #[test]
    fn python_scopes() {
        let src = "import os\nX = 1\ndef f(a, b=1, *args, **kw):\n    q = [i for i in a]\n    return |b\n";
        assert_eq!(at("a.py", src), ["3:9"]);
        let src = "from .models import User\ndef f():\n    return |User()\n";
        assert_eq!(
            at("a.py", src),
            ["elsewhere User [from .models import User] 1:20"]
        );
        let src = "class K:\n    def __init__(self):\n        self.count = 0\n    def m(self):\n        return self.|count\n";
        assert_eq!(at("a.py", src), ["3:13"]);
        let src = "X = 1\ndef f():\n    return |X\n";
        assert_eq!(at("a.py", src), ["1:0"]);
    }

    #[test]
    fn go_declarations() {
        let f = "package main\nfunc (s *S) Meth(a, b int) (r int) {\n  x := 1\n";
        assert_eq!(
            at(
                "a.go",
                &format!("{f}  for i, v := range xs {{ _ = |v }}\n}}\n")
            ),
            ["4:9"]
        );
        assert_eq!(at("a.go", &format!("{f}  return |x\n}}\n")), ["3:2"]);
        let src =
            "package main\ntype S struct { Field int }\nfunc f(s S) int { return s.|Field }\n";
        assert_eq!(at("a.go", src), ["2:16"]);
    }

    #[test]
    fn columns_count_utf16() {
        let src = "const é = 1; const 𝒳 = 2;\n";
        assert_eq!(byte_at(src, 1, 19), Some(src.find('𝒳').unwrap()));
        assert_eq!(
            at("a.js", "let é = 1; let 𝒳 = é;\nconsole.log(|𝒳);\n"),
            ["1:15"]
        );
        let src = "let 𝒳 = 1;\n";
        let spec = spec(Lang::JavaScript);
        let tree = parse(spec, src).unwrap();
        let x = defs(spec, &tree, src).0.remove(0);
        assert_eq!(
            location("a.js", src, x.node),
            Location {
                path: "a.js".into(),
                line: 1,
                column: 4,
                end_column: 6
            }
        );
    }

    #[test]
    fn hints_name_files() {
        assert_eq!(file_names("src/lib/api.ts", Lang::TypeScript), ["api"]);
        assert_eq!(file_names("src/git/mod.rs", Lang::Rust), ["mod", "git"]);
        assert_eq!(file_names("pkg/store/db.go", Lang::Go), ["db", "store"]);
        assert_eq!(file_names("a/button.test.tsx", Lang::Tsx), ["button"]);
    }
}
