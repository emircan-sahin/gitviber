//! GitViber's guided review: the user's own agent CLI explains a commit, a branch, a pull
//! request or the uncommitted changes as sections in reading order beside their diffs, finds the
//! change's risks, and hands any part of it to Claude Code in the worktree's terminal.
//!
//! [`init`] registers the `guide` tabs' opener; [`open_guide`] is how History's Explain Commit,
//! the Changes summary's ✦ and a pull request's menu open one and ask for its guide.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

pub mod agent;
pub mod model;
pub mod note_store;
pub mod store;
mod view;

#[cfg(test)]
mod tests;

use std::collections::HashSet;

use gitviber_ui::workspace::{
    GuideOf, Selection, TabHandle, Workspace, register_opener, selection_key,
};
use gpui_kit::{App, AppContext as _, Global, Window};

pub use note_store::ReviewNotes;
pub use store::{Guides, RunKind, SavedRun};
pub use view::GuideView;

/// Guide tabs asked to write their guide once they know what's kept: a click is the ask, a tab
/// restored from a saved workspace isn't.
#[derive(Default)]
struct Requested(HashSet<String>);

impl Global for Requested {}

pub(crate) fn take_request(cx: &mut App, key: &str) -> bool {
    cx.try_global::<Requested>()
        .is_some_and(|r| r.0.contains(key))
        && cx.default_global::<Requested>().0.remove(key)
}

/// The `guide` tabs' opener, and on the first launch the old app's guides and review notes.
/// Once at startup.
pub fn init(cx: &mut App) {
    store::import_legacy(cx);
    note_store::import_legacy(cx);
    register_opener(cx, "guide", |sel, window, cx| match sel {
        Selection::Guide { of } => {
            let of = of.clone();
            Some(TabHandle::new(cx.new(|cx| GuideView::new(of, window, cx))))
        }
        _ => None,
    });
}

/// Opens `of`'s guide tab, asking the Guided Review agent for its guide when there's none yet
/// and none being written.
pub fn open_guide(of: GuideOf, window: &mut Window, cx: &mut App) {
    let sel = Selection::Guide { of };
    let key = selection_key(&sel);
    let Some(ws) = Workspace::for_window(window, cx) else {
        return;
    };
    cx.default_global::<Requested>().0.insert(key.clone());
    let opened = ws.update(cx, |ws, cx| ws.open(sel.clone(), false, window, cx));
    if !opened {
        cx.default_global::<Requested>().0.remove(&key);
        return;
    }
    // A tab open already has its guide's id: it takes the ask now.
    let view = ws
        .read(cx)
        .tab_for(&sel)
        .and_then(|t| t.item.handle())
        .and_then(|h| h.view().downcast::<GuideView>().ok());
    if let Some(view) = view {
        view.update(cx, |v, cx| v.take_ask(cx));
    }
}
