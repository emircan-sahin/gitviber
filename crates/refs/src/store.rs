//! What the pickers remember per project: pinned branches, where new worktrees go and what runs
//! in their terminal, and the colors picked for worktrees. refs.json beside settings.json; an
//! unreadable file starts fresh.

use std::collections::BTreeMap;

use gitviber_ui::store::{config_dir, write_in_background};
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Saved {
    /// By main worktree, in pin order.
    pub pinned_branches: BTreeMap<String, Vec<String>>,
    /// By main worktree: the folder its new worktrees go in, when not the default beside it.
    pub worktree_dirs: BTreeMap<String, String>,
    /// By main worktree: the command its last new worktree's terminal ran.
    pub worktree_run: BTreeMap<String, String>,
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
