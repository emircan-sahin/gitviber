//! What History's menus and drags do: undo, revert, reset, checkout, cherry-pick, branches and
//! tags at a commit, and the rewrites of the branch, each warning first when it would make
//! pushed commits again. Every action runs through `RepoState::run`, for its busy state, toasts
//! and refresh.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use gitviber_core::rewrite::Outcome;
use gitviber_core::{api, git};
use gitviber_refs::ask::{Ask, ask, ask_stacked};
use gitviber_refs::names::folder_name;
use gitviber_refs::toast::{Action, Note};
use gitviber_refs::{GitAction, repo::plain};
use gpui_kit::{App, AppContext as _, ClipboardItem, Context, Global, SharedString, Window};

use crate::commit::Commit;
use crate::dialogs::{Messaging, NameKind, Submit, open_message, open_name};
use crate::edits::{Edit, Where, edited_shas, kept_up_to, reorder_before};
use crate::list::{CommitList, ListEvent};

pub const PUSHED_WARNING: &str = "Some of these commits are already pushed, so you'd have to force-push, which rewrites history for everyone else on this branch.";
pub const MERGE_WARNING: &str = "This is a merge: the merged-in commits leave the branch too, and all of their changes end up staged together.";

/// What a menu item asks of the list for one commit.
#[derive(Clone, Debug)]
pub enum Act {
    OpenAll,
    /// A guided review of it, asked of the review agent unless one is kept.
    Explain,
    Undo,
    Reword,
    /// Into its parent; `message`: ask for the message, else the oldest one's stays.
    SquashParent {
        message: bool,
    },
    Split,
    FixupStaged,
    MoveUp,
    MoveDown,
    Drop,
    Revert,
    CherryPick,
    /// Onto the branch checked out in another worktree.
    PickInto {
        path: String,
        branch: String,
    },
    Reset(git::ResetMode),
    CompareWorktree,
    SelectForCompare,
    ClearCompare,
    CompareWithMark,
    Bisect,
    Checkout,
    CreateBranch,
    NewWorktree,
    CreateTag,
    PushTag(String),
    DeleteTag(String),
    DeleteRemoteTag(String),
    CopyTag(String),
    Hide(Vec<String>),
    Only(String),
    FilterAuthor,
    CopySha,
    CopyShort,
    CopyMessage,
    CopyPatch,
    CopyLink,
    OpenOnGitHub,
}

/// What a menu item asks of the list for the picked commits (newest first, as listed).
#[derive(Clone, Debug)]
pub enum ActMany {
    CherryPick,
    Squash { message: bool },
    Drop,
    CompareTwo,
    CopyShas,
}

/// What Select for Compare picked, in the repo at `root`: the old side of the next "Compare with".
#[derive(Clone, Debug, Default)]
pub struct CompareMark(pub Option<(String, String, String)>);
impl Global for CompareMark {}

pub fn mark(root: &str, cx: &App) -> Option<(String, String)> {
    cx.try_global::<CompareMark>()?
        .0
        .as_ref()
        .filter(|(r, _, _)| r == root)
        .map(|(_, s, l)| (s.clone(), l.clone()))
}

/// origin's page on GitHub, from status; None off github.com.
pub fn web_url(list: &CommitList, cx: &App) -> Option<String> {
    list.repo
        .read(cx)
        .status
        .as_ref()?
        .web_url
        .clone()
        .flatten()
}

/// GitHub only has commits that reached one of origin's branches.
pub fn commit_url(list: &CommitList, c: &Commit, cx: &App) -> Option<String> {
    let web = web_url(list, cx)?;
    c.on_origin.then(|| format!("{web}/commit/{}", c.sha))
}

fn copy(text: String, done: &str, window: &mut Window, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    Note::success(done.to_string()).show(window, cx);
}

fn target(list: &CommitList, cx: &App) -> String {
    list.repo.read(cx).branch().unwrap_or("HEAD").to_string()
}

/// Runs `work` as the git action `action`, in the window `window`.
pub fn run(
    list: &mut CommitList,
    action: GitAction,
    work: impl FnOnce(&gitviber_core::Core, &gitviber_refs::net::NetOp) -> gitviber_core::Res<bool>
    + Send
    + 'static,
    window: &Window,
    cx: &mut Context<CommitList>,
) -> oneshot::Receiver<bool> {
    list.repo
        .update(cx, |r, cx| r.run(action, work, window, cx))
}

