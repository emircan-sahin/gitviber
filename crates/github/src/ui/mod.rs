//! The views: the PRs and Issues panels, a PR's, a PR file's and an issue's tab, their dialogs.

pub(crate) mod common;
mod connect;
mod dialogs;
mod issue_view;
mod issues_panel;
mod labels;
mod list;
mod markdown_input;
mod page_find;
mod pull_file;
pub(crate) mod pull_view;
pub(crate) mod pulls_panel;
mod rows;

use std::sync::Arc;

use gitviber_core::git::FileChange;
use gitviber_core::github::{Issue, Label, Pull};
use gitviber_ui::workspace::{
    PanelSpec, Project, ProjectEvent, PullRange, Selection, Slot, TabHandle, Toast, Workspace,
    register_opener, register_panel,
};
use gpui_kit::{App, AppContext as _, Global, WeakEntity, Window};

pub use issue_view::IssueView;
pub use issues_panel::IssuesPanel;
pub use pull_file::PullFileView;
pub use pull_view::PullView;
pub use pulls_panel::PullsPanel;

use crate::cache::Cache;
use crate::model::{IssueItem, PullItem, PullState};

pub(crate) fn init(cx: &mut App) {
    follow_projects(cx);
    follow_branch_lists(cx);
    register_panel(
        cx,
        PanelSpec::new("pulls", Slot::Left, "PRs", |ctx, window, cx| {
            cx.new(|cx| PullsPanel::new(ctx, window, cx))
        })
        .order(2),
    );
    register_panel(
        cx,
        PanelSpec::new("issues", Slot::Left, "Issues", |ctx, window, cx| {
            cx.new(|cx| IssuesPanel::new(ctx, window, cx))
        })
        .order(3),
    );
    register_opener(cx, "pull", |sel, window, cx| {
        let Selection::Pull { pull } = sel else {
            return None;
        };
        let ws = Workspace::for_window(window, cx)?.downgrade();
        let item = PullItem::from(&**pull);
        Some(TabHandle::new(cx.new(|cx| {
            PullView::new(ws, item.url.clone(), Some(item), window, cx)
        })))
    });
    register_opener(cx, "issue", |sel, window, cx| {
        let Selection::Issue { issue } = sel else {
            return None;
        };
        let ws = Workspace::for_window(window, cx)?.downgrade();
        let item = IssueItem::from(&**issue);
        Some(TabHandle::new(cx.new(|cx| {
            IssueView::new(ws, item.url.clone(), Some(item), window, cx)
        })))
    });
    register_opener(cx, "pr-file", |sel, window, cx| {
        let Selection::PrFile { range, file } = sel else {
            return None;
        };
        let ws = Workspace::for_window(window, cx)?.downgrade();
        let (range, file) = (range.clone(), file.clone());
        Some(TabHandle::new(
            cx.new(|cx| PullFileView::new(ws, range, file, window, cx)),
        ))
    });
}

/// core's shape of a list's item, which a selection holds.
pub(crate) fn core_pull(p: &PullItem) -> Pull {
    Pull {
        number: p.number,
        title: p.title.clone(),
        state: match p.state {
            PullState::Open => "open",
            PullState::Closed => "closed",
            PullState::Merged => "merged",
        }
        .into(),
        draft: p.draft,
        author: p.author.clone(),
        head_ref: p.head_ref.clone(),
        head_sha: p.head_sha.clone(),
        head_repo: p.head_repo.clone(),
        base_ref: p.base_ref.clone(),
        base_sha: p.base_sha.clone(),
        created_at: p.created_at.clone(),
        updated_at: p.updated_at.clone(),
        url: p.url.clone(),
    }
}

pub(crate) fn core_issue(i: &IssueItem) -> Issue {
    Issue {
        number: i.number,
        title: i.title.clone(),
        state: if i.open { "open" } else { "closed" }.into(),
        state_reason: i.state_reason.clone(),
        author: i.author.clone(),
        labels: i
            .labels
            .iter()
            .map(|l| Label {
                name: l.name.clone(),
                color: l.color.clone(),
                description: l.description.clone(),
            })
            .collect(),
        assignees: i.assignees.clone(),
        comments: i.comments,
        created_at: i.created_at.clone(),
        updated_at: i.updated_at.clone(),
        url: i.url.clone(),
    }
}

/// The tab key a PR's page has, as `selection_key` names it.
pub(crate) fn pull_key(url: &str) -> String {
    format!("pull:{url}")
}

fn open(
    ws: &WeakEntity<Workspace>,
    selection: Selection,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(ws) = ws.upgrade() {
        ws.update(cx, |ws, cx| ws.open(selection, preview, window, cx));
    }
}

