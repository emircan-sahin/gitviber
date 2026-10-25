//! The git actions the top bar and the pickers run: fetch, pull, push and publish; switching,
//! merging, rebasing and deleting branches; removing worktrees and moving the main one back;
//! undo and redo. Each asks first where the old app did, then runs through `RepoState::run`.

use futures::channel::oneshot;
use gitviber_core::network::CANCELLED;
use gitviber_core::{Core, Res, api, git};
use gpui_kit::{App, AppContext as _, Entity, SharedString, WeakEntity, Window};

use crate::ask::{Ask, ask, ask_stacked};
use crate::errors::{GitFix, explain_git_error, secret_commits};
use crate::names::{Branch, folder_name, plural, short_ref};
use crate::net::NetOp;
use crate::remote_tags;
use crate::repo::{GitAction, RepoState, plain};
use crate::toast::{Action, Fixes, Note, git_failed};
use crate::worktrees::{
    Worktree, WorktreeEvent, WorktreeEvents, default_branch, folder_for_branch,
};

/// A force-push question waits behind this, in the error a refused push comes back with.
const ASK_FORCE: &str = "gitviber:ask-force\n";

fn run(
    state: &Entity<RepoState>,
    action: GitAction,
    work: impl FnOnce(&Core, &NetOp) -> Res<bool> + Send + 'static,
    window: &mut Window,
    cx: &mut App,
) -> oneshot::Receiver<bool> {
    state.update(cx, |s, cx| s.run(action, work, window, cx))
}

/// What runs after an action went through.
pub type Then = Box<dyn FnOnce(&mut Window, &mut App)>;

/// `run` for an action whose end nobody waits on.
fn go(
    state: &Entity<RepoState>,
    action: GitAction,
    work: impl FnOnce(&Core, &NetOp) -> Res<bool> + Send + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    drop(run(state, action, work, window, cx));
}

/// Runs `then` in the window once the question is answered yes.
pub fn after_yes(
    rx: oneshot::Receiver<bool>,
    window: &Window,
    cx: &mut App,
    then: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        if rx.await == Ok(true) {
            let _ = handle.update(cx, |_, window, cx| then(window, cx));
        }
    })
    .detach();
}

/// `state`'s action again, if it's still there.
fn with(
    state: &WeakEntity<RepoState>,
    window: &mut Window,
    cx: &mut App,
    f: impl FnOnce(&Entity<RepoState>, &mut Window, &mut App),
) {
    if let Some(state) = state.upgrade() {
        f(&state, window, cx);
    }
}

/// Autostashed changes wait out a stopped merge or rebase, and git keeps them in the stash as
/// well when they conflict coming back.
fn stashed_for(autostash: bool, what: &str) -> Option<String> {
    autostash.then(|| format!(
        "Resolve them in Changes. Your uncommitted changes were set aside for the {what} and come back when it finishes (Continue or Abort, if it's waiting on you). If they conflict coming back, they're kept in Stashes too: drop that stash once resolved."
    ))
}

fn with_conflicts(action: GitAction, text: Option<String>) -> GitAction {
    match text {
        Some(t) => action.conflicts(t),
        None => action,
    }
}

pub fn fetch(state: &Entity<RepoState>, window: &mut Window, cx: &mut App) {
    let action = GitAction::new("Fetch").net().untracked();
    go(
        state,
        action,
        |core, op| plain(api::sync::fetch(core, op.id.clone(), op.progress())),
        window,
        cx,
    );
}

fn pull_label(mode: git::PullMode) -> &'static str {
    match mode {
        git::PullMode::Ff => "Pulled",
        git::PullMode::Merge => "Pulled (merge)",
        git::PullMode::Rebase => "Pulled (rebase)",
    }
}

/// What a diverged pull offers: the same pull with a merge or a rebase.
fn pulls(state: &Entity<RepoState>) -> Action {
    let weak = state.downgrade();
    Action::new("Pull (merge)", move |window, cx| {
        with(&weak, window, cx, |s, w, cx| {
            pull(s, git::PullMode::Merge, false, w, cx)
        })
    })
}

fn retry(
    state: &Entity<RepoState>,
    again: impl Fn(&Entity<RepoState>, &mut Window, &mut App) + 'static,
) -> Action {
    let weak = state.downgrade();
    Action::new("Retry with autostash", move |window, cx| {
        with(&weak, window, cx, |s, w, cx| again(s, w, cx))
    })
}

pub fn pull(
    state: &Entity<RepoState>,
    mode: git::PullMode,
    autostash: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let action = with_conflicts(
        GitAction::new("Pull").net().done(pull_label(mode)),
        stashed_for(autostash, "pull"),
    )
    .fix(GitFix::Diverged, pulls(state))
    .fix(
        GitFix::Autostash,
        retry(state, move |s, w, cx| pull(s, mode, true, w, cx)),
    );
    go(
        state,
        action,
        move |core, op| api::sync::pull(core, mode, Some(autostash), op.id.clone(), op.progress()),
        window,
        cx,
    );
}