pub fn act(
    list: &mut CommitList,
    c: &Rc<Commit>,
    act: Act,
    window: &mut Window,
    cx: &mut Context<CommitList>,
) {
    let sha = c.sha.clone();
    let short = c.short.to_string();
    match act {
        Act::OpenAll => crate::open::open_commit(list, c, window, cx),
        Act::Explain => {
            let commit = std::sync::Arc::new(c.to_core());
            gitviber_review::open_guide(
                gitviber_ui::workspace::GuideOf::Commit { commit },
                window,
                cx,
            );
        }
        Act::Undo => undo(list, c.clone(), window, cx),
        Act::Reword => {
            let submit = submit_rewrite(
                list,
                move |message, _| Edit::Reword {
                    sha: sha.clone(),
                    message,
                },
                vec![c.clone()],
                cx,
            );
            open_message(Messaging::Reword(c.clone()), submit, window, cx);
        }
        Act::SquashParent { message } => {
            if let Some(parent) = c.parents.first().cloned() {
                squash(list, vec![c.clone()], parent, message, window, cx);
            }
        }
        Act::Split => drop(rewrite(
            list,
            Edit::Split { sha },
            vec![c.clone()],
            window,
            cx,
        )),
        Act::FixupStaged => drop(rewrite(
            list,
            Edit::FixupStaged { sha },
            vec![c.clone()],
            window,
            cx,
        )),
        Act::MoveUp => drop(rewrite(
            list,
            Edit::Move { sha, up: true },
            vec![c.clone()],
            window,
            cx,
        )),
        Act::MoveDown => drop(rewrite(
            list,
            Edit::Move { sha, up: false },
            vec![c.clone()],
            window,
            cx,
        )),
        Act::Drop => drop(rewrite(
            list,
            Edit::Drop { shas: vec![sha] },
            vec![c.clone()],
            window,
            cx,
        )),
        Act::Revert => {
            let a = GitAction::new("Revert").done(format!("Reverted {short}"));
            drop(run(
                list,
                a,
                move |core, _| api::history::revert(core, sha),
                window,
                cx,
            ));
        }
        Act::CherryPick => {
            let a = GitAction::new("Cherry-pick")
                .done(format!("Cherry-picked {short} onto {}", target(list, cx)));
            drop(run(
                list,
                a,
                move |core, _| api::history::cherry_pick(core, sha),
                window,
                cx,
            ));
        }
        Act::PickInto { path, branch } => pick_into(list, c, path, branch, window, cx),
        Act::Reset(mode) => reset(list, c.clone(), mode, window, cx),
        Act::CompareWorktree => cx.emit(ListEvent::ComparePoints {
            base: (sha, short),
            head: None,
        }),
        Act::SelectForCompare => {
            let root = list.repo.read(cx).root().to_string_lossy().into_owned();
            cx.set_global(CompareMark(Some((root, sha, short.clone()))));
            let clear = Action::new("Clear", |_, cx| cx.set_global(CompareMark(None)));
            Note::info(format!("{short} selected for Compare"))
                .detail("Right-click another one and choose Compare with…")
                .action(Some(clear))
                .show(window, cx);
        }
        Act::ClearCompare => cx.set_global(CompareMark(None)),
        Act::CompareWithMark => {
            let root = list.repo.read(cx).root().to_string_lossy().into_owned();
            if let Some(base) = mark(&root, cx) {
                cx.emit(ListEvent::ComparePoints {
                    base,
                    head: Some((sha, short)),
                });
            }
        }
        Act::Bisect => bisect(list, sha, window, cx),
        Act::Checkout => {
            let message = format!(
                "Check out {short} without a branch (detached HEAD)? New commits made there belong to no branch until you create one."
            );
            let asked = ask(Ask::new("Checkout commit", message, "Checkout"), window, cx);
            list.flow = Some(cx.spawn_in(window, async move |this, cx| {
                if asked.await.unwrap_or(false) {
                    let _ = this.update_in(cx, |this, window, cx| {
                        let a = GitAction::new("Checkout").done(format!("Checked out {short}"));
                        drop(run(
                            this,
                            a,
                            move |core, _| plain(api::history::checkout_commit(core, sha)),
                            window,
                            cx,
                        ));
                    });
                }
            }));
        }
        Act::CreateBranch => {
            let weak = cx.weak_entity();
            let submit: Submit = Rc::new(move |name, _, window, cx| {
                let sha = sha.clone();
                weak.update(cx, |this, cx| {
                    let a = GitAction::new("Create branch")
                        .done(format!("Switched to new branch {name}"));
                    run(
                        this,
                        a,
                        move |core, _| plain(api::history::create_branch_at(core, name, sha)),
                        window,
                        cx,
                    )
                })
                .unwrap_or_else(|_| oneshot::channel().1)
            });
            open_name(NameKind::Branch, c, submit, window, cx);
        }
        Act::NewWorktree => {
            gitviber_refs::worktrees::open_new_worktree_dialog(Some(sha), window, cx)
        }
        Act::CreateTag => {
            let weak = cx.weak_entity();
            let submit: Submit = Rc::new(move |name, message, window, cx| {
                let (sha, short) = (sha.clone(), short.clone());
                weak.update(cx, |this, cx| {
                    let a = GitAction::new("Create tag").done(format!("Tagged {short} as {name}"));
                    run(
                        this,
                        a,
                        move |core, _| plain(api::branches::create_tag(core, name, sha, message)),
                        window,
                        cx,
                    )
                })
                .unwrap_or_else(|_| oneshot::channel().1)
            });
            open_name(NameKind::Tag, c, submit, window, cx);
        }
        Act::PushTag(tag) => {
            let a = GitAction::new("Push tag")
                .done(format!("Pushed tag {tag}"))
                .untracked()
                .net();
            drop(run(
                list,
                a,
                move |core, op| {
                    plain(api::branches::push_tags(
                        core,
                        vec![tag],
                        op.id.clone(),
                        op.progress(),
                    ))
                },
                window,
                cx,
            ));
        }
        Act::DeleteTag(tag) => {
            let a = GitAction::new("Delete tag").done(format!("Deleted tag {tag}"));
            drop(run(
                list,
                a,
                move |core, _| plain(api::branches::delete_tag(core, tag)),
                window,
                cx,
            ));
        }
        Act::DeleteRemoteTag(tag) => {
            let message = format!(
                "Delete tag {tag} from the remote? Clones that fetched it keep their copy, and GitViber can't undo this. The tag here stays."
            );
            let asked = ask(
                Ask::new("Delete remote tag", message, "Delete").danger(),
                window,
                cx,
            );
            list.flow = Some(cx.spawn_in(window, async move |this, cx| {
                if asked.await.unwrap_or(false) {
                    let _ = this.update_in(cx, |this, window, cx| {
                        let a = GitAction::new("Delete remote tag")
                            .done(format!("Deleted {tag} from the remote"))
                            .untracked()
                            .net();
                        drop(run(
                            this,
                            a,
                            move |core, op| {
                                plain(api::branches::delete_remote_tag(
                                    core,
                                    tag,
                                    op.id.clone(),
                                    op.progress(),
                                ))
                            },
                            window,
                            cx,
                        ));
                    });
                }
            }));
        }
        Act::CopyTag(tag) => copy(tag, "Tag name copied", window, cx),
        Act::Hide(refs) => cx.emit(ListEvent::HideRefs(refs)),
        Act::Only(r) => cx.emit(ListEvent::OnlyRef(r)),
        Act::FilterAuthor => cx.emit(ListEvent::FilterAuthor {
            name: c.author.to_string(),
            email: c.email.to_string(),
        }),
        Act::CopySha => copy(sha, "SHA copied", window, cx),
        Act::CopyShort => copy(short, "Short SHA copied", window, cx),
        Act::CopyMessage => copy(c.message(), "Message copied", window, cx),
        Act::CopyPatch => {
            let core = gitviber_ui::core(cx);
            let read = cx.background_spawn(async move { api::patch::commit_patch(&core, sha) });
            list.flow = Some(cx.spawn_in(window, async move |_, cx| {
                let patch = read.await;
                let _ = cx.update(|window, cx| match patch {
                    Ok(p) => copy(p, "Patch copied", window, cx),
                    Err(e) => Note::error("Could not copy the patch")
                        .detail(e)
                        .show(window, cx),
                });
            }));
        }
        Act::CopyLink => {
            if let Some(url) = commit_url(list, c, cx) {
                copy(url, "Link copied", window, cx);
            }
        }
        Act::OpenOnGitHub => {
            if let Some(url) = commit_url(list, c, cx) {
                cx.open_url(&url);
            }
        }
    }
}

