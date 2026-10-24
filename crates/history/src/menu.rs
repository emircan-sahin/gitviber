//! Right-click menus: on a commit, on several picked together, and on a file of a commit.

use std::rc::Rc;

use gitviber_core::git;
use gitviber_refs::names::folder_name;
use gitviber_refs::remote_tags::{self, There};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{App, ClickEvent, Context, Entity, SharedString, Window};

use crate::act::{Act, ActMany, act, act_many, commit_url, mark, web_url};
use crate::commit::{Commit, RefKind, group_refs};
use crate::list::{CommitList, Source};

fn on(
    list: &Entity<CommitList>,
    c: &Rc<Commit>,
    a: Act,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let (list, c) = (list.downgrade(), c.clone());
    move |_, window, cx| {
        let _ = list.update(cx, |l, cx| act(l, &c, a.clone(), window, cx));
    }
}

fn on_many(
    list: &Entity<CommitList>,
    picked: &[Rc<Commit>],
    a: ActMany,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let (list, picked) = (list.downgrade(), picked.to_vec());
    move |_, window, cx| {
        let _ = list.update(cx, |l, cx| {
            act_many(l, picked.clone(), a.clone(), window, cx)
        });
    }
}

fn item(label: impl Into<SharedString>, icon: Option<IconName>, disabled: bool) -> PopupMenuItem {
    let it = PopupMenuItem::new(label).disabled(disabled);
    match icon {
        Some(i) => it.icon(Icon::new(i)),
        None => it,
    }
}

/// A submenu, or the same row disabled.
fn sub(
    menu: PopupMenu,
    label: &str,
    icon: IconName,
    disabled: bool,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
) -> PopupMenu {
    if disabled {
        menu.item(item(label.to_string(), Some(icon), true))
    } else {
        menu.submenu_with_icon(Some(Icon::new(icon)), label.to_string(), window, cx, f)
    }
}

/// A tag's submenu; `there` None while the remote is asked.
fn tag_items(
    m: PopupMenu,
    list: &Entity<CommitList>,
    c: &Rc<Commit>,
    tag: &str,
    locked: bool,
    there: Option<&There>,
) -> PopupMenu {
    let known = there.and_then(|t| t.as_ref().ok());
    let here = known.map(|(_, names)| names.iter().any(|n| n == tag));
    let remote = known.map(|(r, _)| r.as_str());
    let label = match there {
        None => "Checking the remote…".to_string(),
        Some(Ok((r, _))) if here == Some(true) => format!("On {r}"),
        Some(Ok((r, _))) => format!("Not on {r} yet"),
        Some(Err(e)) => format!("Couldn't ask the remote: {e}"),
    };
    let tag = tag.to_string();
    m.label(label)
        .item(
            item(
                match remote {
                    Some(r) => format!("Push tag to {r}"),
                    None => "Push tag".into(),
                },
                Some(IconName::Upload),
                here == Some(true),
            )
            .on_click(on(list, c, Act::PushTag(tag.clone()))),
        )
        .item(
            item("Delete tag", Some(IconName::Trash), locked).on_click(on(
                list,
                c,
                Act::DeleteTag(tag.clone()),
            )),
        )
        .item(
            item(
                format!("Delete from {}…", remote.unwrap_or("the remote")),
                Some(IconName::Trash),
                here == Some(false),
            )
            .on_click(on(list, c, Act::DeleteRemoteTag(tag.clone()))),
        )
        .separator()
        .item(item("Copy name", Some(IconName::Copy), false).on_click(on(
            list,
            c,
            Act::CopyTag(tag),
        )))
}