/// Pull, then push.
pub fn sync(state: &Entity<RepoState>, autostash: bool, window: &mut Window, cx: &mut App) {
    let behind = pushes_upstream(state, cx).then(|| pulls(state));
    let weak = state.downgrade();
    let mut action = with_conflicts(
        GitAction::new("Sync")
            .net()
            .done("Synced")
            .on_error(move |e, window, cx| secret_refused(&weak, "Sync", e, window, cx)),
        stashed_for(autostash, "pull"),
    )
    .fix(GitFix::Diverged, pulls(state))
    .fix(
        GitFix::Autostash,
        retry(state, |s, w, cx| sync(s, true, w, cx)),
    );
    if let Some(b) = behind {
        action = action.fix(GitFix::FetchFirst, b);
    }
    go(
        state,
        action,
        move |core, op| {
            if api::sync::pull(
                core,
                git::PullMode::Ff,
                Some(autostash),
                op.id.clone(),
                op.progress(),
            )? {
                return Ok(true);
            }
            // The push goes from the worktree the pull ran in, not one opened since.
            op.still_in(core)?;
            plain(api::sync::push(
                core,
                Some(false),
                None,
                None,
                op.id.clone(),
                op.progress(),
            ))
        },
        window,
        cx,
    );
}

/// A pull brings in the upstream, which can't help a push that goes elsewhere (a fork pulling
/// upstream/dev and pushing origin/dev).
fn pushes_upstream(state: &Entity<RepoState>, cx: &App) -> bool {
    let Some(s) = state.read(cx).status.clone() else {
        return true;
    };
    match s.push.as_ref().and_then(|p| p.branch.as_deref()) {
        None => true,
        Some(b) => Some(b) == s.upstream.as_deref(),
    }
}

/// Rejected as non-fast-forward with the remote's extra commits this branch's own from before a
/// rebase or amend: replacing them is a force push, asked first. Anyone else's want a pull,
/// which the error toast offers.
pub fn push(state: &Entity<RepoState>, tags: bool, force: bool, window: &mut Window, cx: &mut App) {
    let weak = state.downgrade();
    let mut action = GitAction::new("Push").net().untracked().done(if tags { "Pushed with tags" } else { "Pushed" }).on_error(move |e, window, cx| {
        if !e.starts_with(ASK_FORCE) {
            return secret_refused(&weak, "Push", e, window, cx);
        }
        let rx = ask(
            Ask::new(
                "Force push",
                "The remote branch has commits yours no longer has, as after a rebase or an amend. Replace them with yours?\n\nThis force-pushes (with lease): it is refused if someone pushed commits there that your branch never had. Anyone who pulled the old commits will have to reconcile.",
                "Force push",
            )
            .danger(),
            window,
            cx,
        );
        let weak = weak.clone();
        after_yes(rx, window, cx, move |w, cx| with(&weak, w, cx, |s, w, cx| push(s, tags, true, w, cx)));
        true
    });
    if pushes_upstream(state, cx) {
        action = action.fix(GitFix::FetchFirst, pulls(state));
    }
    // With tags or followTags, what the remote has of them may change.
    let done = run(
        state,
        action,
        move |core, op| match api::sync::push(
            core,
            Some(force),
            None,
            Some(tags),
            op.id.clone(),
            op.progress(),
        ) {
            Ok(()) => Ok(false),
            Err(e)
                if !force
                    && e.contains("non-fast-forward")
                    && api::sync::remote_was_ours(core).unwrap_or(false) =>
            {
                Err(format!("{ASK_FORCE}{e}"))
            }
            Err(e) => Err(e),
        },
        window,
        cx,
    );
    remote_tags::forget_after(done, cx);
}

/// Pushes the branch to `remote` and tracks it there.
pub fn publish(state: &Entity<RepoState>, remote: String, window: &mut Window, cx: &mut App) {
    let weak = state.downgrade();
    let action = GitAction::new("Publish")
        .net()
        .untracked()
        .done(format!("Branch published to {remote}"))
        .on_error(move |e, window, cx| secret_refused(&weak, "Publish", e, window, cx));
    go(
        state,
        action,
        move |core, op| {
            plain(api::sync::push(
                core,
                Some(false),
                Some(remote),
                None,
                op.id.clone(),
                op.progress(),
            ))
        },
        window,
        cx,
    );
}

/// A push GitHub refused over a secret: when only the last commit has it, the error offers Undo
/// last commit, which brings the file back staged to take the secret out. True: the error is
/// shown here, once that's known.
fn secret_refused(
    state: &WeakEntity<RepoState>,
    label: &str,
    e: &str,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let listed = secret_commits(e);
    if listed.is_empty() || explain_git_error(e).and_then(|h| h.fix) != Some(GitFix::Secret) {
        return false;
    }
    let core = gitviber_ui::core(cx);
    let check = cx.background_spawn(async move { secret_only_in_head(&core, &listed) });
    let (state, handle) = (state.clone(), window.window_handle());
    let (title, e) = (format!("{label} failed"), e.to_string());
    cx.spawn(async move |cx| {
        let head = check.await;
        cx.update(|cx| {
            let fixes: Fixes = head
                .map(|sha| (GitFix::Secret, undo_last(&state, sha)))
                .into_iter()
                .collect();
            let _ = state.update(cx, |s, cx| {
                s.guarded(git_failed(title, &e, &fixes), cx)
                    .show_in(handle, cx)
            });
        });
    })
    .detach();
    true
}

/// HEAD, when it's the only commit GitHub listed a secret in and undoing it is plain: one parent,
/// and not on the push target yet (History's Undo commit would warn about either).
pub(crate) fn secret_only_in_head(core: &Core, listed: &[String]) -> Option<String> {
    let head = core
        .in_repo(|r| git::run(r, &["rev-parse", "--verify", "-q", "HEAD"]))
        .ok()?;
    let head = String::from_utf8_lossy(&head).trim().to_string();
    if listed.is_empty() || !listed.iter().all(|s| head.starts_with(s.as_str())) {
        return None;
    }
    let commit = api::history::find_commit(core, head.clone()).ok()??;
    let [parent] = commit.parents.as_slice() else {
        return None;
    };
    let pushed = api::history::drops_pushed(core, Some(parent.clone())).ok()?;
    (!pushed).then_some(head)
}