pub fn act_many(
    list: &mut CommitList,
    picked: Vec<Rc<Commit>>,
    act: ActMany,
    window: &mut Window,
    cx: &mut Context<CommitList>,
) {
    match act {
        ActMany::CherryPick => {
            let missing: Vec<Rc<Commit>> = picked.into_iter().filter(|c| c.not_in_head).collect();
            // Oldest first onto HEAD.
            let shas: Vec<String> = missing.iter().rev().map(|c| c.sha.clone()).collect();
            let a = GitAction::new("Cherry-pick").done(format!(
                "Cherry-picked {} commits onto {}",
                shas.len(),
                target(list, cx)
            ));
            drop(run(
                list,
                a,
                move |core, _| api::history::cherry_pick_many(core, shas),
                window,
                cx,
            ));
        }
        ActMany::Squash { message } => {
            if let Some(oldest) = picked.last().map(|c| c.sha.clone()) {
                squash(list, picked, oldest, message, window, cx);
            }
        }
        ActMany::Drop => {
            let shas = picked.iter().map(|c| c.sha.clone()).collect();
            drop(rewrite(list, Edit::Drop { shas }, picked, window, cx));
        }
        ActMany::CompareTwo => {
            if let [newer, older] = &picked[..] {
                // Newest first: the older one is the old side.
                cx.emit(ListEvent::ComparePoints {
                    base: (older.sha.clone(), older.short.to_string()),
                    head: Some((newer.sha.clone(), newer.short.to_string())),
                });
            }
        }
        ActMany::CopyShas => copy(
            picked
                .iter()
                .map(|c| c.sha.clone())
                .collect::<Vec<_>>()
                .join("\n"),
            "SHAs copied",
            window,
            cx,
        ),
    }
}

