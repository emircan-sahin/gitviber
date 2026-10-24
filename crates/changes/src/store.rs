//! What Changes keeps across restarts, beside settings.json: the list's view and closed
//! sections and each worktree's commit draft in changes.json, and what was typed into a diff
//! and not saved in edits.json (apart, as whole files' text is written there as it's typed).
//! The worktrees used last are kept, as many as the old app kept; an unreadable file starts
//! fresh.

use crate::model::list::ChangesView;
use gitviber_ui::store::{config_dir, write_in_background};
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Agent worktrees come and go: what's kept for the ones used longest ago goes.
pub const MAX_ROOTS: usize = 30;
/// An edit this large (its text and the file's) isn't written to disk on each pause in the
/// typing; it lives until the app quits.
pub const MAX_EDIT_BYTES: usize = 4 << 20;

/// A commit message being written in a worktree.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Draft {
    pub summary: String,
    pub body: String,
    /// "Name <email>", added as Co-authored-by trailers.
    pub co_authors: Vec<String>,
}

/// What was typed into a file and not saved, and the file's text when the typing began.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Edit {
    pub text: String,
    pub base: String,
}

/// Values by worktree root, the one changed last at the end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ByRoot<T>(Vec<(String, T)>);

impl<T> Default for ByRoot<T> {
    fn default() -> Self {
        ByRoot(vec![])
    }
}

impl<T> ByRoot<T> {
    pub fn get(&self, root: &str) -> Option<&T> {
        self.0.iter().find(|(r, _)| r == root).map(|(_, v)| v)
    }

    /// Sets (or with None removes) `root`'s value; it becomes the newest.
    pub fn put(&mut self, root: &str, value: Option<T>) {
        self.0.retain(|(r, _)| r != root);
        if let Some(v) = value {
            self.0.push((root.to_string(), v));
            let over = self.0.len().saturating_sub(MAX_ROOTS);
            self.0.drain(..over);
        }
    }

