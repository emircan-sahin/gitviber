//! What History remembers: whether it lists every branch (one choice for all repos), and which
//! refs the all-branches graph walks, per repository (its main worktree: worktrees share their
//! branches). history.json beside settings.json; an unreadable file starts fresh.

use std::collections::BTreeMap;

use gitviber_ui::store::{config_dir, write_in_background};
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

use crate::commit::GraphRefs;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Saved {
    pub all_branches: bool,
    /// By main worktree; left out while it's every ref.
    pub graph_refs: BTreeMap<String, GraphRefs>,
}

impl Global for Saved {}

fn path() -> std::path::PathBuf {
    config_dir().join("history.json")
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

pub fn graph_refs(main: &str, cx: &mut App) -> GraphRefs {
    get(cx).graph_refs.get(main).cloned().unwrap_or_default()
}

pub fn set_graph_refs(main: &str, refs: GraphRefs, cx: &mut App) {
    let main = main.to_string();
    update(cx, |s| {
        if refs == GraphRefs::default() {
            s.graph_refs.remove(&main);
        } else {
            s.graph_refs.insert(main, refs);
        }
    });
}