fn undo(list: &mut CommitList, c: Rc<Commit>, window: &mut Window, cx: &mut Context<CommitList>) {
    let core = gitviber_ui::core(cx);
    let parent = c.parents.first().cloned();
    let check = cx.background_spawn(async move { api::history::drops_pushed(&core, parent) });
    list.flow = Some(cx.spawn_in(window, async move |this, cx| {
        let Some(drops) = checked(check.await, cx) else {
            return;
        };
        let mut warnings = vec![];
        if c.is_merge() {
            warnings.push(MERGE_WARNING);
        }
        if drops {
            warnings.push(PUSHED_WARNING);
        }
        if !warnings.is_empty() {
            let message = format!("Undo \"{}\"?\n\n{}", c.subject, warnings.join("\n\n"));
            let Ok(asked) =
                cx.update(|window, cx| ask(Ask::new("Undo commit", message, "Undo"), window, cx))
            else {
                return;
            };
            if !asked.await.unwrap_or(false) {
                return;
            }
        }
        let _ = this.update_in(cx, |this, window, cx| {
            let sha = c.sha.clone();
            let a = GitAction::new("Undo").done("Commit undone; its changes are staged");
            drop(run(
                this,
                a,
                move |core, _| plain(api::history::undo_commit(core, sha)),
                window,
                cx,
            ));
        });
    }));
}

/// The upstream comparison a warning needs, or its error said; None stops the action.
fn checked<T>(r: gitviber_core::Res<T>, cx: &mut gpui_kit::AsyncWindowContext) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            let _ = cx.update(|window, cx| {
                Note::error("Could not compare with the upstream")
                    .detail(e)
                    .show(window, cx)
            });
            None
        }
    }
}

