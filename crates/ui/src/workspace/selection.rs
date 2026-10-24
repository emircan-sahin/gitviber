//! What a center tab shows, as data: the tab's identity (`selection_key`), what's saved for a
//! worktree's tabs, and what an opener builds the view from. Feature crates register an opener
//! per kind (`register_opener`), so one feature opens another's tab without depending on it.
//! Stored as the old app stored it: `{"kind": "unstaged", "file": {...}}`.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gitviber_core::git::{Commit, FileChange, Nested};
use gitviber_core::github::{Issue, Label, Pull};
use gpui_kit::{App, Global, SharedString, Window};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::item::{TabHandle, TabIcon};

/// A PR's diff range (merge base → head), or a branch's in a comparison (no `number`, its
/// name as `label`). Some of a PR's commits have `number` and `label` (their short ids).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// The PR's page, for its line comments; none for some of its commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub base: String,
    pub head: String,
}

impl PullRange {
    /// The dots between its ends as git writes them, unless it has a name.
    pub fn label(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| format!("{}..{}", short(&self.base), short(&self.head)))
    }

    /// A fork's #3 and its original's #3 differ, and so do the PR and one of its commits.
    fn scope(&self) -> String {
        format!(
            "{}@{}..{}",
            self.number.map(|n| n.to_string()).unwrap_or_default(),
            self.base,
            self.head
        )
    }
}

/// A side of a comparison: a full ref or a commit id, and what to call it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparePoint {
    #[serde(rename = "ref")]
    pub reference: String,
    pub label: String,
}

/// The lists Changes shows, which open whole as one stacked diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeList {
    Unstaged,
    Staged,
    Branch,
}