/// Right-click actions on a commit; on one of several picked, on them all.
pub fn commit_menu(
    menu: PopupMenu,
    list: &Entity<CommitList>,
    c: &Rc<Commit>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let l = list.read(cx);
    if l.picked.len() > 1 && l.picked.iter().any(|s| *s == c.sha) {
        let picked = l.picked_commits();
        let all = !l.has_more() && picked.len() == l.len();
        return picked_menu(menu, list, &picked, all, cx);
    }
    let repo = l.repo.read(cx);
    let head_sha = l.head(cx);
    let head = c.sha == head_sha;
    let locked = repo.locked();
    let target = repo.branch().unwrap_or("HEAD").to_string();
    let staged = repo.status.as_ref().is_some_and(|s| !s.staged.is_empty());
    let operation = repo.status.as_ref().is_some_and(|s| s.operation.is_some());
    let busy = repo.busy().is_some();
    let root = repo.root().to_string_lossy().into_owned();
    let picks: Vec<(String, String)> = repo
        .worktrees
        .iter()
        .filter(|w| !w.current && !w.bare && !w.prunable)
        .filter_map(|w| Some((w.path.clone(), w.branch.clone()?)))
        .collect();
    let url = commit_url(l, c, cx);
    let web = l.options.github.is_some() || web_url(l, cx).is_some();
    let marked = mark(&root, cx);
    let ref_menu = l.options.ref_menu;
    let filter_author = l.options.filter_author && !c.author.is_empty();
    let graph_badges: Vec<_> = if ref_menu {
        group_refs(&c.refs, &l.remotes(cx), l.options.show_refs.as_ref())
            .into_iter()
            .filter(|b| !b.refs.is_empty())
            .collect()
    } else {
        vec![]
    };
    let compares = !matches!(l.source(), Source::Between { .. });
    let tags: Vec<String> = c.tags().into_iter().map(str::to_string).collect();
    let no_parent = c.parents.is_empty();
    let edit_off = locked || c.not_in_head;

    let mut menu = menu
        .item(
            item("Open All Changes", Some(IconName::Files), false).on_click(on(
                list,
                c,
                Act::OpenAll,
            )),
        )
        .item(
            item("Explain Commit", Some(IconName::Sparkles), false).on_click(on(
                list,
                c,
                Act::Explain,
            )),
        )
        .separator()
        .item(
            item(
                "Undo commit",
                Some(IconName::Undo2),
                locked || !head || no_parent,
            )
            .on_click(on(list, c, Act::Undo)),
        );
    {
        let (list, c) = (list.clone(), c.clone());
        // Only the branch's own commits: a commit HEAD lacks isn't its history to edit.
        menu = sub(
            menu,
            "Edit history",
            IconName::Pencil,
            edit_off,
            window,
            cx,
            move |m, _, _| {
                m.item(item("Reword…", None, false).on_click(on(&list, &c, Act::Reword)))
                    .item(item("Squash into Parent…", None, no_parent).on_click(on(
                        &list,
                        &c,
                        Act::SquashParent { message: true },
                    )))
                    .item(
                        item("Fixup into Parent (keeps its message)", None, no_parent)
                            .on_click(on(&list, &c, Act::SquashParent { message: false })),
                    )
                    .separator()
                    // Its changes come back unstaged on its parent; a merge or the first commit has none to take apart.
                    .item(
                        item(
                            "Split Commit…",
                            Some(IconName::Scissors),
                            c.parents.len() != 1,
                        )
                        .on_click(on(&list, &c, Act::Split)),
                    )
                    .item(
                        item(
                            "Fixup Staged Changes into This Commit",
                            Some(IconName::GitCommitVertical),
                            !staged,
                        )
                        .on_click(on(&list, &c, Act::FixupStaged)),
                    )
                    .separator()
                    .item(item("Move Up", Some(IconName::ArrowUp), head).on_click(on(
                        &list,
                        &c,
                        Act::MoveUp,
                    )))
                    .item(
                        item("Move Down", Some(IconName::ArrowDown), no_parent).on_click(on(
                            &list,
                            &c,
                            Act::MoveDown,
                        )),
                    )
                    .separator()
                    // The only commit: dropping it would leave the branch with none.
                    .item(
                        item("Drop Commit…", Some(IconName::Trash), head && no_parent)
                            .on_click(on(&list, &c, Act::Drop)),
                    )
            },
        );
    }
    // Reverting a commit HEAD never had would apply the opposite of a change that isn't there.
    menu = menu.item(
        item(
            "Revert commit",
            Some(IconName::RotateCcw),
            locked || c.not_in_head,
        )
        .on_click(on(list, c, Act::Revert)),
    );
    // Only a commit HEAD lacks: picking one it has changes nothing.
    if c.not_in_head {
        menu = menu.item(
            item(
                format!("Cherry-pick onto {target}"),
                Some(IconName::Cherry),
                locked,
            )
            .on_click(on(list, c, Act::CherryPick)),
        );
    }
    if !picks.is_empty() {
        let (list, c) = (list.clone(), c.clone());
        // Another worktree's lock is its own: only this one's action in progress holds it back.
        menu = sub(
            menu,
            "Cherry-pick onto",
            IconName::Cherry,
            busy && !operation,
            window,
            cx,
            move |mut m, _, _| {
                for (path, branch) in &picks {
                    let label = format!("{branch}  ·  {}", folder_name(path));
                    m = m.item(item(label, None, false).on_click(on(
                        &list,
                        &c,
                        Act::PickInto {
                            path: path.clone(),
                            branch: branch.clone(),
                        },
                    )));
                }
                m
            },
        );
    }
    {
        let (list, c) = (list.clone(), c.clone());
        menu = sub(
            menu,
            &format!("Reset {target} to here"),
            IconName::RotateCcwClock,
            locked,
            window,
            cx,
            move |m, _, _| {
                m.item(item("Soft · keep changes staged", None, false).on_click(on(
                    &list,
                    &c,
                    Act::Reset(git::ResetMode::Soft),
                )))
                .item(
                    item("Mixed · keep changes unstaged", None, false).on_click(on(
                        &list,
                        &c,
                        Act::Reset(git::ResetMode::Mixed),
                    )),
                )
                .item(item("Hard · discard changes", None, false).on_click(on(
                    &list,
                    &c,
                    Act::Reset(git::ResetMode::Hard),
                )))
            },
        );
    }
    if compares {
        menu = menu.separator().item(
            item(
                "Compare with Working Tree",
                Some(IconName::GitCompareArrows),
                false,
            )
            .on_click(on(list, c, Act::CompareWorktree)),
        );
        menu = match &marked {
            Some((sha, _)) if *sha == c.sha => menu.item(
                item(
                    "Clear Compare Selection",
                    Some(IconName::SquareDashedMousePointer),
                    false,
                )
                .on_click(on(list, c, Act::ClearCompare)),
            ),
            _ => menu.item(
                item(
                    "Select for Compare",
                    Some(IconName::SquareDashedMousePointer),
                    false,
                )
                .on_click(on(list, c, Act::SelectForCompare)),
            ),
        };
        if let Some((sha, label)) = &marked
            && *sha != c.sha
        {
            menu = menu.item(
                item(
                    format!("Compare with {label}"),
                    Some(IconName::GitCompare),
                    false,
                )
                .on_click(on(list, c, Act::CompareWithMark)),
            );
        }
    }
    // HEAD has the bug, this commit didn't: git halves what's between until it finds where it came in.
    menu = menu
        .separator()
        .item(
            item(
                "Find the Bad Commit Since Here…",
                Some(IconName::SearchCode),
                locked || head || c.not_in_head,
            )
            .on_click(on(list, c, Act::Bisect)),
        )
        .item(
            item(
                "Checkout commit",
                Some(IconName::GitCommitHorizontal),
                locked || head,
            )
            .on_click(on(list, c, Act::Checkout)),
        )
        .item(
            item(
                "Create branch from here…",
                Some(IconName::GitBranchPlus),
                locked,
            )
            .on_click(on(list, c, Act::CreateBranch)),
        )
        // Not held back by an operation here: it's another worktree's checkout.
        .item(
            item("New worktree from here…", Some(IconName::FolderGit2), false).on_click(on(
                list,
                c,
                Act::NewWorktree,
            )),
        )
        .item(
            item("Create tag here…", Some(IconName::Tag), locked).on_click(on(
                list,
                c,
                Act::CreateTag,
            )),
        );
    // Submenus are built with the menu: the remote is asked as it opens.
    let there = (!tags.is_empty()).then(|| {
        let root = list.read(cx).repo.read(cx).root().clone();
        remote_tags::lookup(&root, cx)
    });
    for tag in tags {
        let (list, c, there) = (list.clone(), c.clone(), there.clone());
        menu = menu.submenu_with_icon(
            Some(Icon::new(IconName::Tag)),
            tag.clone(),
            window,
            cx,
            move |m, window, cx| {
                if let Some(there) = there.clone() {
                    let (list, c, tag) = (list.clone(), c.clone(), tag.clone());
                    cx.spawn_in(window, async move |menu, cx| {
                        let answer = there.await;
                        let _ = menu.update_in(cx, |menu, window, cx| {
                            menu.rebuild(window, cx, |m, _, _| {
                                tag_items(m, &list, &c, &tag, locked, Some(&answer))
                            });
                        });
                    })
                    .detach();
                }
                tag_items(m, &list, &c, &tag, locked, None)
            },
        );
    }
    if !graph_badges.is_empty() {
        menu = menu.separator();
        for b in graph_badges {
            menu = menu.item(
                item(
                    format!("Hide {} in graph", b.name),
                    Some(IconName::EyeOff),
                    false,
                )
                .on_click(on(list, c, Act::Hide(b.refs.clone()))),
            );
            if b.kind != RefKind::Tag {
                menu =
                    menu.item(
                        item(format!("Show only {}", b.name), Some(IconName::Eye), false)
                            .on_click(on(list, c, Act::Only(b.refs[0].clone()))),
                    );
            }
        }
    }
    if filter_author {
        menu = menu.separator().item(
            item(
                format!("Filter by author {}", c.author),
                Some(IconName::ListFilter),
                false,
            )
            .on_click(on(list, c, Act::FilterAuthor)),
        );
    }
    menu = menu
        .separator()
        .item(item("Copy SHA", Some(IconName::Copy), false).on_click(on(list, c, Act::CopySha)))
        .item(
            item("Copy short SHA", Some(IconName::Copy), false).on_click(on(
                list,
                c,
                Act::CopyShort,
            )),
        )
        .item(
            item("Copy message", Some(IconName::Copy), false).on_click(on(
                list,
                c,
                Act::CopyMessage,
            )),
        )
        .item(
            item("Copy as Patch", Some(IconName::FileDiff), c.is_merge()).on_click(on(
                list,
                c,
                Act::CopyPatch,
            )),
        );
    if web {
        menu = menu
            .item(
                item("Copy link", Some(IconName::Link), url.is_none()).on_click(on(
                    list,
                    c,
                    Act::CopyLink,
                )),
            )
            .item(
                item(
                    "Open on GitHub",
                    Some(IconName::ExternalLink),
                    url.is_none(),
                )
                .on_click(on(list, c, Act::OpenOnGitHub)),
            );
    }
    menu
}

