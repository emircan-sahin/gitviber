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
pub mod graph;
pub mod list;
mod menu;
pub mod open;
pub mod panel;
mod picker;
pub mod query;
mod reflog;
mod store;

pub use commit_view::CommitView;
pub use compare::CompareView;
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
        AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, Window, div,
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
}