    fn take(&mut self, root: &str) -> Option<T> {
        let at = self.0.iter().position(|(r, _)| r == root)?;
        Some(self.0.remove(at).1)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub tree: bool,
    /// Newest first, by modified time.
    pub recent: bool,
    /// Closed sections, by title.
    pub collapsed: Vec<String>,
    pub drafts: ByRoot<Draft>,
}

impl Global for Saved {}

/// Each worktree's files by path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Edits(ByRoot<BTreeMap<String, Edit>>);

impl Global for Edits {}

fn saved_path() -> PathBuf {
    config_dir().join("changes.json")
}

fn edits_path() -> PathBuf {
    config_dir().join("edits.json")
}

fn read<T: for<'de> Deserialize<'de> + Default>(path: &PathBuf) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn saved(cx: &mut App) -> &Saved {
    if !cx.has_global::<Saved>() {
        let saved = read(&saved_path());
        cx.set_global::<Saved>(saved);
    }
    cx.global::<Saved>()
}

fn update(cx: &mut App, f: impl FnOnce(&mut Saved)) {
    let mut next = saved(cx).clone();
    f(&mut next);
    if &next == cx.global::<Saved>() {
        return;
    }
    let text = serde_json::to_string_pretty(&next).unwrap_or_default();
    cx.set_global(next);
    write_in_background(cx, saved_path(), text);
}

fn edits(cx: &mut App) -> &mut Edits {
    if !cx.has_global::<Edits>() {
        let edits = read(&edits_path());
        cx.set_global::<Edits>(edits);
    }
    cx.global_mut::<Edits>()
}

fn write_edits(cx: &mut App) {
    let text = serde_json::to_string(cx.global::<Edits>()).unwrap_or_default();
    write_in_background(cx, edits_path(), text);
}

pub fn view(cx: &mut App) -> ChangesView {
    let s = saved(cx);
    ChangesView {
        tree: s.tree,
        recent: s.recent,
    }
}

pub fn set_view(view: ChangesView, cx: &mut App) {
    update(cx, |s| {
        s.tree = view.tree;
        s.recent = view.recent;
    });
}

pub fn collapsed(cx: &mut App) -> Vec<String> {
    saved(cx).collapsed.clone()
}

pub fn set_collapsed(titles: Vec<String>, cx: &mut App) {
    update(cx, |s| s.collapsed = titles);
}

pub fn draft(root: &str, cx: &mut App) -> Option<Draft> {
    saved(cx).drafts.get(root).cloned()
}

/// None once there's nothing of the user's in it: committed, or back to how it started.
pub fn set_draft(root: &str, draft: Option<Draft>, cx: &mut App) {
    if saved(cx).drafts.get(root) == draft.as_ref() {
        return;
    }
    update(cx, |s| s.drafts.put(root, draft));
}

pub fn edit(root: &str, path: &str, cx: &mut App) -> Option<Edit> {
    edits(cx).0.get(root)?.get(path).cloned()
}

/// Keeps (or with None forgets) the typing in `root`'s `path`. One too large to write on each
/// pause isn't kept on disk: an older one of the file goes rather than come back stale.
pub fn set_edit(root: &str, path: &str, edit: Option<Edit>, cx: &mut App) {
    let edit = edit.filter(|e| e.text.len() + e.base.len() <= MAX_EDIT_BYTES);
    let all = edits(cx);
    if all.0.get(root).and_then(|files| files.get(path)) == edit.as_ref() {
        return;
    }
    let mut files = all.0.take(root).unwrap_or_default();
    match edit {
        Some(e) => {
            files.insert(path.to_string(), e);
        }
        None => {
            files.remove(path);
        }
    }
    all.0.put(root, (!files.is_empty()).then_some(files));
    write_edits(cx);
}

/// The old app's, on its first launch here: the view, the closed sections, the drafts with
/// something of the user's in them, and the unsaved edits.
fn from_legacy(value: impl Fn(&str) -> Option<Value>) -> (Saved, Edits) {
    let records = |key: &str| match value(key) {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    let string = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    let view = value("gitviber.changes-view").unwrap_or_default();
    let collapsed = match value("gitviber.changes-sections.collapsed") {
        Some(Value::Array(titles)) => titles
            .iter()
            .filter_map(|t| t.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    };
    let mut drafts = ByRoot::default();
    for (root, d) in records("gitviber.drafts") {
        let (Some(summary), Some(body)) = (string(&d, "summary"), string(&d, "body")) else {
            continue;
        };
        let co_authors: Vec<String> = match d.get("coAuthors") {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect(),
            _ => vec![],
        };
        // A draft as it started (a prepared message, commit.template) is the repo's, not theirs.
        let from = d
            .get("from")
            .and_then(|f| Some((string(f, "summary")?, string(f, "body")?)));
        let untouched = from.is_some_and(|f| f == (summary.clone(), body.clone()))
            || (summary.trim().is_empty() && body.trim().is_empty());
        if untouched && co_authors.is_empty() {
            continue;
        }
        drafts.put(
            &root,
            Some(Draft {
                summary,
                body,
                co_authors,
            }),
        );
    }
    let mut edits = ByRoot::default();
    for (root, files) in records("gitviber.fileEdits") {
        let Value::Object(files) = files else {
            continue;
        };
        let files: BTreeMap<String, Edit> = files
            .into_iter()
            .filter_map(|(path, e)| {
                Some((
                    path,
                    Edit {
                        text: string(&e, "text")?,
                        base: string(&e, "base")?,
                    },
                ))
            })
            .filter(|(_, e)| e.text != e.base)
            .collect();
        if !files.is_empty() {
            edits.put(&root, Some(files));
        }
    }
    let saved = Saved {
        tree: view.get("tree").and_then(Value::as_bool).unwrap_or(false),
        recent: view.get("sort").and_then(Value::as_str) == Some("recent"),
        collapsed,
        drafts,
    };
    (saved, Edits(edits))
}

/// On the first launch, what the old app kept, unless this build has files of its own.
pub(crate) fn import_legacy(cx: &mut App) {
    let (saved, imported) = from_legacy(gitviber_ui::legacy::value);
    if saved != Saved::default() && !saved_path().exists() {
        update(cx, |s| *s = saved);
    }
    if imported != Edits::default() && !edits_path().exists() {
        cx.set_global(imported);
        write_edits(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_apps_view_sections_drafts_and_edits_come_over() {
        let old = serde_json::json!({
            "gitviber.changes-view": {"tree": true, "sort": "recent"},
            "gitviber.changes-sections.collapsed": ["Stashes", 3, "Staged"],
            "gitviber.drafts": {
                "/p/a": {"summary": "Fix it", "body": "", "coAuthors": ["Ann <a@x>", 1]},
                "/p/template": {"summary": "", "body": "# why", "coAuthors": [], "from": {"summary": "", "body": "# why"}},
                "/p/empty": {"summary": " ", "body": "", "coAuthors": []},
                "/p/broken": {"summary": 3, "body": ""},
                "/p/b": {"summary": "", "body": "notes", "coAuthors": []},
            },
            "gitviber.fileEdits": {
                "/p/a": {"src/x.rs": {"text": "new", "base": "old"}, "same.rs": {"text": "s", "base": "s"}, "bad.rs": {"text": 1}},
                "/p/none": {"same.rs": {"text": "s", "base": "s"}},
            },
        });
        let (saved, edits) = from_legacy(|key| old.get(key).cloned());
        assert!(saved.tree && saved.recent);
        assert_eq!(saved.collapsed, ["Stashes", "Staged"]);
        let roots: Vec<&str> = saved.drafts.0.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(
            roots,
            ["/p/a", "/p/b"],
            "in the old order, the untouched and broken left out"
        );
        assert_eq!(
            saved.drafts.get("/p/a"),
            Some(&Draft {
                summary: "Fix it".into(),
                body: "".into(),
                co_authors: vec!["Ann <a@x>".into()]
            })
        );
        assert_eq!(edits.0.0.len(), 1);
        let files = edits.0.get("/p/a").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(
            files["src/x.rs"],
            Edit {
                text: "new".into(),
                base: "old".into()
            }
        );
        assert_eq!(from_legacy(|_| None), (Saved::default(), Edits::default()));
    }

    #[test]
    fn the_worktrees_used_last_are_kept() {
        let mut by = ByRoot::default();
        for i in 0..MAX_ROOTS + 5 {
            by.put(&format!("/r{i}"), Some(i));
        }
        by.put("/r10", Some(100));
        assert_eq!(by.0.len(), MAX_ROOTS);
        assert!(by.get("/r0").is_none() && by.get("/r4").is_none());
        assert_eq!(by.0.last(), Some(&("/r10".to_string(), 100)));
        by.put("/r10", None);
        assert!(by.get("/r10").is_none());
        assert_eq!(by.0.len(), MAX_ROOTS - 1);
    }

    #[test]
    fn the_files_read_back_as_written() {
        let mut saved = Saved {
            tree: true,
            collapsed: vec!["Stashes".into()],
            ..Saved::default()
        };
        saved.drafts.put(
            "/p",
            Some(Draft {
                summary: "s".into(),
                body: "b".into(),
                co_authors: vec![],
            }),
        );
        let text = serde_json::to_string(&saved).unwrap();
        assert_eq!(serde_json::from_str::<Saved>(&text).unwrap(), saved);
        let mut edits = Edits::default();
        edits.0.put(
            "/p",
            Some(BTreeMap::from([(
                "a".to_string(),
                Edit {
                    text: "t".into(),
                    base: "b".into(),
                },
            )])),
        );
        let text = serde_json::to_string(&edits).unwrap();
        assert_eq!(serde_json::from_str::<Edits>(&text).unwrap(), edits);
        // A file from a later build, or a damaged one, reads as what it can.
        assert!(
            serde_json::from_str::<Saved>(r#"{"tree": true, "later": 1}"#)
                .unwrap()
                .tree
        );
    }
}