/// Right-click actions on several commits picked together (newest first, as listed); `all`:
/// every commit of the branch.
fn picked_menu(
    menu: PopupMenu,
    list: &Entity<CommitList>,
    picked: &[Rc<Commit>],
    all: bool,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let l = list.read(cx);
    let repo = l.repo.read(cx);
    let n = picked.len();
    // Only the branch's own commits, as for one.
    let off = repo.locked() || picked.iter().any(|c| c.not_in_head);
    let missing = picked.iter().filter(|c| c.not_in_head).count();
    let target = repo.branch().unwrap_or("HEAD").to_string();
    let locked = repo.locked();
    let mut menu = menu.label(format!("{n} commits"));
    if missing > 0 {
        menu = menu
            .item(
                item(
                    format!("Cherry-pick {missing} Commits onto {target}"),
                    Some(IconName::Cherry),
                    locked,
                )
                .on_click(on_many(list, picked, ActMany::CherryPick)),
            )
            .separator();
    }
    menu = menu
        .item(
            item(format!("Squash {n} Commits…"), Some(IconName::Combine), off).on_click(on_many(
                list,
                picked,
                ActMany::Squash { message: true },
            )),
        )
        .item(
            item(
                "Fixup into Oldest (keeps its message)",
                Some(IconName::Combine),
                off,
            )
            .on_click(on_many(list, picked, ActMany::Squash { message: false })),
        )
        .separator()
        .item(
            item(
                format!("Drop {n} Commits…"),
                Some(IconName::Trash),
                off || all,
            )
            .on_click(on_many(list, picked, ActMany::Drop)),
        );
    if n == 2 {
        menu = menu.separator().item(
            item("Compare These Two", Some(IconName::GitCompare), false).on_click(on_many(
                list,
                picked,
                ActMany::CompareTwo,
            )),
        );
    }
    menu.separator().item(
        item("Copy SHAs", Some(IconName::Copy), false).on_click(on_many(
            list,
            picked,
            ActMany::CopyShas,
        )),
    )
}