/// History's Undo commit for `sha`, from a refused push's error; core refuses it if HEAD moved.
fn undo_last(state: &WeakEntity<RepoState>, sha: String) -> Action {
    let state = state.clone();
    Action::new("Undo last commit", move |window, cx| {
        let sha = sha.clone();
        with(&state, window, cx, |s, w, cx| {
            let action = GitAction::new("Undo").done("Commit undone; its changes are staged");
            go(
                s,
                action,
                move |core, _| plain(api::history::undo_commit(core, sha)),
                w,
                cx,
            );
        });
    })
}

pub fn push_tags(
    state: &Entity<RepoState>,
    names: Vec<String>,
    remote: String,
    window: &mut Window,
    cx: &mut App,
) {
    let what = if names.len() == 1 {
        names[0].clone()
    } else {
        format!("{} tags", names.len())
    };
    let action = GitAction::new("Push tags")
        .net()
        .untracked()
        .done(format!("Pushed {what} to {remote}"));
    let done = run(
        state,
        action,
        move |core, op| {
            plain(api::branches::push_tags(
                core,
                names,
                op.id.clone(),
                op.progress(),
            ))
        },
        window,
        cx,
    );
    remote_tags::forget_after(done, cx);
}

/// Merges `name` into HEAD's branch. A squash's changes would get the stash back before they're
/// committed: no autostash for it.
pub fn merge(
    state: &Entity<RepoState>,
    name: String,
    how: git::MergeKind,
    autostash: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let short = short_ref(&name).to_string();
    let nothing = format!("Nothing to merge: {short}'s changes are already here");
    let action = if how == git::MergeKind::Squash {
        GitAction::new("Squash merge")
            .done(format!("Squashed {short} into one commit"))
            .nothing(nothing)
    } else {
        let again = name.clone();
        with_conflicts(
            GitAction::new("Merge")
                .done(format!("Merged {short}"))
                .nothing(nothing),
            stashed_for(autostash, "merge"),
        )
        .fix(
            GitFix::Autostash,
            retry(state, move |s, w, cx| {
                merge(s, again.clone(), how, true, w, cx)
            }),
        )
    };
    let stash = autostash && how != git::MergeKind::Squash;
    go(
        state,
        action,
        move |core, _| api::sync::merge(core, name, Some(how), Some(stash)),
        window,
        cx,
    );
}

/// Rebases HEAD's branch onto `onto`. `update_refs`: whether the branches on the replayed commits
/// move along; None asks when there are any.
pub fn rebase(
    state: &Entity<RepoState>,
    onto: String,
    autostash: bool,
    update_refs: Option<bool>,
    window: &mut Window,
    cx: &mut App,
) {
    if update_refs.is_none() {
        let core = gitviber_ui::core(cx);
        let target = onto.clone();
        let stacked =
            cx.background_spawn(async move { api::sync::rebase_stacked(&core, target).ok() });
        let (weak, handle) = (state.downgrade(), window.window_handle());
        cx.spawn(async move |cx| {
            let stacked = stacked.await;
            let _ = handle.update(cx, |_, window, cx| {
                match stacked.filter(|s| !s.branches.is_empty()) {
                    None => with(&weak, window, cx, |s, w, cx| {
                        go_rebase(s, onto, autostash, None, w, cx)
                    }),
                    Some(st) => {
                        let rx = ask_stacked(
                            format!("Rebase onto {onto}"),
                            "",
                            "Rebase",
                            st.branches,
                            st.update_refs,
                            window,
                            cx,
                        );
                        cx.spawn(async move |cx| {
                            if let Ok(Some(moving)) = rx.await {
                                let _ = handle.update(cx, |_, window, cx| {
                                    with(&weak, window, cx, |s, w, cx| {
                                        go_rebase(s, onto, autostash, Some(moving), w, cx)
                                    })
                                });
                            }
                        })
                        .detach();
                    }
                }
            });
        })
        .detach();
        return;
    }
    go_rebase(state, onto, autostash, update_refs, window, cx);
}

fn go_rebase(
    state: &Entity<RepoState>,
    onto: String,
    autostash: bool,
    update_refs: Option<bool>,
    window: &mut Window,
    cx: &mut App,
) {
    let again = onto.clone();
    let action = with_conflicts(
        GitAction::new("Rebase").done(format!("Rebased onto {onto}")),
        stashed_for(autostash, "rebase"),
    )
    .fix(
        GitFix::Autostash,
        retry(state, move |s, w, cx| {
            go_rebase(s, again.clone(), true, update_refs, w, cx)
        }),
    );
    go(
        state,
        action,
        move |core, _| api::sync::rebase(core, onto, Some(autostash), update_refs),
        window,
        cx,
    );
}

