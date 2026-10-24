//! What the pickers remember per project: pinned branches, where new worktrees go and what runs
//! in their terminal, and the colors picked for worktrees. refs.json beside settings.json; an
//! unreadable file starts fresh.

use std::collections::BTreeMap;

use gitviber_ui::store::{config_dir, write_in_background};
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Saved {
    /// By main worktree, in pin order.
    pub pinned_branches: BTreeMap<String, Vec<String>>,
    /// By main worktree: the folder its new worktrees go in, when not the default beside it.
    pub worktree_dirs: BTreeMap<String, String>,
    /// By main worktree: the command its last new worktree's terminal ran.
    pub worktree_run: BTreeMap<String, String>,
    /// By main worktree: the same, for a worktree started from an issue (`claude "Fix #{issue}"`).
    pub issue_run: BTreeMap<String, String>,
    /// By worktree path: a hue name, or "none".
    pub worktree_colors: BTreeMap<String, String>,
}

impl Global for Saved {}

fn path() -> std::path::PathBuf {
    config_dir().join("refs.json")
}

pub fn get(cx: &mut App) -> &Saved {
    if !cx.has_global::<Saved>() {
        let saved = std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        cx.set_global::<Saved>(saved);
    }
    cx.global::<Saved>()
}

pub fn update(cx: &mut App, f: impl FnOnce(&mut Saved)) {
    get(cx);
    let mut next = cx.global::<Saved>().clone();
    f(&mut next);
    if &next == cx.global::<Saved>() {
        return;
    }
    let text = serde_json::to_string_pretty(&next).unwrap_or_default();
    cx.set_global(next);
    write_in_background(cx, path(), text);
}

/// The old app's, as it kept them by path (its first launch here): pins, worktree folders and
/// runs, colors. Its runs for worktrees started from an issue have no place here yet.
fn from_legacy(value: impl Fn(&str) -> Option<Value>) -> Saved {
    let records = |key: &str| match value(key) {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    let strings = |key: &str| {
        records(key)
            .into_iter()
            .filter_map(|(k, v)| Some((k, v.as_str()?.to_string())))
            .filter(|(_, v)| !v.is_empty())
            .collect()
    };
    let pinned_branches = records("gitviber.pinnedBranches")
        .into_iter()
        .filter_map(|(main, v)| {
            let names: Vec<String> = v
                .as_array()?
                .iter()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect();
            (!names.is_empty()).then_some((main, names))
        })
        .collect();
    Saved {
        pinned_branches,
        worktree_dirs: strings("gitviber.worktreeDirs"),
        worktree_run: strings("gitviber.worktreeRun"),
        issue_run: strings("gitviber.issueRun"),
        worktree_colors: strings("gitviber.worktreeColors"),
    }
}

/// On the first launch, what the old app kept, unless this build has a file of its own.
pub(crate) fn import_legacy(cx: &mut App) {
    let saved = from_legacy(gitviber_ui::legacy::value);
    if saved != Saved::default() && !path().exists() {
        update(cx, |s| *s = saved);
    }
}

pub fn pinned(main: &str, cx: &mut App) -> Vec<String> {
    get(cx)
        .pinned_branches
        .get(main)
        .cloned()
        .unwrap_or_default()
}

pub fn set_pinned(main: &str, names: Vec<String>, cx: &mut App) {
    let main = main.to_string();
    update(cx, |s| {
        if names.is_empty() {
            s.pinned_branches.remove(&main);
        } else {
            s.pinned_branches.insert(main, names);
        }
    });
}

/// A branch renamed in the app keeps its pin.
pub fn rename_pinned(main: &str, from: &str, to: &str, cx: &mut App) {
    let pins = pinned(main, cx);
    if pins.iter().any(|p| p == from) {
        set_pinned(
            main,
            pins.into_iter()
                .map(|p| if p == from { to.to_string() } else { p })
                .collect(),
            cx,
        );
    }
}

pub fn worktree_dir(main: &str, cx: &mut App) -> Option<String> {
    get(cx).worktree_dirs.get(main).cloned()
}

pub fn set_worktree_dir(main: &str, dir: Option<String>, cx: &mut App) {
    let main = main.to_string();
    update(cx, |s| match dir {
        Some(d) => {
            s.worktree_dirs.insert(main, d);
        }
        None => {
            s.worktree_dirs.remove(&main);
        }
    });
}

pub fn worktree_run(main: &str, cx: &mut App) -> String {
    get(cx).worktree_run.get(main).cloned().unwrap_or_default()
}

/// An issue's worktree runs what the last one did, else what a plain one runs.
pub fn issue_run(main: &str, cx: &mut App) -> String {
    let s = get(cx);
    s.issue_run
        .get(main)
        .or_else(|| s.worktree_run.get(main))
        .cloned()
        .unwrap_or_default()
}

pub fn set_issue_run(main: &str, run: &str, cx: &mut App) {
    let (main, run) = (main.to_string(), run.to_string());
    update(cx, |s| {
        if run.is_empty() {
            s.issue_run.remove(&main);
        } else {
            s.issue_run.insert(main, run);
        }
    });
}

pub fn set_worktree_run(main: &str, run: &str, cx: &mut App) {
    let (main, run) = (main.to_string(), run.to_string());
    update(cx, |s| {
        if run.is_empty() {
            s.worktree_run.remove(&main);
        } else {
            s.worktree_run.insert(main, run);
        }
    });
}

pub fn colors(cx: &mut App) -> BTreeMap<String, String> {
    get(cx).worktree_colors.clone()
}

pub fn set_color(path: &str, choice: Option<&str>, cx: &mut App) {
    let path = path.to_string();
    let choice = choice.map(str::to_string);
    update(cx, |s| match choice {
        Some(c) => {
            s.worktree_colors.insert(path, c);
        }
        None => {
            s.worktree_colors.remove(&path);
        }
    });
}

/// A worktree's folder moved: what was kept under its old path follows it.
pub fn move_root(from: &str, to: &str, cx: &mut App) {
    let (from, to) = (from.to_string(), to.to_string());
    update(cx, |s| {
        if let Some(c) = s.worktree_colors.remove(&from) {
            s.worktree_colors.insert(to, c);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_apps_records_come_over() {
        let old = serde_json::json!({
            "gitviber.pinnedBranches": {"/p/a": ["main", 3, "dev"], "/p/b": []},
            "gitviber.worktreeDirs": {"/p/a": "/w", "/p/b": null},
            "gitviber.worktreeRun": {"/p/a": "claude"},
            "gitviber.worktreeColors": {"/p/a.worktrees/x": "teal", "/p/a": "none"},
            "gitviber.worktreeRun.broken": 3,
        });
        let saved = from_legacy(|key| old.get(key).cloned());
        assert_eq!(saved.pinned_branches["/p/a"], ["main", "dev"]);
        assert!(!saved.pinned_branches.contains_key("/p/b"));
        assert_eq!(saved.worktree_dirs.len(), 1);
        assert_eq!(saved.worktree_dirs["/p/a"], "/w");
        assert_eq!(saved.worktree_run["/p/a"], "claude");
        assert_eq!(saved.worktree_colors["/p/a.worktrees/x"], "teal");
        assert_eq!(saved.worktree_colors["/p/a"], "none");
        assert_eq!(from_legacy(|_| None), Saved::default());
    }
}