/// Opens a PR's tab, or shows it. `preview`: a single click's, which the next one replaces.
pub fn open_pull(
    ws: &WeakEntity<Workspace>,
    item: PullItem,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        ws,
        Selection::Pull {
            pull: Arc::new(core_pull(&item)),
        },
        preview,
        window,
        cx,
    );
}

pub fn open_issue(
    ws: &WeakEntity<Workspace>,
    item: IssueItem,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        ws,
        Selection::Issue {
            issue: Arc::new(core_issue(&item)),
        },
        preview,
        window,
        cx,
    );
}

/// A PR file's diff over `range`; with the PR's url in it, the file takes line comments.
pub fn open_pull_file(
    ws: &WeakEntity<Workspace>,
    range: PullRange,
    file: FileChange,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        ws,
        Selection::PrFile {
            range,
            file: Arc::new(file),
        },
        preview,
        window,
        cx,
    );
}

/// Each project, as core opens it or switches it to another worktree: core goes back to gh's
/// active account then, so the project's own goes back on before anything reads, whichever
/// views (a panel, a restored tab) exist; another repository starts the cache over. A git
/// change (`gh pr create` or `gh pr merge` in a terminal) has the PR lists read again.
fn follow_projects(cx: &mut App) {
    cx.observe_new::<Project>(|_, _, cx| {
        cx.default_global::<Projects>().0 += 1;
        cx.on_release(|_, cx| cx.default_global::<Projects>().0 -= 1)
            .detach();
        cx.subscribe_self(|project: &mut Project, ev: &ProjectEvent, cx| {
            let root = project.root().to_path_buf();
            match ev {
                ProjectEvent::Opened | ProjectEvent::Switched { .. } => {}
                ProjectEvent::RepoChanged(change) => {
                    if change.git && Cache::repo_is(cx, &root) {
                        Cache::recheck_pulls_soon(cx);
                    }
                    return;
                }
                ProjectEvent::Failed(_) => return,
            }
            let main = project
                .main_worktree()
                .unwrap_or(&root)
                .to_string_lossy()
                .into_owned();
            if let Err(e) = crate::account::apply_project_account(&main, cx) {
                let project = cx.weak_entity();
                cx.defer(move |cx| account_not_applied(project, e, cx));
            }
            Cache::use_repo(cx, &root);
            crate::account::load(cx, false);
        })
        .detach();
    })
    .detach();
}

/// The GitHub account picked for a project can't be used (core then reads as gh's active one):
/// the project's windows say so rather than act as someone else unasked.
fn account_not_applied(project: WeakEntity<Project>, error: String, cx: &mut App) {
    for window in cx.windows() {
        let _ = window.update(cx, |_, window, cx| {
            let ws = Workspace::for_window(window, cx)?;
            if ws.read(cx).project().map(|p| p.downgrade()) != Some(project.clone()) {
                return None;
            }
            let toast = Toast::error(format!(
                "{error}. The GitHub CLI's active account is used instead."
            ))
            .title("Couldn't use this project's GitHub account");
            ws.update(cx, |ws, cx| ws.toast(toast, cx));
            Some(())
        });
    }
}

/// What the branch lists were last read for: the cache's repo (its generation), its wakes, and
/// a fork's original. None until the account says origin is on GitHub.
#[derive(Default)]
struct BranchLists(Option<(u64, u64, u64, Option<String>)>);
impl Global for BranchLists {}

/// How many projects are open: with none, nothing shows a badge to read lists for.
#[derive(Default)]
struct Projects(usize);
impl Global for Projects {}

/// The branch and worktree badges' PR lists (`pulls_panel::load_branch_lists`) follow the
/// cache: read once the account is known, again at each wake (focus, the poll, a git change),
/// with no view of this crate open.
fn follow_branch_lists(cx: &mut App) {
    cx.observe_global::<Cache>(|cx| {
        let open = cx.try_global::<Projects>().is_some_and(|p| p.0 > 0);
        let account = crate::account::account(cx).filter(|a| a.origin.is_some());
        let now = account.filter(|_| open && Cache::has_repo(cx)).map(|a| {
            let w = Cache::wakes(cx);
            (Cache::generation(cx), w.all, w.pulls, a.upstream())
        });
        if cx.try_global::<BranchLists>().map(|l| &l.0) == Some(&now) {
            return;
        }
        let ask = now.is_some();
        cx.set_global(BranchLists(now));
        if ask {
            pulls_panel::load_branch_lists(cx);
        }
    })
    .detach();
}

