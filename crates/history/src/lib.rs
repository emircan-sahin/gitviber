//! GitViber's History: the commit graph with its search, menus and rewrites, a commit's tab with
//! its files and diff, and the Compare screen.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

mod act;
mod bisect;
pub mod commit;
mod commit_view;
mod compare;
mod dialogs;
mod diff_tab;
pub mod edits;
mod files;
mod fork;
pub mod graph;
pub mod list;
mod menu;
pub mod open;
pub mod panel;
mod picker;
pub mod query;
mod reflog;
mod store;

pub use bisect::BisectBar;
pub use commit_view::CommitView;
pub use compare::CompareView;
pub use fork::{Fork, ForkOf, fork_news, set_fork_of};
pub use panel::HistoryPanel;

use std::rc::Rc;

use gitviber_ui::keymap::{git as git_actions, history as history_actions};
use gitviber_ui::workspace::{PanelSpec, Slot, Workspace, register_panel};
use gpui_kit::{AnyElement, App, AppContext as _, Entity, Global, Window};

fn in_front(
    cx: &mut App,
    f: impl FnOnce(&Entity<Workspace>, &Entity<gitviber_refs::RepoState>, &mut Window, &mut App)
    + 'static,
) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            let Some(ws) = Workspace::for_window(window, cx) else {
                return;
            };
            let Some(project) = ws.read(cx).project().cloned() else {
                return;
            };
            let repo = gitviber_refs::RepoState::get(&project, cx);
            f(&ws, &repo, window, cx);
        });
    });
}

/// What GitHub shows of a commit's checks: a badge on its row (`header` false) or the verdict
/// with its list in a commit's tab. None for a commit without checks or an account to see them.
pub type CommitChecks =
    std::rc::Rc<dyn Fn(&str, bool, &mut Window, &mut App) -> Option<AnyElement>>;

#[derive(Default)]
struct Checks(Option<CommitChecks>);
impl Global for Checks {}

/// For the GitHub crate: where commits' checks come from.
pub fn set_commit_checks(cx: &mut App, checks: CommitChecks) {
    cx.set_global(Checks(Some(checks)));
}

pub(crate) fn commit_checks(
    sha: &str,
    header: bool,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let checks = cx.try_global::<Checks>()?.0.clone()?;
    checks(sha, header, window, cx)
}

/// A row's badge for a commit's checks in a repository other than origin, by owner/name first:
/// a fork's original. Asked with the other rows drawn, as origin's are.
pub type RepoChecks = std::rc::Rc<dyn Fn(&str, &str, &mut Window, &mut App) -> Option<AnyElement>>;

#[derive(Default)]
struct OtherChecks(Option<RepoChecks>);
impl Global for OtherChecks {}

/// For the GitHub crate: where another repository's checks come from.
pub fn set_repo_checks(cx: &mut App, checks: RepoChecks) {
    cx.set_global(OtherChecks(Some(checks)));
}

pub(crate) fn repo_checks(
    repo: &str,
    sha: &str,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let checks = cx.try_global::<OtherChecks>()?.0.clone()?;
    checks(repo, sha, window, cx)
}

/// Whether History lists every branch: one choice, for every repo.
pub fn set_all_branches(on: bool, cx: &mut App) {
    store::update(cx, |s| s.all_branches = on);
}

/// The History panel's id, which replaces the app's placeholder.
pub const PANEL: &str = "history";