fn reset(
    list: &mut CommitList,
    c: Rc<Commit>,
    mode: git::ResetMode,
    window: &mut Window,
    cx: &mut Context<CommitList>,
) {
    let core = gitviber_ui::core(cx);
    let at = Some(c.sha.clone());
    let check = cx.background_spawn(async move { api::history::drops_pushed(&core, at) });
    let target = target(list, cx);
    let head = list.head(cx);
    list.flow = Some(cx.spawn_in(window, async move |this, cx| {
        let Some(drops) = checked(check.await, cx) else { return };
        let hard = mode == git::ResetMode::Hard;
        let mut lines = vec![format!("Move {target} to {}?", c.short)];
        if hard {
            lines.push("Uncommitted changes to tracked files are discarded, and commits after this one leave the branch. This cannot be undone from GitViber.".into());
        }
        if drops {
            lines.push(PUSHED_WARNING.into());
        }
        if hard || drops {
            let title = match mode {
                git::ResetMode::Soft => "Soft reset",
                git::ResetMode::Mixed => "Mixed reset",
                git::ResetMode::Hard => "Hard reset",
            };
            let mut q = Ask::new(title, lines.join("\n\n"), "Reset");
            if hard {
                q = q.danger();
            }
            let Ok(asked) = cx.update(|window, cx| ask(q, window, cx)) else { return };
            if !asked.await.unwrap_or(false) {
                return;
            }
        }
        let _ = this.update_in(cx, |this, window, cx| {
            let sha = c.sha.clone();
            let a = GitAction::new("Reset").done(format!("{target} reset to {}", c.short));
            drop(run(this, a, move |core, _| plain(api::history::reset(core, sha, mode, head)), window, cx));
        });
    }));
}

/// git runs in that worktree, and the entry lands in its undo history, not this one's. A pick
/// stopped on conflicts waits there, for its own Changes panel to finish.
fn pick_into(
    list: &mut CommitList,
    c: &Rc<Commit>,
    path: String,
    branch: String,
    window: &mut Window,
    cx: &mut Context<CommitList>,
) {
    let core = gitviber_ui::core(cx);
    let (sha, short) = (c.sha.clone(), c.short.to_string());
    let where_ = folder_name(&path).to_string();
    let pick = cx.background_spawn({
        let path = path.clone();
        async move { api::history::cherry_pick_into(&core, path, sha) }
    });
    let project = list.repo.read(cx).project();
    list.flow = Some(cx.spawn_in(window, async move |this, cx| {
        let picked = pick.await;
        let _ = this.update_in(cx, |this, window, cx| {
            let go = project.map(|p| {
                let path = path.clone();
                Action::new("Switch to worktree", move |_, cx| {
                    p.update(cx, |p, cx| p.switch_worktree(path.clone().into(), cx))
                })
            });
            let note = match picked {
                Ok(true) => Note::info(format!("Cherry-pick onto {branch} stopped on conflicts"))
                    .detail(format!(
                        "It waits in {where_}: switch there to resolve them and continue."
                    ))
                    .action(go),
                Ok(false) => Note::success(format!("Cherry-picked {short} onto {branch}"))
                    .detail(format!("In {where_}; undo it from there."))
                    .action(go),
                Err(e) => Note::error(format!("Cherry-pick onto {branch} failed")).detail(e),
            };
            note.show(window, cx);
            this.repo.update(cx, |r, cx| r.refresh(cx));
        });
    }));
}

fn bisect(list: &mut CommitList, good: String, window: &mut Window, cx: &mut Context<CommitList>) {
    // It checks out other commits in this folder: a stray click shouldn't do that unasked.
    let message = format!(
        "Find the commit that broke something since {}, which still worked. Git checks out commits between it and HEAD in this folder, one at a time: test each one and mark it Good (no problem) or Bad (has the problem). Stop takes you back to your branch.",
        &good[..good.len().min(7)]
    );
    let asked = ask(
        Ask::new("Find the bad commit", message, "Start"),
        window,
        cx,
    );
    let core = gitviber_ui::core(cx);
    list.flow = Some(cx.spawn_in(window, async move |this, cx| {
        if !asked.await.unwrap_or(false) {
            return;
        }
        let step = cx
            .background_executor()
            .spawn(async move { api::history::bisect_start(&core, good) })
            .await;
        let _ = this.update_in(cx, |this, window, cx| {
            match step {
                Ok(step) => {
                    crate::bisect::said(Some((step.message, step.first_bad)), cx);
                    Note::info("Bisecting")
                        .detail("Test the commit checked out, then mark it good or bad.")
                        .show(window, cx);
                }
                Err(e) => Note::error("Could not start the bisect")
                    .detail(e)
                    .show(window, cx),
            }
            this.repo.update(cx, |r, cx| r.refresh(cx));
        });
    }));
}