/// Changes that the other branch's files would overwrite: git refuses, so they are left here in
/// a stash (Stashes in Changes brings them back), once asked.
fn switching(
    state: &Entity<RepoState>,
    to: String,
    again: impl Fn(&Entity<RepoState>, &mut Window, &mut App) + 'static,
) -> impl Fn(&str, &mut Window, &mut App) -> bool + 'static {
    let weak = state.downgrade();
    let again = std::rc::Rc::new(again);
    move |e, window, cx| {
        if !e.contains("would be overwritten by checkout") {
            return false;
        }
        let here = weak
            .upgrade()
            .and_then(|s| s.read(cx).branch().map(str::to_string))
            .unwrap_or_else(|| "this commit".into());
        let rx = ask(
            Ask::new(
                "Switch branch",
                format!(
                    "Your changes to some files conflict with {to}. Stash them and switch? They stay in Stashes, to bring back on {here} or anywhere."
                ),
                "Stash and Switch",
            ),
            window,
            cx,
        );
        let (weak, again) = (weak.clone(), again.clone());
        let message = format!("Left on {here} when switching to {to}");
        after_yes(rx, window, cx, move |window, cx| {
            let core = gitviber_ui::core(cx);
            let stash = cx.background_spawn(async move {
                api::stash::stash_push(&core, message, true, None, None)
            });
            let handle = window.window_handle();
            cx.spawn(async move |cx| {
                let stashed = stash.await;
                let _ = handle.update(cx, |_, window, cx| match stashed {
                    Ok(()) => with(&weak, window, cx, |s, w, cx| again(s, w, cx)),
                    Err(e) => Note::error("Could not stash your changes")
                        .detail(e)
                        .show(window, cx),
                });
            })
            .detach();
        });
        true
    }
}

/// Names the folder that switched: a branch another worktree holds opens that worktree instead.
fn switched(state: &Entity<RepoState>, name: &str, cx: &App) -> String {
    format!(
        "Switched {} to {name}",
        folder_name(&state.read(cx).root().to_string_lossy())
    )
}

pub fn switch_branch(state: &Entity<RepoState>, name: String, window: &mut Window, cx: &mut App) {
    let again = name.clone();
    let action = GitAction::new("Switch branch")
        .done(switched(state, &name, cx))
        .on_error(switching(state, name.clone(), move |s, w, cx| {
            switch_branch(s, again.clone(), w, cx)
        }));
    go(
        state,
        action,
        move |core, _| plain(api::branches::switch_branch(core, name, false)),
        window,
        cx,
    );
}

pub fn create_branch_here(
    state: &Entity<RepoState>,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let action = GitAction::new("Create branch").done(format!("Switched to new branch {name}"));
    go(
        state,
        action,
        move |core, _| plain(api::branches::switch_branch(core, name, true)),
        window,
        cx,
    );
}

/// A branch another worktree holds, checked out here as its commit: nothing is moved.
pub fn detach_here(state: &Entity<RepoState>, b: Branch, window: &mut Window, cx: &mut App) {
    let root = folder_name(&state.read(cx).root().to_string_lossy()).to_string();
    let again = b.clone();
    let action = GitAction::new("Check out")
        .done(format!("Checked out {} in {root}, detached", b.name))
        .detail("No branch is moved: commits made here belong to no branch until you create one.")
        .on_error(switching(state, b.name.clone(), move |s, w, cx| {
            detach_here(s, again.clone(), w, cx)
        }));
    go(
        state,
        action,
        move |core, _| plain(api::history::checkout_commit(core, b.sha)),
        window,
        cx,
    );
}

/// upstream/dev → dev. A local dev that tracks something else is a different line of work; said
/// rather than switched to silently.
pub fn switch_remote(state: &Entity<RepoState>, b: Branch, window: &mut Window, cx: &mut App) {
    let name = b.short_name().to_string();
    let local = state
        .read(cx)
        .branches
        .iter()
        .find(|x| !x.remote && x.name == name)
        .cloned();
    if local.as_ref().is_some_and(|l| l.current) {
        return;
    }
    let go = {
        let (weak, name, b) = (state.downgrade(), name.clone(), b.clone());
        move |window: &mut Window, cx: &mut App| {
            with(&weak, window, cx, |s, w, cx| {
                let again = b.clone();
                let action = GitAction::new("Switch branch")
                    .done(switched(s, &name, cx))
                    .on_error(switching(s, name.clone(), move |s, w, cx| {
                        switch_remote(s, again.clone(), w, cx)
                    }));
                let remote = b.name.clone();
                go(
                    s,
                    action,
                    move |core, _| plain(api::branches::switch_tracking(core, remote)),
                    w,
                    cx,
                );
            })
        }
    };
    match local.filter(|l| l.upstream.as_deref() != Some(b.name.as_str())) {
        Some(l) => {
            let tracks = l
                .upstream
                .map_or("tracks nothing".to_string(), |u| format!("tracks {u}"));
            let rx = ask(
                Ask::new(
                    "Switch branch",
                    format!(
                        "A local {name} already exists and {tracks}, not {}. Switch to it as it is?",
                        b.name
                    ),
                    "Switch",
                ),
                window,
                cx,
            );
            after_yes(rx, window, cx, go);
        }
        None => go(window, cx),
    }
}

