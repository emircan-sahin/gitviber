//! Guided reviews: the user's agent CLI explains a commit, a branch, a pull request or the
//! uncommitted changes as ordered sections, the way a reviewer should read them (core's suggest.rs
//! runs it, as for commit messages).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use serde_json::{Map, Value, json};

use super::answer::{
    Schema, clip, flag, json_of, language_name, normalized, one_line, positive_line, records, text,
    whole,
};

/// What a section is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Ui,
    Api,
    Core,
    Data,
    Cli,
    Security,
    Tests,
    Docs,
    Examples,
    Deps,
    Build,
    Scripts,
    Config,
    I18n,
    Assets,
    Other,
}

/// Each category's key and the line the prompt explains it by, in the prompt's order.
pub const CATEGORIES: [(Category, &str, &str); 16] = [
    (Category::Ui, "ui", "screens, components and styles"),
    (
        Category::Api,
        "api",
        "endpoints, commands and the interfaces between parts",
    ),
    (
        Category::Core,
        "core",
        "the logic and state the rest relies on",
    ),
    (
        Category::Data,
        "data",
        "schemas, models, migrations and storage",
    ),
    (Category::Cli, "cli", "command-line commands and flags"),
    (
        Category::Security,
        "security",
        "auth, permissions, secrets, input checks and sandboxing",
    ),
    (Category::Tests, "tests", "tests and their fixtures"),
    (
        Category::Docs,
        "docs",
        "docs, READMEs, changelogs and comments",
    ),
    (Category::Examples, "examples", "examples and demos"),
    (Category::Deps, "deps", "dependencies and lockfiles"),
    (Category::Build, "build", "build, packaging and CI"),
    (
        Category::Scripts,
        "scripts",
        "development and maintenance scripts",
    ),
    (
        Category::Config,
        "config",
        "settings and configuration files",
    ),
    (Category::I18n, "i18n", "translations and locale strings"),
    (Category::Assets, "assets", "images, fonts, icons and media"),
    (Category::Other, "other", "anything else"),
];

impl Category {
    pub fn key(self) -> &'static str {
        CATEGORIES
            .iter()
            .find(|c| c.0 == self)
            .map_or("other", |c| c.1)
    }

    /// As the guide shows it.
    pub fn label(self) -> &'static str {
        match self {
            Category::Ui => "UI",
            Category::Api => "API",
            Category::Core => "Core",
            Category::Data => "Data",
            Category::Cli => "CLI",
            Category::Security => "Security",
            Category::Tests => "Tests",
            Category::Docs => "Docs",
            Category::Examples => "Examples",
            Category::Deps => "Dependencies",
            Category::Build => "Build",
            Category::Scripts => "Scripts",
            Category::Config => "Config",
            Category::I18n => "Translations",
            Category::Assets => "Assets",
            Category::Other => "Other",
        }
    }

    /// One the prompt has, any case; anything else (a guide from before categories) is other.
    pub fn read(v: Option<&Value>) -> Category {
        let c = text(v).to_lowercase();
        CATEGORIES
            .iter()
            .find(|k| k.1 == c)
            .map_or(Category::Other, |k| k.0)
    }
}

/// How carefully a section wants reading, most first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Importance {
    High,
    Medium,
    Low,
}

impl Importance {
    pub fn key(self) -> &'static str {
        match self {
            Importance::High => "high",
            Importance::Medium => "medium",
            Importance::Low => "low",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Importance::High => "High",
            Importance::Medium => "Medium",
            Importance::Low => "Low",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Importance::High => {
                "Review carefully: security, data loss, or hard to revert or get right"
            }
            Importance::Medium => "A change in behavior worth a careful read",
            Importance::Low => "Mechanical, docs or style: a skim is enough",
        }
    }

    /// As the model gave it; from before importance was asked: high when it was critical, else
    /// medium, the neutral one.
    fn read(v: Option<&Value>, critical: Option<&Value>) -> Importance {
        match text(v).to_lowercase().as_str() {
            "high" => Importance::High,
            "medium" => Importance::Medium,
            "low" => Importance::Low,
            _ if flag(critical) => Importance::High,
            _ => Importance::Medium,
        }
    }
}

