//! The git commands this crate owns, from anywhere in a window: keys, the palette, the menu bar.
//! Global listeners, as no element of ours is an ancestor of every panel; each acts on the
//! project of the window in front, once the dispatch is over (the window is busy during it).

use gitviber_core::git::RepoStatus;
use gitviber_ui::Workspace;
use gitviber_ui::keymap::{git, repo};
use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::{App, Entity, ParentElement as _, Styled as _, Window, div, px};

use crate::ops;
use crate::repo::RepoState;
use crate::requests::{self, Picker};
use crate::toast::{Action, Note};

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
            if project.read(cx).is_folder() {
                crate::folder::not_a_repo(&project, window, cx);
                return;
            }
            let state = RepoState::get(&project, cx);
            f(&state, window, cx);
        });
    });
}

/// A network command of the top bar's, from its button or its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Fetch,
    Pull,
    /// Pushes, and publishes a branch with no upstream yet.
    Push,
    Sync,
    Publish,
}

impl Op {
    fn verb(self) -> &'static str {
        match self {
            Op::Fetch => "Fetch",
            Op::Pull => "Pull",
            Op::Push => "Push",
            Op::Sync => "Sync",
            Op::Publish => "Publish",
        }
    }
}

/// The one click that removes what stops a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unblock {
    Publish,
    NewBranch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WhyNot {
    pub reason: String,
    pub fix: Option<Unblock>,
}

fn why(reason: impl Into<String>, fix: Option<Unblock>) -> Option<WhyNot> {
    Some(WhyNot {
        reason: reason.into(),
        fix,
    })
}

pub(crate) fn tracked(st: &RepoStatus) -> bool {
    st.upstream.is_some() && !st.upstream_gone
}

/// What stops `op` in this state, if anything: the top bar's disabled tooltips and the notes
/// its keys show both say this, so the two can't disagree.
pub(crate) fn why_not(op: Op, st: &RepoStatus) -> Option<WhyNot> {
    let no_remote = || {
        why(
            "No remote: add one with `git remote add origin <url>`.",
            None,
        )
    };
    if op == Op::Fetch {
        return if st.remotes.is_empty() {
            no_remote()
        } else {
            None
        };
    }
    // A fork's pull request checked out pulls from its refs/pull/<n>/head.
    if let Some(f) = &st.follows {
        return (op != Op::Pull).then(|| WhyNot {
            reason: format!("Pull request {f}'s branch is on its author's fork; push there."),
            fix: None,
        });
    }
    // An upstream can be a local branch, with no remote at all.
    if tracked(st) && op != Op::Publish {
        return None;
    }
    if st.remotes.is_empty() {
        return no_remote();
    }
    let pulls = matches!(op, Op::Pull | Op::Sync);
    let Some(branch) = &st.branch else {
        return if pulls {
            why(
                "HEAD is detached: switch to a branch to pull into it.",
                None,
            )
        } else {
            why(
                "HEAD is detached: create a branch to push these commits.",
                Some(Unblock::NewBranch),
            )
        };
    };
    if st.head.is_none() {
        return why(
            format!("{branch} has no commits yet: make a first commit to publish it."),
            None,
        );
    }
    if !pulls {
        return None;
    }
    match (&st.upstream, st.upstream_gone) {
        (Some(up), true) => why(
            format!("{up} isn't on the remote (deleted, or never pushed)."),
            None,
        ),
        _ => why(
            format!("{branch} has no upstream yet: publish it first."),
            Some(Unblock::Publish),
        ),
    }
}

/// Runs `go` unless something stops `op`; else says what, with the click that fixes it.
fn unless_blocked(
    s: &Entity<RepoState>,
    op: Op,
    window: &mut Window,
    cx: &mut App,
    go: impl FnOnce(&Entity<RepoState>, &RepoStatus, &mut Window, &mut App),
) {
    let state = s.read(cx);
    let blocked = match (state.busy().cloned(), state.status.clone()) {
        (Some(b), _) => WhyNot {
            reason: format!("Wait for {b} to finish."),
            fix: None,
        },
        (None, None) => WhyNot {
            reason: "The repository is still being read.".into(),
            fix: None,
        },
        (None, Some(st)) => match why_not(op, &st) {
            None => return go(s, &st, window, cx),
            Some(blocked) => blocked,
        },
    };
    let fix = blocked.fix.map(|fix| match fix {
        Unblock::Publish => {
            let weak = s.downgrade();
            Action::new("Publish", move |window, cx| {
                if let Some(s) = weak.upgrade()
                    && let Some(st) = s.read(cx).status.clone()
                {
                    publish(&s, &st, window, cx)
                }
            })
        }
        Unblock::NewBranch => Action::new("New branch…", |window, cx| {
            window.dispatch_action(Box::new(git::NewBranch), cx)
        }),
    });
    let note = Note::not_run(op.verb(), blocked.reason).action(fix);
    s.update(cx, |s, cx| s.guarded(note, cx)).show(window, cx);
}

/// Publishes to the remote Publish would; with several and none preferred, asks which.
fn publish(s: &Entity<RepoState>, st: &RepoStatus, window: &mut Window, cx: &mut App) {
    let to = st
        .publish
        .clone()
        .or_else(|| (st.remotes.len() == 1).then(|| st.remotes[0].clone()));
    match to {
        Some(remote) => ops::publish(s, remote, window, cx),
        None => choose_remote(s, st, window, cx),
    }
}