/// Merged is deleted outright: nothing is lost; merged upstream too, once core checks it again.
/// Anything else needs a yes, then -D. A remote branch always asks: it goes for everyone.
pub fn delete_branch(
    state: &Entity<RepoState>,
    b: Branch,
    upstream: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let weak = state.downgrade();
    if b.remote {
        let remote = b.remote_name().unwrap_or_default().to_string();
        let rx = ask(
            Ask::new("Delete remote branch", format!("Delete {} from {remote}? It goes for everyone who uses {remote}; local branches stay.", b.short_name()), "Delete").danger(),
            window,
            cx,
        );
        after_yes(rx, window, cx, move |w, cx| {
            with(&weak, w, cx, |s, w, cx| {
                let action = GitAction::new("Delete remote branch")
                    .net()
                    .untracked()
                    .done(format!("Deleted {}", b.name));
                go(
                    s,
                    action,
                    move |core, op| {
                        plain(api::branches::delete_remote_branch(
                            core,
                            b.name,
                            op.id.clone(),
                            op.progress(),
                        ))
                    },
                    w,
                    cx,
                );
            })
        });
        return;
    }
    let (name, merged) = (b.name.clone(), b.merged);
    let question = format!(
        "{name} isn't known to be merged into {}. Deleting it loses any commits that exist only on it.",
        state.read(cx).branch().unwrap_or("HEAD")
    );
    let go = move |w: &mut Window, cx: &mut App| {
        with(&weak, w, cx, |s, w, cx| {
            let action = GitAction::new("Delete branch").done(format!("Deleted {name}"));
            let name = name.clone();
            go(
                s,
                action,
                move |core, _| {
                    plain(if upstream {
                        api::branches::delete_merged(core, vec![], vec![name])
                    } else {
                        api::branches::delete_branches(core, vec![name], !merged)
                    })
                },
                w,
                cx,
            );
        })
    };
    if b.merged || upstream {
        return go(window, cx);
    }
    let rx = ask(
        Ask::new("Delete branch", question, "Delete").danger(),
        window,
        cx,
    );
    after_yes(rx, window, cx, go);
}

pub fn clean_up_branches(
    state: &Entity<RepoState>,
    merged: Vec<String>,
    upstream: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let names: Vec<String> = merged.iter().chain(&upstream).cloned().collect();
    let label = |n: &String| {
        if upstream.contains(n) {
            format!("{n} (merged upstream)")
        } else {
            n.clone()
        }
    };
    let mut shown = names
        .iter()
        .take(12)
        .map(label)
        .collect::<Vec<_>>()
        .join("\n");
    if names.len() > 12 {
        shown.push_str(&format!("\n…and {} more", names.len() - 12));
    }
    let here = state.read(cx).branch().unwrap_or("HEAD").to_string();
    let where_ = if upstream.is_empty() {
        format!("already merged into {here}")
    } else if !merged.is_empty() {
        format!("merged into {here} or upstream")
    } else {
        "squash- or rebase-merged upstream".into()
    };
    let count = format!(
        "{} {}",
        names.len(),
        if names.len() == 1 {
            "branch"
        } else {
            "branches"
        }
    );
    let rx = ask(
        Ask::new(
            "Clean up merged branches",
            format!("Delete {count} {where_}?\n\n{shown}"),
            "Delete",
        ),
        window,
        cx,
    );
    let weak = state.downgrade();
    after_yes(rx, window, cx, move |w, cx| {
        with(&weak, w, cx, |s, w, cx| {
            let action = GitAction::new("Clean up").done(format!("Deleted {count}"));
            go(
                s,
                action,
                move |core, _| plain(api::branches::delete_merged(core, merged, upstream)),
                w,
                cx,
            );
        })
    });
}