/// Every file of a list in one scroll.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "list", rename_all = "lowercase")]
pub enum ChangesOf {
    Unstaged,
    Staged,
    Branch,
    Commit {
        #[serde(serialize_with = "ser", deserialize_with = "de_commit")]
        commit: Arc<Commit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    Range {
        range: PullRange,
    },
}

/// An agent's guided review.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "of", rename_all = "lowercase")]
pub enum GuideOf {
    Commit {
        #[serde(serialize_with = "ser", deserialize_with = "de_commit")]
        commit: Arc<Commit>,
    },
    /// HEAD's branch since `base` (a full ref), `label` its short name.
    Branch { base: String, label: String },
    /// A pull request; `target`: the repo it's on ("owner/name"), None for the origin.
    Pull {
        #[serde(serialize_with = "ser", deserialize_with = "de_pull")]
        pull: Arc<Pull>,
        #[serde(default)]
        target: Option<String>,
    },
    /// The worktree's uncommitted changes.
    Changes,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Selection {
    #[serde(rename = "unstaged")]
    Unstaged {
        #[serde(serialize_with = "ser", deserialize_with = "de_file")]
        file: Arc<FileChange>,
    },
    #[serde(rename = "staged")]
    Staged {
        #[serde(serialize_with = "ser", deserialize_with = "de_file")]
        file: Arc<FileChange>,
    },
    #[serde(rename = "conflict")]
    Conflict {
        #[serde(serialize_with = "ser", deserialize_with = "de_file")]
        file: Arc<FileChange>,
    },
    /// A commit's file; `url` its GitHub page, when it's there.
    #[serde(rename = "commit")]
    Commit {
        #[serde(serialize_with = "ser", deserialize_with = "de_commit")]
        commit: Arc<Commit>,
        #[serde(serialize_with = "ser", deserialize_with = "de_file")]
        file: Arc<FileChange>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    #[serde(rename = "file")]
    File { path: String },
    #[serde(rename = "pull")]
    Pull {
        #[serde(serialize_with = "ser", deserialize_with = "de_pull")]
        pull: Arc<Pull>,
    },
    #[serde(rename = "issue")]
    Issue {
        #[serde(serialize_with = "ser", deserialize_with = "de_issue")]
        issue: Arc<Issue>,
    },
    #[serde(rename = "pr-file")]
    PrFile {
        range: PullRange,
        #[serde(serialize_with = "ser", deserialize_with = "de_file")]
        file: Arc<FileChange>,
    },
    /// A branch under review: `base` is the merge base, `label` the branch it was compared with;
    /// `fixed`: History's comparison of the working tree with one commit.
    #[serde(rename = "branch")]
    Branch {
        base: String,
        label: String,
        #[serde(serialize_with = "ser", deserialize_with = "de_file")]
        file: Arc<FileChange>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        fixed: bool,
    },
    #[serde(rename = "changes")]
    Changes {
        #[serde(flatten)]
        of: ChangesOf,
    },
    /// The Compare screen: `head` against `base`, from their merge base or as they stand.
    #[serde(rename = "compare")]
    Compare {
        base: ComparePoint,
        head: ComparePoint,
        #[serde(rename = "mergeBase")]
        merge_base: bool,
    },
    /// Two working-tree files side by side: `file.old_path` against `file.path`.
    #[serde(rename = "files")]
    Files {
        #[serde(serialize_with = "ser", deserialize_with = "de_file")]
        file: Arc<FileChange>,
    },
    /// A file in an Obsidian vault (`vault`: its folder); `path` is vault-relative.
    #[serde(rename = "vault")]
    Vault { vault: String, path: String },
    /// A file outside the repo that a terminal link named, read-only.
    #[serde(rename = "external")]
    External { path: String },
    #[serde(rename = "guide")]
    Guide {
        #[serde(flatten)]
        of: GuideOf,
    },
    /// A web page: `id` names the tab's native view, whatever it loads. `title` is the page's
    /// last; `device`, `zoom` and `scheme` how it shows it (the browser crate's to read).
    #[serde(rename = "browser")]
    Browser {
        id: String,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zoom: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scheme: Option<String>,
    },
}

impl std::fmt::Debug for Selection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&selection_key(self))
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

const BLANK: &str = "about:blank";

/// A page's address as a tab names it: no scheme, no lone trailing slash.
pub fn page_label(url: &str) -> String {
    if url.is_empty() || url == BLANK {
        return "New Tab".into();
    }
    let label = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    match label.strip_suffix('/') {
        Some(trimmed) if !trimmed.contains('/') => trimmed.to_string(),
        _ => label.to_string(),
    }
}

impl Selection {
    /// The kind openers are registered under ("unstaged", "pr-file", "changes"…).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unstaged { .. } => "unstaged",
            Self::Staged { .. } => "staged",
            Self::Conflict { .. } => "conflict",
            Self::Commit { .. } => "commit",
            Self::File { .. } => "file",
            Self::Pull { .. } => "pull",
            Self::Issue { .. } => "issue",
            Self::PrFile { .. } => "pr-file",
            Self::Branch { .. } => "branch",
            Self::Changes { .. } => "changes",
            Self::Compare { .. } => "compare",
            Self::Files { .. } => "files",
            Self::Vault { .. } => "vault",
            Self::External { .. } => "external",
            Self::Guide { .. } => "guide",
            Self::Browser { .. } => "browser",
        }
    }