/// The side of a diff a note's line number counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteSide {
    /// An added or unchanged line.
    New,
    /// A removed one.
    Old,
}

impl NoteSide {
    fn read(v: Option<&Value>) -> NoteSide {
        if text(v).eq_ignore_ascii_case("old") {
            NoteSide::Old
        } else {
            NoteSide::New
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileNote {
    /// As the section names it.
    pub path: String,
    pub text: String,
    pub critical: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineNote {
    pub path: String,
    pub text: String,
    pub critical: bool,
    pub side: NoteSide,
    pub line: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    /// Other for a guide written before categories.
    pub category: Category,
    pub summary: String,
    pub files: Vec<String>,
    /// How to verify it, one line; "" for none.
    pub check: String,
    /// "" when the model saw none.
    pub risk: String,
    pub importance: Importance,
    pub file_notes: Vec<FileNote>,
    pub line_notes: Vec<LineNote>,
}

impl Section {
    /// The files no section has, listed after the rest.
    pub fn other() -> Section {
        Section {
            title: "Other changes".into(),
            category: Category::Other,
            summary: "In no section of the guide.".into(),
            files: vec![],
            check: String::new(),
            risk: String::new(),
            importance: Importance::Medium,
            file_notes: vec![],
            line_notes: vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartStatus {
    New,
    Changed,
    Same,
}

impl PartStatus {
    fn read(v: Option<&Value>) -> PartStatus {
        match text(v).to_lowercase().as_str() {
            "new" | "added" => PartStatus::New,
            "changed" | "modified" => PartStatus::Changed,
            _ => PartStatus::Same,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            PartStatus::New => "New",
            PartStatus::Changed => "Changed",
            PartStatus::Same => "Unchanged",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelField {
    pub name: String,
    pub note: String,
    pub status: PartStatus,
    /// The section that explains it, from 1.
    pub section: Option<usize>,
}

/// A data type, table or schema the change touches, drawn as a card of its fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub name: String,
    pub file: String,
    pub status: PartStatus,
    pub note: String,
    pub section: Option<usize>,
    pub fields: Vec<ModelField>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlowStep {
    /// The model's, for its edges.
    pub id: String,
    pub label: String,
    pub file: String,
    pub status: PartStatus,
    pub decision: bool,
    pub section: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlowEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub title: String,
    pub steps: Vec<FlowStep>,
    pub edges: Vec<FlowEdge>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Guide {
    pub title: String,
    /// The overview: a paragraph and the change's parts, in Markdown.
    pub summary: String,
    /// A guide written before diagrams were structured: its Mermaid source; "" for none.
    pub diagram: String,
    pub models: Vec<Model>,
    pub flows: Vec<Flow>,
    pub sections: Vec<Section>,
}

impl Guide {
    pub fn has_diagram(&self) -> bool {
        !self.models.is_empty() || !self.flows.is_empty() || !self.diagram.is_empty()
    }
}

/// How much of a diagram is drawn, as the prompt asks (past this it stops explaining), how many
/// sections, and their notes.
pub mod limits {
    pub const MODELS: usize = 6;
    pub const FIELDS: usize = 12;
    pub const FLOWS: usize = 2;
    pub const STEPS: usize = 12;
    pub const EDGES: usize = 24;
    pub const SECTIONS: usize = 50;
    pub const NOTES: usize = 40;
    pub const NOTE: usize = 500;
}

fn section_ref() -> Schema {
    Schema {
        ty: "integer",
        description: Some("the number of the section that covers it, 1 for the first".into()),
        ..Default::default()
    }
    .example(json!(1))
}

fn status() -> Schema {
    Schema::choice(&["new", "changed", "same"])
}

fn note(extra: Vec<(&'static str, Schema)>, required: &[&'static str]) -> Schema {
    let mut properties = vec![("path", Schema::string("one of the section's files"))];
    properties.extend(extra);
    properties.push(("text", Schema::string("what to know there, in a sentence")));
    properties.push((
        "critical",
        Schema {
            ty: "boolean",
            description: Some(
                "true only for security, data loss or a change that's hard to revert".into(),
            ),
            ..Default::default()
        }
        .example(json!(false)),
    ));
    let mut req = vec!["path"];
    req.extend_from_slice(required);
    req.push("text");
    Schema::array(Schema::object(&req, properties))
}

/// The answer's shape: sent as a JSON Schema to a CLI that enforces one (Claude Code's
/// --json-schema), and drawn from it as the example in the prompt for the rest, so the two can't
/// drift. Only what every guide has is required; the parser takes any of the rest.
pub static GUIDE_SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
    let s = Schema::string;
    let field = Schema::object(
        &["name"],
        vec![
            ("name", s("a field or member")),
            ("status", status()),
            ("note", s("a few words, if it changed")),
            ("section", section_ref()),
        ],
    );
    let model = Schema::object(
        &["name"],
        vec![
            (
                "name",
                s("a type, table, schema or config the change adds or alters"),
            ),
            ("file", s("its path")),
            ("status", status()),
            ("note", s("what changed in it, in a few words")),
            ("section", section_ref()),
            ("fields", Schema::array(field)),
        ],
    );
    let step = Schema::object(
        &["id", "label"],
        vec![
            ("id", s("a short id")),
            (
                "label",
                s("the function or call, as code; for a decision, the question"),
            ),
            ("file", s("its path")),
            ("status", status()),
            ("kind", Schema::choice(&["step", "decision"])),
            ("section", section_ref()),
        ],
    );
    let edge = Schema::object(
        &["from", "to"],
        vec![
            ("from", s("a step id")),
            ("to", s("a step id")),
            ("label", s("yes or no after a decision; left out otherwise")),
        ],
    );
    let flow = Schema::object(
        &["steps"],
        vec![
            (
                "title",
                s("a path through the code the change adds or alters"),
            ),
            ("steps", Schema::array(step)),
            ("edges", Schema::array(edge)),
        ],
    );
    let diagram = Schema::object(
        &[],
        vec![("models", Schema::array(model)), ("flows", Schema::array(flow))],
    )
    .described("only for a change to a real flow or data model; left out for style, text, config or docs changes");
    let keys: Vec<&'static str> = CATEGORIES.iter().map(|c| c.1).collect();
    let section = Schema::object(
        &["title", "category", "summary", "files"],
        vec![
            ("title", s("a label of at most 4 words")),
            (
                "category",
                Schema::choice(&keys).described("one of the categories below"),
            ),
            (
                "summary",
                s("1-3 sentences of Markdown: why, and what isn't obvious from the diff"),
            ),
            (
                "files",
                Schema::array(s("a path, as the list of changed files names it")),
            ),
            (
                "check",
                s("one line on how to verify it, only when there's something specific to check"),
            ),
            (
                "risk",
                s("what could break, only when something really could"),
            ),
            (
                "importance",
                Schema::choice(&["high", "medium", "low"]).described("high, medium or low"),
            ),
            ("fileNotes", note(vec![], &[])),
            (
                "lineNotes",
                note(
                    vec![
                        ("side", Schema::choice(&["new", "old"])),
                        (
                            "line",
                            Schema {
                                ty: "integer",
                                description: Some("the line's number on that side".into()),
                                ..Default::default()
                            }
                            .example(json!(12)),
                        ),
                    ],
                    &["side", "line"],
                ),
            ),
        ],
    );
    Schema::object(
        &["title", "overview", "sections"],
        vec![
            ("title", s("what the change does, under 72 characters")),
            (
                "overview",
                s(
                    "why the change was made, in 1-2 sentences of Markdown; a numbered list of its parts after them only when it has 3 or more separate ones",
                ),
            ),
            ("diagram", diagram),
            ("sections", Schema::array(section)),
        ],
    )
});

/// Sent ahead of the commit's message or the branch's commits, the changed files and their
/// diffs; Settings shows it.
pub static GUIDE_PROMPT: LazyLock<String> = LazyLock::new(|| {
    let categories: Vec<String> = CATEGORIES
        .iter()
        .map(|(_, k, v)| format!("{k} ({v})"))
        .collect();
    [
        format!("Explain this change to a reviewer so they can read it quickly. Answer with only a JSON object, with no code fences or other text, shaped like this: {}.", GUIDE_SCHEMA.example_of()),
        "Size the guide to the change: one section for a small change (1-3 files, or under about 150 changed lines) unless it has truly separate concerns, and more only for separate concerns. Put mechanical edits (docs, changelog, formatting, renames) together in the last section, never in one of their own for a line. Order the sections the way a reviewer should read them, and put every path of the list of changed files in exactly one section.".into(),
        "Write for a developer who scans: never restate what the diff shows (values, strings, numbers, styles); say why, and point at what isn't obvious. Most sections have no check or risk. Give importance sparingly, most sections medium or low: high only for security, data loss, or a change that breaks things, is hard to revert or is easy to get wrong; medium for a change in behavior worth a careful read; low for mechanical edits, docs or style. A note's critical is true only for security, data loss or a change that's hard to revert. Add a fileNote or lineNote only where it saves the reviewer real time, a few in the whole guide at most; a lineNote's side is new for an added or unchanged line and old for a removed one, and its line is the line's number on that side.".into(),
        "Draw the diagram only when the change alters a real flow or data model, and leave it out for style, text, config or docs changes. Each of its parts' \"section\" is the number of the section that covers it, 1 for the first. Keep it small: at most 6 models of at most 12 fields, and at most 2 flows of at most 12 steps; leave out models or flows the change has none of.".into(),
        format!("Categories: {}.", categories.join(", ")),
    ]
    .join("\n\n")
});

/// The guide's prompt with the language its prose is written in.
pub fn guide_prompt(language: &str) -> String {
    let lang = language_name(language);
    format!(
        "{}\n\nWrite all prose (the title, overview, summaries, checks, risks and notes) in {lang}; keep the JSON keys and the values picked from a list (category, importance, side, status, kind), code, identifiers, paths and quoted strings as they are.",
        *GUIDE_PROMPT
    )
}

/// The changed file (of `paths`) a model's path names: as written, or without git's a/ or b/.
pub fn match_path<'a>(path: &str, paths: &HashSet<&'a str>) -> Option<&'a str> {
    if let Some(p) = paths.get(path) {
        return Some(*p);
    }
    let bare = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))?;
    paths.get(bare).copied()
}

fn bare(path: String) -> String {
    match path.strip_prefix("./") {
        Some(rest) => rest.to_string(),
        None => path,
    }
}

/// One line of text, as long as a diagram's box can hold.
fn line(v: Option<&Value>, max: usize) -> String {
    clip(&one_line(&text(v)), max)
}

/// `line`, without the bidi controls that would reorder a flow's text when it's drawn.
fn flow_line(v: Option<&Value>, max: usize) -> String {
    let t: String = text(v)
        .chars()
        .filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .collect();
    clip(&one_line(&t), max)
}

/// A part's section number, when it names one of the `count` there are.
fn section_of(v: Option<&Value>, count: usize) -> Option<usize> {
    let n = whole(v)?;
    (n.fract() == 0. && n >= 1. && n <= count as f64).then_some(n as usize)
}

fn parse_models(v: Option<&Value>, count: usize) -> Vec<Model> {
    let mut out = Vec::new();
    for m in records(v) {
        let name = line(m.get("name"), 80);
        if name.is_empty() {
            continue;
        }
        let fields = records(m.get("fields"))
            .into_iter()
            .map(|f| ModelField {
                name: line(f.get("name"), 80),
                note: line(f.get("note"), 120),
                status: PartStatus::read(f.get("status")),
                section: section_of(f.get("section"), count),
            })
            .filter(|f| !f.name.is_empty())
            .take(limits::FIELDS)
            .collect();
        out.push(Model {
            name,
            file: bare(line(m.get("file"), 200)),
            status: PartStatus::read(m.get("status")),
            note: line(m.get("note"), 120),
            section: section_of(m.get("section"), count),
            fields,
        });
        if out.len() == limits::MODELS {
            break;
        }
    }
    out
}

/// Flows with steps; a step needs a label and an id of its own, an edge two steps of its flow.
fn parse_flows(v: Option<&Value>, count: usize) -> Vec<Flow> {
    let mut out = Vec::new();
    for f in records(v) {
        let mut ids = HashSet::new();
        let mut steps = Vec::new();
        for (i, s) in records(f.get("steps")).into_iter().enumerate() {
            let mut id = line(s.get("id"), 80);
            if id.is_empty() {
                id = format!("#{i}");
            }
            let label = flow_line(s.get("label"), 100);
            if label.is_empty() || ids.contains(&id) || steps.len() >= limits::STEPS {
                continue;
            }
            ids.insert(id.clone());
            steps.push(FlowStep {
                id,
                label,
                file: bare(flow_line(s.get("file"), 200)),
                status: PartStatus::read(s.get("status")),
                decision: text(s.get("kind")).eq_ignore_ascii_case("decision"),
                section: section_of(s.get("section"), count),
            });
        }
        if steps.is_empty() {
            continue;
        }
        let mut seen = HashSet::new();
        let mut edges = Vec::new();
        for e in records(f.get("edges")) {
            let edge = FlowEdge {
                from: line(e.get("from"), 80),
                to: line(e.get("to"), 80),
                label: flow_line(e.get("label"), 40),
            };
            let key = format!("{}\0{}\0{}", edge.from, edge.to, edge.label);
            if !ids.contains(&edge.from) || !ids.contains(&edge.to) || !seen.insert(key) {
                continue;
            }
            edges.push(edge);
            if edges.len() == limits::EDGES {
                break;
            }
        }
        out.push(Flow {
            title: flow_line(f.get("title"), 100),
            steps,
            edges,
        });
        if out.len() == limits::FLOWS {
            break;
        }
    }
    out
}

/// A section, with notes only on its own files and lines that can be, at most NOTES of them.
fn parse_section(s: &Map<String, Value>) -> Section {
    let mut files: Vec<String> = Vec::new();
    if let Some(Value::Array(list)) = s.get("files") {
        for f in list {
            let path = bare(text(Some(f)));
            if !path.is_empty() && !files.contains(&path) {
                files.push(path);
            }
        }
    }
    let own: HashSet<&str> = files.iter().map(String::as_str).collect();
    let note = |n: &Map<String, Value>| {
        let path = bare(text(n.get("path")));
        (
            match_path(&path, &own).unwrap_or("").to_string(),
            clip(&text(n.get("text")), limits::NOTE),
            flag(n.get("critical")),
        )
    };
    let file_notes: Vec<FileNote> = records(s.get("fileNotes"))
        .into_iter()
        .map(note)
        .filter(|(p, t, _)| !p.is_empty() && !t.is_empty())
        .take(limits::NOTES)
        .map(|(path, text, critical)| FileNote {
            path,
            text,
            critical,
        })
        .collect();
    let line_notes: Vec<LineNote> = records(s.get("lineNotes"))
        .into_iter()
        .filter_map(|n| {
            let (path, text, critical) = note(n);
            let line = positive_line(n.get("line"))?;
            (!path.is_empty() && !text.is_empty()).then(|| LineNote {
                path,
                text,
                critical,
                side: NoteSide::read(n.get("side")),
                line,
            })
        })
        .take(limits::NOTES - file_notes.len())
        .collect();
    Section {
        title: text(s.get("title")),
        category: Category::read(s.get("category")),
        summary: text(s.get("summary")),
        files,
        check: clip(&one_line(&text(s.get("check"))), limits::NOTE),
        risk: text(s.get("risk")),
        importance: Importance::read(s.get("importance"), s.get("critical")),
        file_notes,
        line_notes,
    }
}

/// A Mermaid source a model fenced anyway, unwrapped.
fn unfenced(diagram: &str) -> String {
    let is_fence = |l: &str| {
        let run = l.chars().take_while(|c| *c == '`').count();
        let tildes = l.chars().take_while(|c| *c == '~').count();
        run >= 3 || tildes >= 3
    };
    let mut body = diagram;
    if let Some((first, rest)) = body.split_once('\n')
        && is_fence(first)
    {
        let info = first.trim_start_matches(['`', '~']).trim();
        if info.is_empty() || info.eq_ignore_ascii_case("mermaid") {
            body = rest;
        }
    }
    if let Some((rest, last)) = body.rsplit_once('\n')
        && is_fence(last)
        && last.trim_start_matches(['`', '~']).is_empty()
    {
        body = rest;
    }
    body.trim().to_string()
}

/// A model's answer as a guide; None when it isn't the JSON asked for (the view then shows it
/// as Markdown). Fields it left out or mistyped read as empty, parts of a diagram it got wrong
/// are dropped and past the limits cut. A guide from before structured diagrams keeps its
/// Mermaid source, and its summary stands for the overview.
pub fn parse_guide(output: &str) -> Option<Guide> {
    let v = json_of(
        &normalized(output),
        &["title", "summary", "overview", "sections"],
    )?;
    let v = v.as_object()?;
    let sections: Vec<Section> = records(v.get("sections"))
        .into_iter()
        .map(parse_section)
        .filter(|s| !s.title.is_empty() || !s.summary.is_empty() || !s.files.is_empty())
        .take(limits::SECTIONS)
        .collect();
    let diagram = v.get("diagram").and_then(Value::as_object);
    let overview = text(v.get("overview"));
    let count = sections.len();
    let guide = Guide {
        title: text(v.get("title")),
        summary: if overview.is_empty() {
            text(v.get("summary"))
        } else {
            overview
        },
        diagram: unfenced(&text(v.get("diagram"))),
        models: parse_models(diagram.and_then(|d| d.get("models")), count),
        flows: parse_flows(diagram.and_then(|d| d.get("flows")), count),
        sections,
    };
    (!guide.title.is_empty() || !guide.summary.is_empty() || !guide.sections.is_empty())
        .then_some(guide)
}

/// A file a section names but doesn't show: where it shows (`at`, from 1), or None when the diff
/// doesn't have it.
#[derive(Clone, Debug, PartialEq)]
pub struct Named {
    pub path: String,
    pub at: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placed {
    /// Per section, the files whose diffs it shows (a file named twice shows in the first).
    pub shown: Vec<Vec<String>>,
    /// Per section, the others it names.
    pub named: Vec<Vec<Named>>,
    /// The files no section names, so none is left out of the review.
    pub rest: Vec<String>,
}

/// Where each changed file (`paths`) shows.
pub fn place_files(guide: &Guide, paths: &[String]) -> Placed {
    let all: HashSet<&str> = paths.iter().map(String::as_str).collect();
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut out = Placed::default();
    for (i, s) in guide.sections.iter().enumerate() {
        let (mut shown, mut named) = (vec![], vec![]);
        for f in &s.files {
            match match_path(f, &all) {
                None => named.push(Named {
                    path: f.clone(),
                    at: None,
                }),
                Some(path) => match at.get(path) {
                    None => {
                        at.insert(path.to_string(), i + 1);
                        shown.push(path.to_string());
                    }
                    Some(&n) if n != i + 1 => named.push(Named {
                        path: path.to_string(),
                        at: Some(n),
                    }),
                    Some(_) => {}
                },
            }
        }
        out.shown.push(shown);
        out.named.push(named);
    }
    out.rest = paths
        .iter()
        .filter(|p| !at.contains_key(p.as_str()))
        .cloned()
        .collect();
    out
}

/// "01", as sections are numbered.
pub fn section_badge(n: usize) -> String {
    format!("{n:02}")
}

/// A flow in words, a line a step: what a screen reader reads of each step's box.
pub fn describe_flow(flow: &Flow) -> Vec<String> {
    let label: HashMap<&str, &str> = flow
        .steps
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str()))
        .collect();
    flow.steps
        .iter()
        .map(|s| {
            let kind = if s.decision {
                "decision".to_string()
            } else {
                s.status.word().to_lowercase()
            };
            let where_ = if s.file.is_empty() {
                kind
            } else {
                format!("{kind} in {}", s.file)
            };
            let next: Vec<String> = flow
                .edges
                .iter()
                .filter(|e| e.from == s.id)
                .map(|e| {
                    let to = label.get(e.to.as_str()).copied().unwrap_or_default();
                    if e.label.is_empty() {
                        to.to_string()
                    } else {
                        format!("{}: {to}", e.label)
                    }
                })
                .collect();
            if next.is_empty() {
                format!("{} ({where_})", s.label)
            } else {
                format!("{} ({where_}), then {}", s.label, next.join("; "))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