/// Squashing: commits picked from elsewhere move to `onto` first, older ones below it and newer
/// ones above, then they become one there. `message`: ask for the message, else the oldest one's
/// stays (fixup).
pub fn squash(
    list: &mut CommitList,
    about: Vec<Rc<Commit>>,
    onto: String,
    message: bool,
    window: &mut Window,
    cx: &mut Context<CommitList>,
) {
    let index = |sha: &str| list.commits.iter().position(|c| c.sha == sha);
    let mut all: Vec<Rc<Commit>> = about.clone();
    if let Some(t) = list.commit(&onto).cloned()
        && !all.iter().any(|c| c.sha == t.sha)
    {
        all.push(t);
    }
    // Oldest first.
    all.sort_by_key(|c| std::cmp::Reverse(index(&c.sha).unwrap_or(usize::MAX)));
    let mut seen = HashSet::new();
    let shas: Vec<String> = about
        .iter()
        .map(|c| c.sha.clone())
        .filter(|s| *s != onto && seen.insert(s.clone()))
        .collect();
    if !message {
        drop(rewrite(
            list,
            Edit::Squash {
                shas,
                onto,
                message: None,
            },
            all,
            window,
            cx,
        ));
        return;
    }
    // Only a whole list shows what lies between them.
    let (lo, hi) = (
        all.iter().filter_map(|c| index(&c.sha)).min(),
        all.iter().filter_map(|c| index(&c.sha)).max(),
    );
    let apart = list.source().graph()
        && list.commit(&onto).is_some()
        && lo.zip(hi).is_some_and(|(lo, hi)| {
            list.commits[lo..=hi]
                .iter()
                .any(|c| !c.not_in_head && !all.iter().any(|a| a.sha == c.sha))
        });
    let count = shas.len() + 1;
    let about_for = all.clone();
    let onto_for = onto.clone();
    let submit = submit_rewrite(
        list,
        move |message, _| Edit::Squash {
            shas: shas.clone(),
            onto: onto_for.clone(),
            message: Some(message),
        },
        about_for,
        cx,
    );
    open_message(
        Messaging::Squash {
            commits: all,
            count,
            onto,
            apart,
        },
        submit,
        window,
        cx,
    );
}

fn submit_rewrite(
    _list: &CommitList,
    edit: impl Fn(String, Option<String>) -> Edit + 'static,
    about: Vec<Rc<Commit>>,
    cx: &mut Context<CommitList>,
) -> Submit {
    let weak = cx.weak_entity();
    Rc::new(move |message, extra, window, cx| {
        let edit = edit(message, extra);
        let about = about.clone();
        weak.update(cx, |this, cx| rewrite(this, edit, about, window, cx))
            .unwrap_or_else(|_| oneshot::channel().1)
    })
}

/// Commits dragged onto a commit squash into it; into a gap they move there.
pub fn drop_commits(
    list: &mut CommitList,
    shas: Vec<String>,
    sha: String,
    at: Where,
    window: &mut Window,
    cx: &mut Context<CommitList>,
) {
    let moved: Vec<Rc<Commit>> = list
        .commits
        .iter()
        .filter(|c| shas.contains(&c.sha))
        .cloned()
        .collect();
    if moved.is_empty() || list.commit(&sha).is_none() {
        return;
    }
    if at == Where::Onto {
        squash(list, moved, sha, true, window, cx);
        return;
    }
    let line: Vec<String> = list.commits.iter().map(|c| c.sha.clone()).collect();
    let set: HashSet<String> = shas.iter().cloned().collect();
    if let Some(before) = reorder_before(&line, &set, &sha, at) {
        drop(rewrite(
            list,
            Edit::Reorder { shas, before },
            moved,
            window,
            cx,
        ));
    }
}