    /// The kind its opener is registered under: `kind()`, but for whole lists, which two crates
    /// open: "changes" (the unstaged, staged and branch lists), "changes-commit" (a commit's
    /// files), "changes-range" (a comparison's or a PR's).
    pub fn opener_kind(&self) -> &'static str {
        match self {
            Self::Changes {
                of: ChangesOf::Commit { .. },
            } => "changes-commit",
            Self::Changes {
                of: ChangesOf::Range { .. },
            } => "changes-range",
            _ => self.kind(),
        }
    }

    /// The repo file it shows, for file-like selections.
    pub fn file(&self) -> Option<&FileChange> {
        match self {
            Self::Unstaged { file }
            | Self::Staged { file }
            | Self::Conflict { file }
            | Self::Commit { file, .. }
            | Self::PrFile { file, .. }
            | Self::Branch { file, .. }
            | Self::Files { file } => Some(file),
            _ => None,
        }
    }

    /// The explorer's comparison of two working-tree files, `left` the old side.
    pub fn files(left: &str, right: &str) -> Self {
        Self::Files {
            file: Arc::new(FileChange {
                path: right.into(),
                old_path: Some(left.into()),
                status: "M".into(),
                additions: None,
                deletions: None,
                oid: None,
                index_oid: None,
                conflict: None,
                mode: None,
                submodule: None,
                nested: None,
            }),
        }
    }

    /// The tab's title before its view is built: the file's name, else what `selection_path` says.
    pub fn title(&self) -> SharedString {
        let path = selection_path(self);
        let file_like = self.file().is_some()
            || matches!(
                self,
                Self::File { .. } | Self::Vault { .. } | Self::External { .. }
            );
        if file_like {
            path.rsplit('/').next().unwrap_or(&path).to_string().into()
        } else {
            path.into()
        }
    }

    /// The icon before an unbuilt tab's title.
    pub fn icon(&self) -> TabIcon {
        match self {
            Self::File { path } | Self::Vault { path, .. } | Self::External { path } => {
                TabIcon::File(path.clone().into())
            }
            _ => match self.file() {
                Some(f) => TabIcon::File(f.path.clone().into()),
                None => TabIcon::None,
            },
        }
    }

    /// The faint word after a tab's title, so a file's diff and the file don't read alike. Read
    /// from the selection, so a restored tab whose view isn't built yet keeps it.
    pub fn detail(&self) -> Option<SharedString> {
        Some(match self {
            Self::Unstaged { .. } => "diff".into(),
            Self::Staged { .. } => "staged".into(),
            Self::Conflict { .. } => "conflict".into(),
            Self::Branch { .. } => "branch".into(),
            Self::Commit { commit, .. } => commit.short_sha.clone().into(),
            Self::PrFile { range, .. } => match (range.number, &range.label) {
                (Some(n), None) => format!("#{n}").into(),
                _ => range.label().into(),
            },
            Self::Compare {
                base,
                head,
                merge_base,
            } => {
                let dots = if *merge_base { "..." } else { ".." };
                format!("{}{dots}{}", base.label, head.label).into()
            }
            Self::External { .. } => "read-only".into(),
            _ => return None,
        })
    }

    /// The tab's file is in the working tree: not a commit's or a pull request's version, nor deleted.
    pub fn on_disk(&self) -> bool {
        match self {
            Self::File { .. } => true,
            Self::Unstaged { file }
            | Self::Staged { file }
            | Self::Conflict { file }
            | Self::Branch { file, .. } => file.status != "D",
            _ => false,
        }
    }

    /// What unsaved edits to the tab's file are kept under: a repo file by its path, a vault's
    /// by its tab's key. None for anything that can't be typed into.
    pub fn edit_path(&self) -> Option<String> {
        match self {
            Self::File { .. } | Self::Unstaged { .. } => Some(selection_path(self)),
            Self::Vault { .. } => Some(selection_key(self)),
            _ => None,
        }
    }
}

// The unstaged list is what the Changes panel's "Changes" section holds, not staged ones too.
const LIST_TITLES: [(ChangeList, &str); 3] = [
    (ChangeList::Unstaged, "All Unstaged Changes"),
    (ChangeList::Staged, "All Staged Changes"),
    (ChangeList::Branch, "All Branch Changes"),
];

fn list_title(list: ChangeList) -> &'static str {
    LIST_TITLES
        .iter()
        .find(|(l, _)| *l == list)
        .map(|(_, t)| *t)
        .unwrap_or_default()
}

