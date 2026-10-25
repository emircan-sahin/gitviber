//! The git commands this crate owns, from anywhere in a window: keys, the palette, the menu bar.
//! Global listeners, as no element of ours is an ancestor of every panel; each acts on the
//! project of the window in front, once the dispatch is over (the window is busy during it).

use gitviber_ui::Workspace;
use gitviber_ui::keymap::{git, repo};
use gpui_kit::{App, Entity, Window};

use crate::ops;
use crate::repo::RepoState;
use crate::requests::{self, Picker};

fn in_front(cx: &mut App, f: impl FnOnce(&Entity<RepoState>, &mut Window, &mut App) + 'static) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            let Some(project) =
                Workspace::for_window(window, cx).and_then(|ws| ws.read(cx).project().cloned())
            else {
                return;
            };
            let state = RepoState::get(&project, cx);
            f(&state, window, cx);
        });
    });
}

fn idle(state: &Entity<RepoState>, cx: &App) -> bool {
    state.read(cx).busy().is_none()
}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &git::Fetch, cx| {
        in_front(cx, |s, w, cx| {
            if idle(s, cx) {
                ops::fetch(s, w, cx)
            }
        })
    });
    cx.on_action(|_: &git::Pull, cx| {
        in_front(cx, |s, w, cx| {
            let pullable = s.read(cx).status.as_ref().is_some_and(|st| {
                (st.upstream.is_some() && !st.upstream_gone) || st.follows.is_some()
            });
            if idle(s, cx) && pullable {
                ops::pull(s, gitviber_core::git::PullMode::Ff, false, w, cx)
            }
        })
    });
    // With no upstream yet, pushing is publishing, where Publish would without asking.
    cx.on_action(|_: &git::Push, cx| {
        in_front(cx, |s, w, cx| {
            let Some(st) = s.read(cx).status.clone() else {
                return;
            };
            if !idle(s, cx) || st.follows.is_some() {
                return;
            }
            if st.upstream.is_some() && !st.upstream_gone {
                return ops::push(s, false, false, w, cx);
            }
            let to = st
                .publish
                .clone()
                .or_else(|| (st.remotes.len() == 1).then(|| st.remotes[0].clone()));
            if let Some(remote) = to.filter(|_| st.branch.is_some() && st.head.is_some()) {
                ops::publish(s, remote, w, cx)
            }
        })
    });
    cx.on_action(|_: &git::Sync, cx| {
        in_front(cx, |s, w, cx| {
            let tracked = s
                .read(cx)
                .status
                .as_ref()
                .is_some_and(|st| st.upstream.is_some() && !st.upstream_gone);
            if idle(s, cx) && tracked {
                ops::sync(s, false, w, cx)
            }
        })
    });
    cx.on_action(|_: &git::Undo, cx| in_front(cx, |s, w, cx| ops::step(s, false, w, cx)));
    cx.on_action(|_: &git::Redo, cx| in_front(cx, |s, w, cx| ops::step(s, true, w, cx)));
    // The app's window handles Refresh itself; this one is for a window without that handler.
    cx.on_action(|_: &repo::Refresh, cx| {
        in_front(cx, |s, _, cx| {
            if let Some(p) = s.read(cx).project() {
                p.update(cx, |p, cx| p.repo_changed(cx));
            }
        })
    });
    cx.on_action(|_: &git::SwitchBranch, cx| {
        if let Some(h) = cx.active_window() {
            requests::request(Picker::Branches, h, cx);
        }
    });
    cx.on_action(|_: &git::SwitchWorktree, cx| {
        if let Some(h) = cx.active_window() {
            requests::request(Picker::Worktrees, h, cx);
        }
    });
    cx.on_action(|_: &git::NewBranch, cx| {
        in_front(cx, |s, w, cx| {
            let base = s
                .read(cx)
                .branch()
                .map_or("HEAD".to_string(), |b| format!("refs/heads/{b}"));
            crate::branches::open_new_branch(base, w, cx)
        })
    });
    cx.on_action(|_: &git::NewWorktree, cx| {
        in_front(cx, |_, w, cx| {
            crate::worktrees::open_new_worktree(None, w, cx)
        })
    });
}
