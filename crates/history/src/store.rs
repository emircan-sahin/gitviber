//! What History remembers: whether it lists every branch (one choice for all repos), and which
//! refs the all-branches graph walks, per repository (its main worktree: worktrees share their
//! branches). history.json beside settings.json; an unreadable file starts fresh.

use std::collections::BTreeMap;

use gitviber_ui::store::{config_dir, write_in_background};
use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commit::GraphRefs;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Saved {
    pub all_branches: bool,
    /// By main worktree; left out while it's every ref.
    pub graph_refs: BTreeMap<String, GraphRefs>,
    /// A fork's panes closed: "origin" (your branch), "parent" (the original).
    pub closed_panes: Vec<String>,
    /// Your branch's share of the height the two open fork panes split; left out while even.
    pub fork_split: Option<f32>,
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

/// The old app's (its first launch here): the one all-branches choice, and each repository's
/// refs, a field it left out read as every ref.
fn from_legacy(value: impl Fn(&str) -> Option<Value>) -> Saved {
    let graph_refs = match value("gitviber.history.graphRefs") {
        Some(Value::Object(map)) => map
            .into_iter()
            .filter_map(|(main, v)| Some((main, serde_json::from_value::<GraphRefs>(v).ok()?)))
            .filter(|(_, refs)| *refs != GraphRefs::default())
            .collect(),
        _ => BTreeMap::new(),
    };
    let closed_panes = match value("gitviber.history-panes.collapsed") {
        Some(Value::Array(ids)) => ids
            .into_iter()
            .filter_map(|id| id.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    };
    // The resizable panes kept each one's percentage, under their group's id and theirs.
    let fork_split = value("react-resizable-panels:gitviber-history-panes-v2:origin:parent")
        .and_then(|sizes| {
            let (ours, theirs) = (
                sizes.get("origin")?.as_f64()?,
                sizes.get("parent")?.as_f64()?,
            );
            (ours > 0. && theirs > 0.).then(|| (ours / (ours + theirs)) as f32)
        });
    Saved {
        all_branches: value("gitviber.history.allBranches") == Some(Value::Bool(true)),
        graph_refs,
        closed_panes,
        fork_split,
    }
}

/// On the first launch, what the old app kept, unless this build has a file of its own.
pub(crate) fn import_legacy(cx: &mut App) {
    let saved = from_legacy(gitviber_ui::legacy::value);
    if saved != Saved::default() && !path().exists() {
        update(cx, |s| *s = saved);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_apps_choices_come_over() {
        let old = serde_json::json!({
            "gitviber.history.allBranches": true,
            "gitviber.history-panes.collapsed": ["parent", 3],
            "react-resizable-panels:gitviber-history-panes-v2:origin:parent": {"origin": 75, "parent": 25},
            "gitviber.history.graphRefs": {
                "/p/a": {"local": true, "remote": false, "tags": true, "hidden": ["refs/heads/x"], "only": null},
                "/p/b": {"tags": false},
                "/p/c": {"local": true, "remote": true, "tags": true, "hidden": [], "only": null},
                "/p/d": {"hidden": "x"},
            },
        });
        let saved = from_legacy(|key| old.get(key).cloned());
        assert!(saved.all_branches);
        assert_eq!(saved.closed_panes, ["parent"]);
        assert_eq!(saved.fork_split, Some(0.75));
        assert!(!saved.graph_refs["/p/a"].remote);
        assert_eq!(saved.graph_refs["/p/a"].hidden, ["refs/heads/x"]);
        assert!(!saved.graph_refs["/p/b"].tags && saved.graph_refs["/p/b"].local);
        // Every ref is what nothing saved means; one that can't be read is left out.
        assert_eq!(saved.graph_refs.len(), 2);
        assert_eq!(from_legacy(|_| None), Saved::default());
    }
}