/// A file path for file-like tabs; for a PR, an issue or a list, a label.
pub fn selection_path(s: &Selection) -> String {
    match s {
        Selection::File { path } | Selection::Vault { path, .. } | Selection::External { path } => {
            path.clone()
        }
        Selection::Pull { pull } => format!("#{} {}", pull.number, pull.title),
        Selection::Issue { issue } => format!("#{} {}", issue.number, issue.title),
        Selection::Changes { of } => match of {
            ChangesOf::Commit { commit, .. } => format!("Commit {}", commit.short_sha),
            ChangesOf::Range { range } => format!("All Changes · {}", range.label()),
            ChangesOf::Unstaged => list_title(ChangeList::Unstaged).into(),
            ChangesOf::Staged => list_title(ChangeList::Staged).into(),
            ChangesOf::Branch => list_title(ChangeList::Branch).into(),
        },
        Selection::Compare { .. } => "Compare".into(),
        Selection::Guide { of } => match of {
            GuideOf::Commit { commit } => format!("Explain {}", commit.short_sha),
            GuideOf::Pull { pull, .. } => format!("Guided Review · #{}", pull.number),
            GuideOf::Changes => "Guided Review · Uncommitted".into(),
            GuideOf::Branch { label, .. } => format!("Guided Review · {label}"),
        },
        Selection::Browser { url, .. } => page_label(url),
        _ => s.file().map(|f| f.path.clone()).unwrap_or_default(),
    }
}

/// Identity of what a tab shows; also how a list row finds its open tab.
pub fn selection_key(s: &Selection) -> String {
    match s {
        // By url: a fork's #3 and its original's #3 are different threads.
        Selection::Pull { pull } => return format!("pull:{}", pull.url),
        Selection::Issue { issue } => return format!("issue:{}", issue.url),
        Selection::Vault { vault, path } => return format!("vault:{vault}:{path}"),
        Selection::External { path } => return format!("external:{path}"),
        // The same tab as its page moves on.
        Selection::Browser { id, .. } => return format!("browser::{id}"),
        _ => {}
    }
    let scope = match s {
        Selection::Commit { commit, .. } => commit.sha.clone(),
        Selection::PrFile { range, .. } => range.scope(),
        Selection::Changes {
            of: ChangesOf::Commit { commit, .. },
        } => commit.sha.clone(),
        Selection::Changes {
            of: ChangesOf::Range { range },
        } => range.scope(),
        Selection::Branch { base, .. } => base.clone(),
        Selection::Files { file } => file.old_path.clone().unwrap_or_default(),
        Selection::Guide { of } => match of {
            GuideOf::Commit { commit } => commit.sha.clone(),
            GuideOf::Pull { pull, .. } => pull.url.clone(),
            GuideOf::Changes => "changes".into(),
            GuideOf::Branch { base, .. } => base.clone(),
        },
        _ => String::new(),
    };
    format!("{}:{scope}:{}", s.kind(), selection_path(s))
}

fn ser<T: Serialize, S: Serializer>(value: &Arc<T>, s: S) -> Result<S::Ok, S::Error> {
    value.as_ref().serialize(s)
}