/// The repository core has open, as a tab built before any panel (a restored one) asks. Not
/// the tab's workspace: an opener builds the tab while the workspace is being updated.
pub(crate) fn use_workspace_project(_: &WeakEntity<Workspace>, cx: &mut App) {
    if let Ok(root) = crate::core(cx).repo() {
        pulls_panel::use_project(&root, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use gitviber_core::watch::RepoChanged;
    use gitviber_ui::workspace::Project;
    use gpui_kit::{Context, Entity, IntoElement, Render, TestAppContext, VisualTestContext, div};

    use super::*;
    use crate::cache::MIN_AGE;
    use crate::logic::narrow::{Filter, Narrow};
    use crate::model::{Access, Account};
    use crate::store::Store;

    struct Host(Entity<Workspace>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// A window with `root` open as its project, which core then says opened: no PRs panel
    /// nor any other GitHub view.
    fn opened<'a>(
        root: &str,
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<Project>,
        &'a mut VisualTestContext,
    ) {
        crate::leak_tests::setup(cx);
        let (host, cx) = cx.add_window_view(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            Host(ws)
        });
        let ws = host.read_with(cx, |h, _| h.0.clone());
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from(root), window, cx)
            })
        });
        cx.run_until_parked();
        let project = ws
            .read_with(cx, |ws, _| ws.project().cloned())
            .expect("open");
        (ws, project, cx)
    }

    fn access(owner: &str) -> Access {
        Access {
            owner: owner.into(),
            name: "r".into(),
            default_branch: Some("main".into()),
            push: true,
            triage: true,
            admin: false,
            issues: true,
            allow_merge_commit: None,
            allow_squash_merge: None,
            allow_rebase_merge: None,
        }
    }

    #[gpui_kit::test]
    fn a_project_account_that_cant_be_used_says_so(cx: &mut TestAppContext) {
        let root = "/tmp/gitviber-github-account";
        let (ws, project, cx) = opened(root, cx);
        cx.update(|_, cx| Store::update(cx, |s| s.set_account(root, Some("not a login".into()))));
        project.update(cx, |_, cx| cx.emit(ProjectEvent::Opened));
        cx.run_until_parked();
        let toasts = ws.update(cx, |ws, _| ws.take_toasts());
        assert!(
            toasts.iter().any(|t| {
                t.title.as_deref() == Some("Couldn't use this project's GitHub account")
                    && t.message.contains("not a login")
            }),
            "{toasts:?}"
        );
    }

    #[gpui_kit::test]
    fn the_badges_lists_are_read_without_the_prs_panel(cx: &mut TestAppContext) {
        let (_ws, project, cx) = opened("/tmp/gitviber-github-badges", cx);
        project.update(cx, |_, cx| cx.emit(ProjectEvent::Opened));
        cx.run_until_parked();
        // The account says origin is a fork of up/r.
        cx.update(|_, cx| {
            Cache::load(cx, crate::account::KEY, Duration::ZERO, |_| {
                Ok(Account {
                    login: "me".into(),
                    source: "gh".into(),
                    origin: Some(access("me")),
                    parent: Some(access("up")),
                })
            })
        });
        cx.run_until_parked();
        // Open ones, any state (a squash-merged branch's), and the original's: each asked.
        cx.update(|_, cx| {
            for (name, filter) in [
                ("origin", Filter::Open),
                ("origin", Filter::All),
                ("up/r", Filter::All),
            ] {
                let key = pulls_panel::PullsPanel::list_key(name, filter, &Narrow::default(), 1);
                assert!(!Cache::is_stale(cx, &key, MIN_AGE), "{key} never asked");
            }
        });
    }

    #[gpui_kit::test]
    fn a_git_change_has_the_pr_lists_read_again_without_the_panel(cx: &mut TestAppContext) {
        let root = "/tmp/gitviber-github-recheck";
        let (_ws, project, cx) = opened(root, cx);
        project.update(cx, |_, cx| cx.emit(ProjectEvent::Opened));
        cx.run_until_parked();
        let pulls = |cx: &mut VisualTestContext| cx.update(|_, cx| Cache::wakes(cx).pulls);
        let before = pulls(cx);
        let change = |git: bool| {
            ProjectEvent::RepoChanged(RepoChanged {
                root: root.into(),
                worktree: !git,
                git,
            })
        };
        // `gh pr create` in a terminal: once things settle, the lists are read again.
        project.update(cx, |_, cx| cx.emit(change(true)));
        cx.executor().advance_clock(Duration::from_secs(4));
        cx.run_until_parked();
        assert_eq!(pulls(cx), before + 1);
        // A file saved isn't one.
        project.update(cx, |_, cx| cx.emit(change(false)));
        cx.executor().advance_clock(Duration::from_secs(30));
        cx.run_until_parked();
        assert_eq!(pulls(cx), before + 1);
    }
}