/// Registers the History panel, the openers of a commit's, a range's and the Compare screen's
/// tabs, and the Compare screen's opener for the pickers. Once at startup.
pub fn init(cx: &mut App) {
    store::import_legacy(cx);
    register_panel(
        cx,
        PanelSpec::new(PANEL, Slot::Left, "History", |ctx, window, cx| {
            let project = ctx
                .project
                .clone()
                .expect("a per-project panel has its project");
            cx.new(|cx| HistoryPanel::new(ctx, project, window, cx))
        })
        .order(1),
    );
    open::register(cx);
    // From anywhere in a window: no element of ours is an ancestor of every panel. Each acts on
    // the window in front once the key dispatch is over, as the window is busy during it.
    cx.on_action(|_: &git_actions::CompareBranches, cx| {
        in_front(cx, |ws, repo, window, cx| {
            open::open_compare(&ws.downgrade(), repo, None, None, window, cx)
        })
    });
    cx.on_action(|_: &history_actions::Search, cx| {
        in_front(cx, |ws, _, window, cx| {
            let panel = ws.update(cx, |ws, cx| {
                ws.show_panel(PANEL, cx);
                ws.panel(PANEL, window, cx)
            });
            if let Some(p) = panel.and_then(|p| p.view().downcast::<HistoryPanel>().ok()) {
                p.update(cx, |p, cx| p.focus_search(window, cx));
            }
        })
    });
    // A commit's link (a terminal's SHA): History shows it, selected.
    gitviber_refs::hooks::set_show_commit(
        cx,
        Rc::new(|sha, window, cx| {
            let Some(ws) = Workspace::for_window(window, cx) else {
                return;
            };
            let panel = ws.update(cx, |ws, cx| {
                ws.show_panel(PANEL, cx);
                ws.panel(PANEL, window, cx)
            });
            if let Some(p) = panel.and_then(|p| p.view().downcast::<HistoryPanel>().ok()) {
                p.update(cx, |p, cx| p.jump_to(sha, window, cx));
            }
        }),
    );
    gitviber_refs::hooks::set_open_compare(
        cx,
        Rc::new(|base, head, window, cx| {
            let Some(ws) = Workspace::for_window(window, cx) else {
                return;
            };
            let Some(project) = ws.read(cx).project().cloned() else {
                return;
            };
            let repo = gitviber_refs::RepoState::get(&project, cx);
            open::open_compare(&ws.downgrade(), &repo, base, head, window, cx);
        }),
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gitviber_ui::workspace::{PanelContext, TabGroup, Workspace};
    use gpui_kit::{
        AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, TestAppContext,
        Window, div,
    };

    use super::HistoryPanel;

    struct Host(Entity<Workspace>);
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// This repository: History only reads it.
    fn repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    #[gpui_kit::test]
    fn panels_and_commit_tabs_give_their_entities_back(cx: &mut TestAppContext) {
        let dir =
            std::env::temp_dir().join(format!("gitviber-history-test-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(|cx| {
            gitviber_ui::init(cx);
            let core = gitviber_ui::install_core(cx);
            gitviber_diff::init(cx);
            gitviber_diff::set_core(core, cx);
            gitviber_refs::init(cx);
            super::init(cx);
        });
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        cx.update(|window, cx| {
            Workspace::register_window(&ws, window, cx);
            ws.update(cx, |ws, cx| ws.open_project(repo(), window, cx));
        });
        cx.run_until_parked();
        let project = cx
            .update(|_, cx| ws.read(cx).project().cloned())
            .expect("the project opened");
        let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
        for _ in 0..3 {
            let panel = cx.update(|window, cx| {
                let ctx = PanelContext {
                    workspace: ws.downgrade(),
                    project: Some(project.clone()),
                };
                cx.new(|cx| HistoryPanel::new(&ctx, project.clone(), window, cx))
            });
            cx.run_until_parked();
            assert!(panel.read_with(cx, |p, cx| p.loaded(cx)) > 0);
            // A click on the first commit: its files under it, its first file's tab.
            panel.update_in(cx, |p, window, cx| {
                p.open_first(window, cx);
                p.open_compare(window, cx);
            });
            cx.run_until_parked();
            assert!(ws.read_with(cx, |ws, _| ws.tabs().len()) >= 2);
            ws.update(cx, |ws, cx| ws.close_group(TabGroup::All, cx));
            let weak = panel.downgrade();
            drop(panel);
            cx.run_until_parked();
            assert!(weak.upgrade().is_none(), "the panel is released");
        }
        // A released entity is dropped at the next effect flush.
        cx.update(|_, _| {});
        cx.run_until_parked();
        cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    }

    #[gpui_kit::test]
    fn the_list_acts_on_the_head_it_read_and_ignores_rows_gone_since(cx: &mut TestAppContext) {
        let dir =
            std::env::temp_dir().join(format!("gitviber-history-test-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(|cx| {
            gitviber_ui::init(cx);
            let core = gitviber_ui::install_core(cx);
            gitviber_diff::init(cx);
            gitviber_diff::set_core(core, cx);
            gitviber_refs::init(cx);
            super::init(cx);
        });
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        cx.update(|window, cx| {
            Workspace::register_window(&ws, window, cx);
            ws.update(cx, |ws, cx| ws.open_project(repo(), window, cx));
        });
        cx.run_until_parked();
        let project = cx
            .update(|_, cx| ws.read(cx).project().cloned())
            .expect("the project opened");
        let panel = cx.update(|window, cx| {
            let ctx = PanelContext {
                workspace: ws.downgrade(),
                project: Some(project.clone()),
            };
            cx.new(|cx| HistoryPanel::new(&ctx, project.clone(), window, cx))
        });
        cx.run_until_parked();
        let list = panel.read_with(cx, |p, _| p.list().clone());
        let head = cx.update(|_, cx| {
            let core = gitviber_ui::core(cx);
            gitviber_core::api::history::log(&core, None, 0, 1, None, None)
                .unwrap()
                .remove(0)
                .sha
        });
        let first = list.read_with(cx, |l, cx| (l.head(cx), l.commits[0].sha.clone()));
        assert_eq!(first, (head.clone(), head.clone()));
        // A row far down takes the cursor, before searches list fewer.
        let far = list.read_with(cx, |l, _| l.commits[150].sha.clone());
        list.update_in(cx, |l, window, cx| {
            l.click_commit(&far, &gpui_kit::ClickEvent::default(), window, cx)
        });
        // A search that lists only older commits still acts on HEAD, read with it.
        panel.update_in(cx, |p, window, cx| {
            p.search_for("path:Cargo.toml".into(), window, cx)
        });
        cx.run_until_parked();
        assert_eq!(list.read_with(cx, |l, cx| l.head(cx)), head);
        // A row far down holds the cursor; then a search lists a few: clicks and keys on rows
        // drawn before find what's there now, or nothing.
        panel.update_in(cx, |p, window, cx| {
            p.search_for("path:crates/history/Cargo.toml".into(), window, cx)
        });
        cx.run_until_parked();
        let n = list.read_with(cx, |l, _| l.len());
        assert!(n > 0 && n < 150, "{n}");
        list.update_in(cx, |l, window, cx| {
            l.click_commit(&far, &gpui_kit::ClickEvent::default(), window, cx);
            let down = gpui_kit::KeyDownEvent {
                keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
                is_held: false,
                prefer_character_input: false,
            };
            l.key_down(&down, window, cx);
        });
    }

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        // Not the owner's config: no signing, no hooks.
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[gpui_kit::test]
    fn a_fork_shows_the_originals_history_with_what_your_branch_lacks(cx: &mut TestAppContext) {
        let config =
            std::env::temp_dir().join(format!("gitviber-history-test-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &config) };
        // Your branch has one commit; the original's main, fetched before, one more.
        let dir =
            std::env::temp_dir().join(format!("gitviber-history-fork-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let two = git(
            &dir,
            &["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "two"],
        );
        git(&dir, &["update-ref", "refs/remotes/upstream/main", &two]);
        git(
            &dir,
            &[
                "remote",
                "add",
                "upstream",
                "https://github.com/someone/original.git",
            ],
        );
        let fork = super::Fork {
            original: "someone/original".into(),
            branch: Some("main".into()),
            fork_branch: Some("main".into()),
            can_sync: true,
        };
        let forked = std::rc::Rc::new(std::cell::Cell::new(true));
        let checked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, String)>::new()));
        cx.update(|cx| {
            gitviber_ui::init(cx);
            let core = gitviber_ui::install_core(cx);
            gitviber_diff::init(cx);
            gitviber_diff::set_core(core, cx);
            gitviber_refs::init(cx);
            super::init(cx);
            super::set_all_branches(false, cx);
            let on = forked.clone();
            super::set_fork_of(
                cx,
                std::rc::Rc::new(move |_| on.get().then(|| fork.clone())),
            );
            let seen = checked.clone();
            super::set_repo_checks(
                cx,
                std::rc::Rc::new(move |repo, sha, _, _| {
                    seen.borrow_mut().push((repo.into(), sha.into()));
                    None
                }),
            );
            // Fetched a moment ago: no network round trip in a test.
            cx.default_global::<super::fork::Fetched>()
                .0
                .insert("someone/original".into(), std::time::Instant::now());
        });
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        cx.update(|window, cx| {
            Workspace::register_window(&ws, window, cx);
            ws.update(cx, |ws, cx| ws.open_project(dir.clone(), window, cx));
        });
        cx.run_until_parked();
        let project = cx
            .update(|_, cx| ws.read(cx).project().cloned())
            .expect("the project opened");
        let panel = cx.update(|window, cx| {
            let ctx = PanelContext {
                workspace: ws.downgrade(),
                project: Some(project.clone()),
            };
            cx.new(|cx| HistoryPanel::new(&ctx, project.clone(), window, cx))
        });
        // The original's pane comes with the first look at History.
        let draw = |cx: &mut gpui_kit::VisualTestContext| {
            let panel = panel.clone();
            cx.draw(
                gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(0.)),
                gpui_kit::size(gpui_kit::px(400.), gpui_kit::px(800.)),
                move |_, _| div().child(panel),
            );
        };
        draw(cx);
        cx.run_until_parked();
        let pane = panel
            .read_with(cx, |p, _| p.original().cloned())
            .expect("a fork shows the original");
        assert_eq!(
            pane.read_with(cx, |p, _| p.remote().map(|r| r.map(str::to_string))),
            Some(Some("upstream".into()))
        );
        let list = pane
            .read_with(cx, |p, _| p.list().cloned())
            .expect("its history");
        let marks: Vec<bool> =
            list.read_with(cx, |l, _| l.commits.iter().map(|c| c.not_in_head).collect());
        assert_eq!(marks, [true, false], "two is the original's only");
        // Its rows show the original's checks.
        draw(cx);
        assert!(
            checked
                .borrow()
                .contains(&("someone/original".into(), two.clone()))
        );
        // The divider's place is kept, saved once it rests.
        panel.update(cx, |p, cx| p.set_split(Some(0.7), cx));
        draw(cx);
        assert_eq!(cx.update(|_, cx| super::store::get(cx).fork_split), None);
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| super::store::get(cx).fork_split),
            Some(0.7)
        );
        // No longer a fork: the pane goes, its entities with it.
        let weak = pane.downgrade();
        drop((pane, list));
        forked.set(false);
        cx.update(|_, cx| super::fork_news(cx));
        cx.run_until_parked();
        assert!(panel.read_with(cx, |p, _| p.original().is_none()));
        cx.update(|_, _| {});
        cx.run_until_parked();
        assert!(weak.upgrade().is_none(), "the original's pane is released");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_commits_link_selects_it_in_history_reading_pages_until_its_there(cx: &mut TestAppContext) {
        let dir =
            std::env::temp_dir().join(format!("gitviber-history-test-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(|cx| {
            gitviber_ui::init(cx);
            let core = gitviber_ui::install_core(cx);
            gitviber_diff::init(cx);
            gitviber_diff::set_core(core, cx);
            gitviber_refs::init(cx);
            super::init(cx);
            super::set_all_branches(false, cx);
        });
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        cx.update(|window, cx| {
            Workspace::register_window(&ws, window, cx);
            ws.update(cx, |ws, cx| ws.open_project(repo(), window, cx));
        });
        cx.run_until_parked();
        // Past the first page, by a short id as a terminal prints it.
        let far = cx.update(|_, cx| {
            let core = gitviber_ui::core(cx);
            gitviber_core::api::history::log(&core, None, 450, 1, None, None)
                .unwrap()
                .remove(0)
                .sha
        });
        let shown = cx.update(|window, cx| {
            gitviber_refs::hooks::show_commit(far[..9].to_string(), window, cx)
        });
        assert!(shown);
        cx.run_until_parked();
        let panel = cx
            .update(|_, cx| ws.read(cx).built_panel(super::PANEL).cloned())
            .and_then(|p| p.view().downcast::<HistoryPanel>().ok())
            .expect("History shows");
        let list = panel.read_with(cx, |p, _| p.list().clone());
        let (len, open) = list.read_with(cx, |l, _| (l.len(), l.open_sha().map(str::to_string)));
        assert!(len > 450, "{len}");
        assert_eq!(open.as_deref(), Some(far.as_str()));
        // One nowhere in it: History reads on, then says so and stops.
        cx.update(|window, cx| gitviber_refs::hooks::show_commit("0".repeat(40), window, cx));
        cx.run_until_parked();
        let toasts = ws.update(cx, |ws, _| ws.take_toasts());
        assert!(toasts.iter().any(|t| {
            t.title
                .as_ref()
                .is_some_and(|t| t.contains("isn't in the history"))
        }));
    }
}