// What core's types serialize to, read back; a field an older build didn't write is its default.

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct StoredFile {
    path: String,
    old_path: Option<String>,
    status: String,
    additions: Option<u32>,
    deletions: Option<u32>,
    oid: Option<String>,
    index_oid: Option<String>,
    conflict: Option<String>,
    mode: Option<String>,
    submodule: Option<String>,
    nested: Option<StoredNested>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct StoredNested {
    path: String,
}

fn de_file<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<FileChange>, D::Error> {
    let f = StoredFile::deserialize(d)?;
    Ok(Arc::new(FileChange {
        path: f.path,
        old_path: f.old_path,
        status: f.status,
        additions: f.additions,
        deletions: f.deletions,
        oid: f.oid,
        index_oid: f.index_oid,
        conflict: f.conflict,
        mode: f.mode,
        submodule: f.submodule,
        nested: f.nested.map(|n| Nested { path: n.path }),
    }))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct StoredCommit {
    sha: String,
    short_sha: String,
    author_name: String,
    author_email: String,
    timestamp: i64,
    committer_name: String,
    committed_at: i64,
    parents: Vec<String>,
    refs: Vec<String>,
    subject: String,
    body: String,
    unpushed: bool,
    on_origin: bool,
    not_in_head: bool,
    file: Option<String>,
}

fn de_commit<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Commit>, D::Error> {
    let c = StoredCommit::deserialize(d)?;
    Ok(Arc::new(Commit {
        short_sha: if c.short_sha.is_empty() {
            short(&c.sha).to_string()
        } else {
            c.short_sha
        },
        sha: c.sha,
        author_name: c.author_name,
        author_email: c.author_email,
        timestamp: c.timestamp,
        committer_name: c.committer_name,
        committed_at: c.committed_at,
        parents: c.parents,
        refs: c.refs,
        subject: c.subject,
        body: c.body,
        unpushed: c.unpushed,
        on_origin: c.on_origin,
        not_in_head: c.not_in_head,
        file: c.file,
    }))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct StoredPull {
    number: u64,
    title: String,
    state: String,
    draft: bool,
    author: String,
    head_ref: String,
    head_sha: String,
    head_repo: Option<String>,
    base_ref: String,
    base_sha: String,
    created_at: String,
    updated_at: String,
    url: String,
}

fn de_pull<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Pull>, D::Error> {
    let p = StoredPull::deserialize(d)?;
    Ok(Arc::new(Pull {
        number: p.number,
        title: p.title,
        state: p.state,
        draft: p.draft,
        author: p.author,
        head_ref: p.head_ref,
        head_sha: p.head_sha,
        head_repo: p.head_repo,
        base_ref: p.base_ref,
        base_sha: p.base_sha,
        created_at: p.created_at,
        updated_at: p.updated_at,
        url: p.url,
    }))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct StoredLabel {
    name: String,
    color: String,
    description: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct StoredIssue {
    number: u64,
    title: String,
    state: String,
    state_reason: Option<String>,
    author: String,
    labels: Vec<StoredLabel>,
    assignees: Vec<String>,
    comments: u64,
    created_at: String,
    updated_at: String,
    url: String,
}

fn de_issue<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Issue>, D::Error> {
    let i = StoredIssue::deserialize(d)?;
    Ok(Arc::new(Issue {
        number: i.number,
        title: i.title,
        state: i.state,
        state_reason: i.state_reason,
        author: i.author,
        labels: i
            .labels
            .into_iter()
            .map(|l| Label {
                name: l.name,
                color: l.color,
                description: l.description,
            })
            .collect(),
        assignees: i.assignees,
        comments: i.comments,
        created_at: i.created_at,
        updated_at: i.updated_at,
        url: i.url,
    }))
}

/// Builds a tab's view for a selection of its kind; None when it can't (the file is gone).
pub type Opener = Rc<dyn Fn(&Selection, &mut Window, &mut App) -> Option<TabHandle>>;

#[derive(Default)]
struct Openers(HashMap<&'static str, Opener>);

impl Global for Openers {}

/// Makes `open` the builder of `kind`'s tabs, replacing one registered before: a
/// `Selection::opener_kind()` ("unstaged", "file", "pull", "changes-commit"…). One feature can
/// register several kinds with the same function.
pub fn register_opener(
    cx: &mut App,
    kind: &'static str,
    open: impl Fn(&Selection, &mut Window, &mut App) -> Option<TabHandle> + 'static,
) {
    cx.default_global::<Openers>().0.insert(kind, Rc::new(open));
}

pub fn opener(kind: &str, cx: &App) -> Option<Opener> {
    cx.try_global::<Openers>()?.0.get(kind).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: &str) -> Arc<Commit> {
        de_commit(serde_json::json!({ "sha": sha, "subject": "s" })).unwrap()
    }

    #[test]
    fn two_files_compared_make_one_tab_per_pair() {
        let ab = Selection::files("src/a.ts", "src/b.ts");
        assert_eq!(selection_path(&ab), "src/b.ts");
        assert_eq!(
            selection_key(&ab),
            selection_key(&Selection::files("src/a.ts", "src/b.ts"))
        );
        assert_ne!(
            selection_key(&ab),
            selection_key(&Selection::files("src/c.ts", "src/b.ts"))
        );
        assert_ne!(
            selection_key(&ab),
            selection_key(&Selection::files("src/b.ts", "src/a.ts"))
        );
        assert_ne!(
            selection_key(&ab),
            selection_key(&Selection::File {
                path: "src/b.ts".into()
            })
        );
        assert!(!ab.on_disk());
    }

    #[test]
    fn a_commits_list_is_its_own_tab() {
        let a = Selection::Changes {
            of: ChangesOf::Commit {
                commit: commit(&"a".repeat(40)),
                url: None,
            },
        };
        let a_url = Selection::Changes {
            of: ChangesOf::Commit {
                commit: commit(&"a".repeat(40)),
                url: Some("https://example.test/c".into()),
            },
        };
        assert_eq!(selection_key(&a), selection_key(&a_url));
        let b = Selection::Changes {
            of: ChangesOf::Commit {
                commit: commit(&"b".repeat(40)),
                url: None,
            },
        };
        assert_ne!(selection_key(&a), selection_key(&b));
        assert_eq!(
            selection_key(&Selection::Changes {
                of: ChangesOf::Branch
            }),
            "changes::All Branch Changes"
        );
        assert_eq!(selection_path(&a), "Commit aaaaaaa");
    }

    #[test]
    fn a_range_is_told_apart_by_its_ends_and_by_being_a_prs() {
        let range = PullRange {
            number: None,
            pull_url: None,
            label: Some("main...feature".into()),
            base: "1".repeat(40),
            head: "2".repeat(40),
        };
        let key = |r: PullRange| {
            selection_key(&Selection::Changes {
                of: ChangesOf::Range { range: r },
            })
        };
        assert_ne!(
            key(range.clone()),
            key(PullRange {
                head: "3".repeat(40),
                ..range.clone()
            })
        );
        assert_ne!(
            key(range.clone()),
            key(PullRange {
                number: Some(4),
                ..range.clone()
            })
        );
        assert_eq!(
            selection_path(&Selection::Changes {
                of: ChangesOf::Range {
                    range: range.clone()
                }
            }),
            "All Changes · main...feature"
        );
        let plain = PullRange {
            label: None,
            ..range
        };
        assert_eq!(
            selection_path(&Selection::Changes {
                of: ChangesOf::Range { range: plain }
            }),
            "All Changes · 1111111..2222222"
        );
    }

    #[test]
    fn the_compare_screen_is_one_tab() {
        let side = |l: &str| ComparePoint {
            reference: format!("refs/heads/{l}"),
            label: l.into(),
        };
        let one = Selection::Compare {
            base: side("main"),
            head: side("a"),
            merge_base: true,
        };
        let two = Selection::Compare {
            base: side("dev"),
            head: side("b"),
            merge_base: false,
        };
        assert_eq!(selection_key(&one), selection_key(&two));
        assert_eq!(selection_path(&one), "Compare");
        assert_eq!(one.detail().as_deref(), Some("main...a"));
        assert_eq!(two.detail().as_deref(), Some("dev..b"));
    }

    #[test]
    fn a_tab_not_built_yet_keeps_its_detail() {
        let file = || de_file(serde_json::json!({ "path": "src/main.rs", "status": "M" })).unwrap();
        let detail = |s: Selection| s.detail().map(|d| d.to_string());
        assert_eq!(
            detail(Selection::Unstaged { file: file() }).as_deref(),
            Some("diff")
        );
        assert_eq!(
            detail(Selection::Staged { file: file() }).as_deref(),
            Some("staged")
        );
        assert_eq!(
            detail(Selection::Conflict { file: file() }).as_deref(),
            Some("conflict")
        );
        assert_eq!(
            detail(Selection::Commit {
                commit: commit(&"a".repeat(40)),
                file: file(),
                url: None
            })
            .as_deref(),
            Some("aaaaaaa")
        );
        let range = PullRange {
            number: Some(7),
            pull_url: None,
            label: None,
            base: "1".repeat(40),
            head: "2".repeat(40),
        };
        let pr = |range| Selection::PrFile {
            range,
            file: file(),
        };
        assert_eq!(detail(pr(range.clone())).as_deref(), Some("#7"));
        assert_eq!(
            detail(pr(PullRange {
                label: Some("main...x".into()),
                ..range
            }))
            .as_deref(),
            Some("main...x")
        );
        // The file itself has none, so it reads apart from its diff.
        assert_eq!(
            detail(Selection::File {
                path: "src/main.rs".into()
            }),
            None
        );
        assert_eq!(
            detail(Selection::Changes {
                of: ChangesOf::Unstaged
            }),
            None
        );
        assert_eq!(
            selection_path(&Selection::Changes {
                of: ChangesOf::Unstaged
            }),
            "All Unstaged Changes"
        );
    }

    #[test]
    fn a_browser_tab_is_one_tab_wherever_its_page_goes() {
        let at = |id: &str, url: &str| Selection::Browser {
            id: id.into(),
            url: url.into(),
            title: None,
            device: None,
            zoom: None,
            scheme: None,
        };
        assert_eq!(
            selection_key(&at("tab-1", "http://localhost:5173/")),
            selection_key(&at("tab-1", "http://localhost:5173/docs"))
        );
        assert_ne!(
            selection_key(&at("tab-1", "http://localhost:5173/")),
            selection_key(&at("tab-2", "http://localhost:5173/"))
        );
        assert_eq!(
            selection_path(&at("tab-1", "http://localhost:5173/")),
            "localhost:5173"
        );
        assert_eq!(selection_path(&at("tab-1", "about:blank")), "New Tab");
        assert!(at("tab-1", "x").file().is_none());
        assert!(at("tab-1", "x").edit_path().is_none());
    }

    #[test]
    fn stored_as_the_old_app_stored_it() {
        let json = serde_json::json!({
            "kind": "unstaged",
            "file": { "path": "src/a.rs", "status": "M", "additions": 3, "deletions": null }
        });
        let s: Selection = serde_json::from_value(json).unwrap();
        assert_eq!(s.kind(), "unstaged");
        assert_eq!(selection_key(&s), "unstaged::src/a.rs");
        assert_eq!(s.title(), "a.rs");
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["kind"], "unstaged");
        assert_eq!(back["file"]["path"], "src/a.rs");
        assert_eq!(back["file"]["additions"], 3);

        let list: Selection =
            serde_json::from_value(serde_json::json!({ "kind": "changes", "list": "staged" }))
                .unwrap();
        assert_eq!(selection_path(&list), "All Staged Changes");
        let guide: Selection = serde_json::from_value(serde_json::json!({ "kind": "guide", "of": "branch", "base": "refs/heads/main", "label": "main" })).unwrap();
        assert_eq!(selection_path(&guide), "Guided Review · main");
        let round: Selection =
            serde_json::from_value(serde_json::to_value(&guide).unwrap()).unwrap();
        assert_eq!(selection_key(&round), selection_key(&guide));
        assert!(
            serde_json::from_value::<Selection>(serde_json::json!({ "kind": "nope" })).is_err()
        );
    }

    #[test]
    fn whole_lists_open_by_their_own_kinds() {
        let commit = Selection::Changes {
            of: ChangesOf::Commit {
                commit: commit(&"c".repeat(40)),
                url: None,
            },
        };
        let range = PullRange {
            number: None,
            pull_url: None,
            label: None,
            base: "1".into(),
            head: "2".into(),
        };
        assert_eq!(
            Selection::Changes {
                of: ChangesOf::Unstaged
            }
            .opener_kind(),
            "changes"
        );
        assert_eq!(
            Selection::Changes {
                of: ChangesOf::Branch
            }
            .opener_kind(),
            "changes"
        );
        assert_eq!(commit.opener_kind(), "changes-commit");
        assert_eq!(
            Selection::Changes {
                of: ChangesOf::Range { range }
            }
            .opener_kind(),
            "changes-range"
        );
        assert_eq!(commit.kind(), "changes");
        assert_eq!(Selection::files("a", "b").opener_kind(), "files");
    }
}