/// The main worktree of the project `state` is for: new worktrees go beside it.
pub fn main_of(state: &Entity<RepoState>, cx: &App) -> String {
    let s = state.read(cx);
    s.project()
        .and_then(|p| {
            p.read(cx)
                .main_worktree()
                .map(|m| m.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| s.root().to_string_lossy().into_owned())
}

/// Where `main`'s next worktree goes; None is `<project>.worktrees` beside it.
pub fn worktree_dir(main: &str, cx: &mut App) -> Option<String> {
    crate::store::worktree_dir(main, cx).or_else(|| {
        let root = gitviber_ui::Settings::get(cx).worktree_root.clone()?;
        Some(format!(
            "{}/{}",
            root.trim_end_matches('/'),
            folder_name(main)
        ))
    })
}

/// A terminal on another branch gets its own worktree rather than a checkout here, which would
/// pull the files out from under this window.
pub fn branch_terminal(state: &Entity<RepoState>, name: String, window: &mut Window, cx: &mut App) {
    if state.read(cx).branch() == Some(name.as_str()) {
        let path = state.read(cx).root().to_string_lossy().into_owned();
        return WorktreeEvents::emit(WorktreeEvent::OpenTerminal { path }, cx);
    }
    let main = main_of(state, cx);
    let dir = worktree_dir(&main, cx);
    let where_ = format!(
        "{}/{}",
        dir.clone()
            .unwrap_or_else(|| format!("{}.worktrees", folder_name(&main))),
        folder_for_branch(&name)
    );
    let keep = if default_branch(&state.read(cx).branches).as_deref() == Some(name.as_str()) {
        format!(
            " {name} is best kept in {}: a worktree that holds it blocks switching to it there.",
            folder_name(&main)
        )
    } else {
        String::new()
    };
    let beside = if dir.is_some() {
        ""
    } else {
        ", next to this project,"
    };
    let rx = ask(
        Ask::new(
            "Open terminal on branch",
            format!(
                "{name} isn't checked out anywhere. Create a worktree for it at {where_}{beside} and open a terminal there?{keep}"
            ),
            "Create worktree",
        ),
        window,
        cx,
    );
    let weak = state.downgrade();
    after_yes(rx, window, cx, move |w, cx| {
        with(&weak, w, cx, |s, w, cx| {
            let action = GitAction::new("Create worktree")
                .untracked()
                .done(format!("{name} checked out in {where_}"));
            let made = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
            let slot = made.clone();
            let rx = run(
                s,
                action,
                move |core, _| {
                    let path = api::worktrees::add_worktree(core, name, None, false, dir)?;
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
                    Ok(false)
                },
                w,
                cx,
            );
            after_yes(rx, w, cx, move |_, cx| {
                if let Some(path) = made.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    WorktreeEvents::emit(WorktreeEvent::OpenTerminal { path }, cx);
                }
            });
        })
    });
}

/// A fresh count decides force: git refuses a dirty or locked worktree otherwise, and the warning
/// must say what gets lost. `then`: after it went through.
pub fn remove_worktree(
    state: &Entity<RepoState>,
    w: Worktree,
    then: Option<Then>,
    window: &mut Window,
    cx: &mut App,
) {
    let name = folder_name(&w.path).to_string();
    let weak = state.downgrade();
    let terminals = crate::hooks::terminals_in(&w.path, cx);
    if w.prunable && !w.locked {
        let drive = ["/Volumes/", "/media/", "/run/media/", "/mnt/"]
            .iter()
            .any(|p| w.path.starts_with(p));
        let drive = if drive {
            " It was on another drive: if that's only unplugged, plug it in instead."
        } else {
            ""
        };
        let branch = w
            .branch
            .as_deref()
            .map(|b| format!(" {b}"))
            .unwrap_or_default();
        let rx = ask(
            Ask::new(
                "Prune worktree",
                format!(
                    "{name}'s folder is gone. Prune it from the worktree list?{drive} The branch{branch} stays."
                ),
                "Prune",
            ),
            window,
            cx,
        );
        after_yes(rx, window, cx, move |win, cx| {
            with(&weak, win, cx, |s, win, cx| {
                let path = w.path.clone();
                let rx = run(
                    s,
                    GitAction::new("Prune worktree")
                        .untracked()
                        .done(format!("Pruned {name}")),
                    move |core, _| plain(api::worktrees::remove_worktree(core, path, false)),
                    win,
                    cx,
                );
                if let Some(then) = then {
                    after_yes(rx, win, cx, then);
                }
            })
        });
        return;
    }
    let core = gitviber_ui::core(cx);
    let path = w.path.clone();
    let prunable = w.prunable;
    let count = cx.background_spawn(async move {
        if prunable {
            0
        } else {
            api::worktrees::worktree_state(&core, path, false).map_or(0, |s| s.uncommitted)
        }
    });
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let changed = count.await;
        let _ = handle.update(cx, |_, window, cx| {
            let branch = w.branch.as_deref().map(|b| format!(" The branch {b} stays.")).unwrap_or_default();
            let lost = if changed > 0 { format!(" Its {changed} uncommitted {} will be lost.", if changed == 1 { "change" } else { "changes" }) } else { String::new() };
            let reason = w.lock_reason.as_deref();
            let lock = if w.in_use {
                format!(" Something is working in it right now ({}); deleting pulls the folder out from under it.", reason.unwrap_or("it holds the lock"))
            } else if w.locked {
                format!(" It's locked{}; this overrides the lock. The lock on its row unlocks it instead.", reason.map(|r| format!(" ({r})")).unwrap_or_default())
            } else {
                String::new()
            };
            let shells = match terminals {
                0 => String::new(),
                1 => " A terminal runs in it; whatever runs there, like an agent or a dev server, loses its folder.".into(),
                n => format!(" {n} terminals run in it; whatever runs there, like an agent or a dev server, loses its folder."),
            };
            let (title, text, ok) = if w.prunable {
                let why = reason.map(|r| format!(" ({r})")).unwrap_or_default();
                ("Prune worktree", format!("{name}'s folder is gone, but it's locked{why}: its drive may only be unplugged. Prune it anyway?{branch}"), "Prune")
            } else {
                ("Remove worktree", format!("Delete worktree {name} and its folder?{lost}{lock}{shells}{branch}"), "Delete worktree")
            };
            let rx = ask(Ask::new(title, text, ok).danger(), window, cx);
            let force = changed > 0 || w.locked;
            after_yes(rx, window, cx, move |win, cx| with(&weak, win, cx, |s, win, cx| go_remove(s, w, name, force, then, win, cx)));
        });
    })
    .detach();
}

fn go_remove(
    state: &Entity<RepoState>,
    w: Worktree,
    name: String,
    force: bool,
    then: Option<Then>,
    window: &mut Window,
    cx: &mut App,
) {
    let weak = state.downgrade();
    let again = (w.clone(), name.clone());
    // git refuses any worktree with submodules checked out, clean or not, unless forced.
    let action = GitAction::new("Remove worktree").untracked().done(format!("Worktree {name} removed")).on_error(move |e, window, cx| {
        if force || !e.contains("working trees containing submodules cannot be moved or removed") {
            return false;
        }
        let rx = ask(
            Ask::new(
                "Remove worktree",
                format!("{} has submodules, which git only removes with force: everything in it goes, including commits made only inside its submodules. Delete anyway?", again.1),
                "Delete worktree",
            )
            .danger(),
            window,
            cx,
        );
        let (weak, w, name) = (weak.clone(), again.0.clone(), again.1.clone());
        after_yes(rx, window, cx, move |win, cx| with(&weak, win, cx, |s, win, cx| go_remove(s, w, name, true, None, win, cx)));
        true
    });
    let path = w.path.clone();
    let rx = run(
        state,
        action,
        move |core, _| plain(api::worktrees::remove_worktree(core, path, force)),
        window,
        cx,
    );
    if let Some(then) = then {
        after_yes(rx, window, cx, then);
    }
}

/// git keeps a branch for a worktree whose folder is gone until it's pruned, and refuses to
/// switch to it meanwhile.
pub fn prune_holder(
    state: &Entity<RepoState>,
    w: Worktree,
    branch: String,
    window: &mut Window,
    cx: &mut App,
) {
    let weak = state.downgrade();
    remove_worktree(
        state,
        w,
        Some(Box::new(move |win, cx| {
            with(&weak, win, cx, |s, win, cx| {
                switch_branch(s, branch, win, cx)
            })
        })),
        window,
        cx,
    );
}