/// Rewrites the branch's history: warns first when it makes pushed commits again, offers moving
/// the branches stacked on it along, then runs. Resolves to whether it ran.
pub fn rewrite(
    list: &mut CommitList,
    edit: Edit,
    about: Vec<Rc<Commit>>,
    window: &mut Window,
    cx: &mut Context<CommitList>,
) -> oneshot::Receiver<bool> {
    let (tx, rx) = oneshot::channel();
    let kept = kept_up_to(&edited_shas(&edit, &about), &list.commits);
    let head = list.head(cx);
    let core = gitviber_ui::core(cx);
    let probe = edit.to_core();
    let check = cx.background_spawn(async move {
        // None: from the root, where every commit is made again.
        let drops = api::history::drops_pushed(&core, kept);
        let stacked = api::history::stacked_branches(&core, probe).ok();
        (drops, stacked)
    });
    list.flow = Some(cx.spawn_in(window, async move |this, cx| {
        let (drops, stacked) = check.await;
        let Some(drops) = checked(drops, cx) else {
            let _ = tx.send(false);
            return;
        };
        let n = about.len();
        let c = about.first().cloned();
        let verb = edit.verb();
        let mut warnings: Vec<String> = vec![];
        if let Edit::Drop { .. } = edit {
            warnings.push(match (&c, n) {
                (Some(c), 1) => format!("Drop \"{}\"? Its changes leave the branch.", c.subject),
                _ => format!("Drop {n} commits? Their changes leave the branch."),
            });
        }
        if drops {
            warnings.push(PUSHED_WARNING.into());
        }
        let title = format!("{verb} commit{}", if n == 1 { "" } else { "s" });
        let mut branches = false;
        match stacked.filter(|s| !s.branches.is_empty()) {
            Some(s) => {
                let message = warnings.join("\n\n");
                let Ok(asked) = cx.update(|window, cx| ask_stacked(title.clone(), message, verb, s.branches, s.update_refs, window, cx)) else {
                    return;
                };
                match asked.await.ok().flatten() {
                    Some(m) => branches = m,
                    None => {
                        let _ = tx.send(false);
                        return;
                    }
                }
            }
            None if !warnings.is_empty() => {
                let mut q = Ask::new(title.clone(), warnings.join("\n\n"), verb);
                if matches!(edit, Edit::Drop { .. }) {
                    q = q.danger();
                }
                let Ok(asked) = cx.update(|window, cx| ask(q, window, cx)) else { return };
                if !asked.await.unwrap_or(false) {
                    let _ = tx.send(false);
                    return;
                }
            }
            None => {}
        }
        let what = match (&c, n) {
            (Some(c), 1) => c.short.to_string(),
            _ => format!("{n} commits"),
        };
        let done: Option<String> = match &edit {
            Edit::Squash { shas, onto, message } => {
                let v = if message.is_none() { "Fixed up" } else { "Squashed" };
                Some(if shas.len() > 1 {
                    format!("{v} {} commits into one", shas.len() + 1)
                } else {
                    let sha = shas.first().cloned().unwrap_or_default();
                    let parent = about.iter().find(|c| c.sha == sha).and_then(|c| c.parents.first()) == Some(onto);
                    format!("{v} {} into {}", &sha[..sha.len().min(7)], if parent { "its parent".to_string() } else { onto[..onto.len().min(7)].to_string() })
                })
            }
            Edit::Reword { .. } => Some("Commit reworded".into()),
            Edit::Drop { .. } => Some(format!("Dropped {what}")),
            Edit::Move { .. } | Edit::Reorder { .. } => Some(format!("Moved {what}")),
            Edit::FixupStaged { .. } => c.as_ref().map(|c| format!("Fixed up {} with the staged changes", c.short)),
            Edit::Split { .. } => None,
        };
        let outcome: Arc<Mutex<Option<Outcome>>> = Arc::default();
        let seen = outcome.clone();
        let core_edit = edit.to_core();
        let Ok(ran) = this.update_in(cx, |this, window, cx| {
            let mut a = GitAction::new(verb);
            if let Some(done) = done {
                a = a.done(done);
            }
            run(
                this,
                a,
                move |core, _| {
                    let o = api::history::rewrite(core, head, core_edit, branches)?;
                    let stopped = o == Outcome::Conflicts;
                    *seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(o);
                    Ok(stopped)
                },
                window,
                cx,
            )
        }) else {
            return;
        };
        let ok = ran.await.unwrap_or(false);
        let outcome = outcome.lock().unwrap_or_else(|e| e.into_inner()).take();
        let short: SharedString = c.map(|c| c.short.clone()).unwrap_or_default();
        let _ = cx.update(|window, cx| match outcome {
            Some(Outcome::Split) if ok => Note::info(format!("Splitting {short}"))
                .detail("Its changes are unstaged in Changes. Commit them in pieces with line staging, then Continue.")
                .show(window, cx),
            // Done, but the uncommitted changes set aside for it didn't come back cleanly.
            Some(Outcome::StashConflicts) if ok => Note::info("Your uncommitted changes conflict with the new history")
                .detail("They're marked conflicted in Changes: resolve them there. git keeps a copy in Stashes too; drop it once resolved.")
                .show(window, cx),
            _ => {}
        });
        let _ = tx.send(ok);
    }));
    rx
}