/// Right-click actions on a file of a commit: the working tree takes its version, or its change
/// undone.
pub fn file_menu(
    menu: PopupMenu,
    list: &Entity<CommitList>,
    c: &Rc<Commit>,
    f: &git::FileChange,
    _: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
    let open = {
        let (list, c, f) = (list.downgrade(), c.clone(), f.clone());
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let _ = list.update(cx, |l, cx| {
                crate::open::open_commit_file(l, &c, &f, false, window, cx)
            });
        }
    };
    let file = |verb: &'static str| {
        let (list, c, f, name) = (list.downgrade(), c.clone(), f.clone(), name.clone());
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let _ = list.update(cx, |l, cx| {
                crate::open::file_action(l, &c, &f, &name, verb, window, cx)
            });
        }
    };
    let open_file = {
        let (ws, path) = (list.read(cx).workspace.clone(), f.path.clone());
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let sel = gitviber_ui::workspace::Selection::File { path: path.clone() };
            let _ = ws.update(cx, |ws, cx| ws.open(sel, false, window, cx));
        }
    };
    menu.item(item("Open Changes", Some(IconName::FileDiff), false).on_click(open))
        .item(item("Open File", Some(IconName::File), false).on_click(open_file))
        .item(
            item("Open All Changes in Commit", Some(IconName::Files), false).on_click(on(
                list,
                c,
                Act::OpenAll,
            )),
        )
        .separator()
        // Deleted there: nothing to take.
        .item(
            item(
                "Restore This Version",
                Some(IconName::RotateCcwClock),
                f.status == "D",
            )
            .on_click(file("restore")),
        )
        .item(
            item(
                "Revert This File's Change",
                Some(IconName::RotateCcw),
                false,
            )
            .on_click(file("revert")),
        )
        .separator()
        .item(item("Copy as Patch", Some(IconName::FileDiff), false).on_click(file("patch")))
}