/// The default branch back in the main folder, which git won't switch to while a linked worktree
/// holds it. Neither folder's files change, so Undo can hand it back while nothing moved on.
pub fn move_main_back(
    state: &Entity<RepoState>,
    branch: String,
    window: &mut Window,
    cx: &mut App,
) {
    let core = gitviber_ui::core(cx);
    let b = branch.clone();
    let plan = cx.background_spawn(async move { api::worktrees::main_back_plan(&core, b) });
    let (weak, handle) = (state.downgrade(), window.window_handle());
    cx.spawn(async move |cx| {
        let plan = plan.await;
        let _ = handle.update(cx, |_, window, cx| {
            let plan = match plan {
                Ok(p) => p,
                Err(e) => return Note::error(format!("Can't move {branch} back")).detail(e).show(window, cx),
            };
            let (into, held) = (folder_name(&plan.main).to_string(), folder_name(&plan.holder).to_string());
            let shells = crate::hooks::terminals_in(&plan.holder, cx);
            let note = if shells > 0 { format!(" {} open in {held} will be on a detached HEAD.", plural(shells, "terminal")) } else { String::new() };
            let text = format!(
                "{into} is on {}, and {branch} is checked out in {held}, so git won't switch {into} to it.\n\nThis detaches {held} where it stands (its files don't change), then switches {into} to {branch}. {} stays as a branch, and nothing is deleted.{note}",
                plan.main_branch, plan.main_branch
            );
            let rx = ask(Ask::new(format!("Move {branch} back to {into}"), text, "Move"), window, cx);
            after_yes(rx, window, cx, move |win, cx| {
                with(&weak, win, cx, |s, win, cx| {
                    let moving = branch.clone();
                    let rx = run(s, GitAction::new("Move main back").untracked(), move |core, _| plain(api::worktrees::move_main_back(core, moving)), win, cx);
                    let weak = s.downgrade();
                    after_yes(rx, win, cx, move |win, cx| {
                        let done = format!("{into} is back on {}, {held} on {branch}", plan.main_branch);
                        let title = format!("{branch} is back in {into}");
                        let detail = format!("{held} is detached at the same commit.");
                        let undo = Action::new("Undo", move |win, cx| {
                            with(&weak, win, cx, |s, win, cx| {
                                let plan = plan.clone();
                                go(s, GitAction::new("Undo").untracked().done(done.clone()), move |core, _| plain(api::worktrees::undo_main_back(core, plan)), win, cx);
                            })
                        });
                        Note::success(title).detail(detail).action(Some(undo)).show(win, cx);
                    });
                })
            });
        });
    })
    .detach();
}

pub fn lock_worktree(
    state: &Entity<RepoState>,
    w: &Worktree,
    reason: Option<String>,
    window: &mut Window,
    cx: &mut App,
) -> oneshot::Receiver<bool> {
    let path = w.path.clone();
    let action = GitAction::new("Lock worktree")
        .untracked()
        .done(format!("Locked {}", folder_name(&w.path)));
    run(
        state,
        action,
        move |core, _| plain(api::worktrees::lock_worktree(core, path, reason)),
        window,
        cx,
    )
}

pub fn unlock_worktree(state: &Entity<RepoState>, w: Worktree, window: &mut Window, cx: &mut App) {
    let name = folder_name(&w.path).to_string();
    let (in_use, reason) = (w.in_use, w.lock_reason.clone());
    let name_shown = name.clone();
    let weak = state.downgrade();
    let go = move |win: &mut Window, cx: &mut App| {
        with(&weak, win, cx, |s, win, cx| {
            let path = w.path.clone();
            go(
                s,
                GitAction::new("Unlock worktree")
                    .untracked()
                    .done(format!("Unlocked {name}")),
                move |core, _| plain(api::worktrees::unlock_worktree(core, path)),
                win,
                cx,
            );
        })
    };
    if !in_use {
        return go(window, cx);
    }
    let why = reason.unwrap_or_else(|| "it holds the lock".into());
    let text = format!(
        "Something is working in {} right now ({why}). Unlocked, it can be pruned, moved or removed while that runs.",
        name_shown
    );
    let rx = ask(
        Ask::new("Unlock worktree", text, "Unlock").danger(),
        window,
        cx,
    );
    after_yes(rx, window, cx, go);
}

/// Opens a worktree in this window; a branch another worktree holds opens that one, with a way back.
pub fn open_worktree(path: String, window: &mut Window, cx: &mut App) {
    if let Some(ws) = gitviber_ui::Workspace::for_window(window, cx) {
        ws.update(cx, |ws, cx| ws.switch_worktree(path.into(), cx));
    }
}

pub fn open_holder(
    state: &Entity<RepoState>,
    path: String,
    branch: String,
    window: &mut Window,
    cx: &mut App,
) {
    let from = state.read(cx).root().to_string_lossy().into_owned();
    let was = state
        .read(cx)
        .branch()
        .map(str::to_string)
        .unwrap_or_else(|| folder_name(&from).to_string());
    let name = folder_name(&path).to_string();
    open_worktree(path, window, cx);
    let back = Action::new(format!("Back to {was}"), move |w, cx| {
        open_worktree(from.clone(), w, cx)
    });
    Note::info(format!("Opened {name}"))
        .detail(format!(
            "{branch} is checked out there, and git keeps a branch in one worktree."
        ))
        .action(Some(back))
        .show(window, cx);
}

// MARK: Undo and redo