fn choose_remote(s: &Entity<RepoState>, st: &RepoStatus, window: &mut Window, cx: &mut App) {
    let title = format!("Publish {} to", st.branch.clone().unwrap_or_default());
    let (weak, remotes) = (s.downgrade(), st.remotes.clone());
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, _| {
        // Opened from a key, it should take Enter: once drawn, focus moves from the dialog
        // into its first tab stop, the first remote.
        if first.replace(false) {
            window.on_next_frame(|window, cx| window.focus_next(cx));
        }
        let buttons = remotes.iter().enumerate().map(|(i, r)| {
            let (weak, remote) = (weak.clone(), r.clone());
            Button::new(("publish-to", i))
                .icon(IconName::CloudUpload)
                .label(r.clone())
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    if let Some(s) = weak.upgrade() {
                        ops::publish(&s, remote.clone(), window, cx)
                    }
                })
        });
        dialog
            .title(title.clone())
            .w(px(320.))
            .child(div().flex().flex_col().gap_2().children(buttons))
    });
}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &git::Fetch, cx| {
        in_front(cx, |s, w, cx| {
            unless_blocked(s, Op::Fetch, w, cx, |s, _, w, cx| ops::fetch(s, w, cx))
        })
    });
    cx.on_action(|_: &git::Pull, cx| {
        in_front(cx, |s, w, cx| {
            unless_blocked(s, Op::Pull, w, cx, |s, _, w, cx| {
                ops::pull(s, gitviber_core::git::PullMode::Ff, false, w, cx)
            })
        })
    });
    // With no upstream yet, pushing is publishing, where Publish would without asking.
    cx.on_action(|_: &git::Push, cx| {
        in_front(cx, |s, w, cx| {
            unless_blocked(s, Op::Push, w, cx, |s, st, w, cx| {
                if tracked(st) {
                    ops::push(s, false, false, w, cx)
                } else {
                    publish(s, st, w, cx)
                }
            })
        })
    });
    cx.on_action(|_: &git::Sync, cx| {
        in_front(cx, |s, w, cx| {
            unless_blocked(s, Op::Sync, w, cx, |s, _, w, cx| ops::sync(s, false, w, cx))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn status(branch: Option<&str>, upstream: Option<&str>, remotes: &[&str]) -> RepoStatus {
        RepoStatus {
            branch: branch.map(Into::into),
            head: Some("1a2b3c4".into()),
            upstream: upstream.map(Into::into),
            remotes: remotes.iter().map(|r| r.to_string()).collect(),
            ..Default::default()
        }
    }

    fn reason(op: Op, st: &RepoStatus) -> Option<(String, Option<Unblock>)> {
        why_not(op, st).map(|w| (w.reason, w.fix))
    }

    #[test]
    fn a_tracked_branch_runs_everything() {
        let st = status(Some("main"), Some("origin/main"), &["origin"]);
        for op in [Op::Fetch, Op::Pull, Op::Push, Op::Sync] {
            assert_eq!(why_not(op, &st), None, "{op:?}");
        }
    }

    #[test]
    fn no_remote_says_how_to_add_one_for_every_command() {
        let st = status(Some("main"), None, &[]);
        for op in [Op::Fetch, Op::Pull, Op::Push, Op::Sync, Op::Publish] {
            assert_eq!(
                reason(op, &st),
                Some((
                    "No remote: add one with `git remote add origin <url>`.".into(),
                    None
                )),
                "{op:?}"
            );
        }
    }

    #[test]
    fn a_local_upstream_pulls_with_no_remote() {
        let st = status(Some("feat"), Some("main"), &[]);
        assert_eq!(why_not(Op::Pull, &st), None);
        assert!(why_not(Op::Fetch, &st).is_some());
    }

    #[test]
    fn an_unpublished_branch_pulls_after_publishing_and_pushes_by_publishing() {
        let st = status(Some("feat"), None, &["origin", "upstream"]);
        let first = Some((
            "feat has no upstream yet: publish it first.".to_string(),
            Some(Unblock::Publish),
        ));
        assert_eq!(reason(Op::Pull, &st), first);
        assert_eq!(reason(Op::Sync, &st), first);
        assert_eq!(why_not(Op::Push, &st), None);
        assert_eq!(why_not(Op::Publish, &st), None);
        assert_eq!(why_not(Op::Fetch, &st), None);
    }

    #[test]
    fn a_deleted_upstream_is_named_with_nothing_to_click() {
        let mut st = status(Some("feat"), Some("origin/feat"), &["origin"]);
        st.upstream_gone = true;
        assert_eq!(
            reason(Op::Pull, &st),
            Some((
                "origin/feat isn't on the remote (deleted, or never pushed).".into(),
                None
            ))
        );
        // Push publishes it again.
        assert_eq!(why_not(Op::Push, &st), None);
    }

    #[test]
    fn a_detached_head_offers_a_branch_to_push_from() {
        let st = status(None, None, &["origin"]);
        assert_eq!(
            reason(Op::Push, &st).and_then(|r| r.1),
            Some(Unblock::NewBranch)
        );
        assert_eq!(
            reason(Op::Publish, &st).and_then(|r| r.1),
            Some(Unblock::NewBranch)
        );
        assert_eq!(reason(Op::Pull, &st).and_then(|r| r.1), None);
    }

    #[test]
    fn an_empty_branch_wants_a_first_commit() {
        let mut st = status(Some("main"), None, &["origin"]);
        st.head = None;
        assert_eq!(
            reason(Op::Publish, &st),
            Some((
                "main has no commits yet: make a first commit to publish it.".into(),
                None
            ))
        );
    }

    #[test]
    fn a_pull_request_from_a_fork_pulls_but_doesnt_push() {
        let mut st = status(Some("pr-7"), None, &["origin"]);
        st.follows = Some("#7".into());
        assert_eq!(why_not(Op::Pull, &st), None);
        assert_eq!(
            reason(Op::Push, &st).map(|r| r.0),
            Some("Pull request #7's branch is on its author's fork; push there.".into())
        );
    }
}