/// The branch undoing (or redoing) `steps` in order leaves checked out, if one of them switches.
pub fn landing(steps: &[Option<String>]) -> Option<String> {
    steps.iter().fold(None, |at, e| e.clone().or(at))
}

/// What a tooltip adds after the action's name: where it switches to, if it does.
pub fn switch_note(forward: bool, to: Option<&str>) -> String {
    to.map_or(String::new(), |t| {
        format!(": switches {}to {t}", if forward { "" } else { "back " })
    })
}

/// What to ask before undoing (or redoing) `steps`, or None to go ahead. Switching carries
/// uncommitted changes along, which ⌘Z pressed in passing shouldn't do unasked.
pub fn switch_question(
    forward: bool,
    steps: &[Option<String>],
    dirty: Option<bool>,
) -> Option<String> {
    let to = landing(steps)?;
    if dirty != Some(true) {
        return None;
    }
    Some(format!(
        "This {} switches {}to {to}, and your uncommitted changes go along to it.",
        if forward { "redo" } else { "undo" },
        if forward { "" } else { "back " }
    ))
}

/// What the undo history covers; the empty history and "Nothing to undo" both say it.
pub const UNDOABLE: &str =
    "Commits, merges, pulls, discards, and branch and tag changes made in GitViber";

/// Undoes (or redoes) the entries `ids` (with where each switches to), asking first when that
/// switch takes uncommitted changes along.
pub fn travel(
    state: &Entity<RepoState>,
    forward: bool,
    steps: Vec<(u64, Option<String>)>,
    window: &mut Window,
    cx: &mut App,
) {
    if state.read(cx).asking {
        return;
    }
    let dirty = state
        .read(cx)
        .status
        .as_ref()
        .map(|s| !s.staged.is_empty() || !s.unstaged.is_empty());
    let switches: Vec<Option<String>> = steps.iter().map(|(_, s)| s.clone()).collect();
    let ids: Vec<u64> = steps.iter().map(|(id, _)| *id).collect();
    let handle = window.window_handle();
    let weak = state.downgrade();
    let Some(q) = switch_question(forward, &switches, dirty) else {
        let _ = weak.update(cx, |s, cx| s.travel(forward, ids, handle, cx));
        return;
    };
    let verb = if forward { "Redo" } else { "Undo" };
    let rx = ask(
        Ask::new(
            format!("{verb} git action"),
            q,
            format!("{verb} and Switch"),
        ),
        window,
        cx,
    );
    state.update(cx, |s, _| s.asking = true);
    cx.spawn(async move |cx| {
        let yes = rx.await == Ok(true);
        let _ = weak.update(cx, |s, cx| {
            s.asking = false;
            if yes {
                s.travel(forward, ids, handle, cx);
            }
        });
    })
    .detach();
}

/// ⌘Z and ⇧⌘Z: the next step, or why nothing happened.
pub fn step(state: &Entity<RepoState>, forward: bool, window: &mut Window, cx: &mut App) {
    let s = state.read(cx);
    if s.busy().is_some() {
        return;
    }
    let verb = if forward { "redo" } else { "undo" };
    let Some(j) = s.journal.clone() else { return };
    let (next, blocked) = if forward {
        (j.redo.first(), j.redo_blocked.clone())
    } else {
        (j.undo.first(), j.undo_blocked.clone())
    };
    match (next, blocked) {
        (None, _) => Note::info(format!("Nothing to {verb}"))
            .detail(format!("{UNDOABLE} can be undone."))
            .show(window, cx),
        (Some(e), Some(why)) => Note::error(format!("Can't {verb} {}", e.label))
            .detail(why)
            .show(window, cx),
        (Some(e), None) => {
            let step = vec![(e.id, e.switch_to.clone())];
            travel(state, forward, step, window, cx)
        }
    }
}

/// The label a running action or a network command shows: theirs, or None.
pub fn activity(state: &Entity<RepoState>, cx: &App) -> Option<SharedString> {
    state.read(cx).busy().cloned()
}

/// For `CANCELLED` checks elsewhere.
pub fn cancelled(e: &str) -> bool {
    e == CANCELLED
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: Option<&str>) -> Option<String> {
        s.map(Into::into)
    }

    #[test]
    fn landing_is_the_last_switch_along_the_way() {
        assert_eq!(landing(&[]), None);
        assert_eq!(landing(&[at(None), at(None)]), None);
        assert_eq!(
            landing(&[at(Some("main")), at(None)]).as_deref(),
            Some("main")
        );
        assert_eq!(
            landing(&[at(Some("main")), at(Some("feat")), at(None)]).as_deref(),
            Some("feat")
        );
    }

    #[test]
    fn switch_note_says_back_only_for_undo() {
        assert_eq!(switch_note(false, Some("main")), ": switches back to main");
        assert_eq!(switch_note(true, Some("feat")), ": switches to feat");
        assert_eq!(switch_note(false, None), "");
    }

    #[test]
    fn asks_only_when_a_switch_takes_uncommitted_changes_along() {
        assert_eq!(
            switch_question(false, &[at(Some("main"))], Some(false)),
            None
        );
        assert_eq!(switch_question(false, &[at(None)], Some(true)), None);
        assert_eq!(switch_question(false, &[at(Some("main"))], None), None);
        assert!(
            switch_question(false, &[at(Some("main"))], Some(true))
                .unwrap()
                .contains("undo switches back to main")
        );
        assert!(
            switch_question(true, &[at(Some("feat"))], Some(true))
                .unwrap()
                .contains("redo switches to feat")
        );
    }
}
