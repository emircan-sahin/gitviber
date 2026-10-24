//! Opening and closing the GitHub views gives their memory back: PR, issue and PR file tabs,
//! the two panels, and the dialogs' entities, ten times over, leave no entity behind.

use std::path::PathBuf;
use std::sync::Arc;

use gitviber_core::git::FileChange;
use gitviber_ui::workspace::{PullRange, Selection, TabGroup, Workspace};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div,
};

use crate::model::{IssueItem, PullItem, PullState};

struct Host(Entity<Workspace>);

/// Draws what the app would: the PRs and Issues panels and the active tab.
impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let views: Vec<gpui_kit::AnyView> = self.0.update(cx, |ws, cx| {
            let mut v: Vec<gpui_kit::AnyView> = ["pulls", "issues"]
                .iter()
                .filter_map(|id| ws.panel(id, window, cx).map(|p| p.view()))
                .collect();
            v.extend(ws.active_view(window, cx));
            v
        });
        div().size_full().flex().children(
            views
                .into_iter()
                .map(|v| div().w(gpui_kit::px(400.)).h(gpui_kit::px(700.)).child(v)),
        )
    }
}

fn setup(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("gitviber-github-test-{}", std::process::id()));
    // SAFETY: the tests that read the config dir all point it at a temporary folder.
    unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
    cx.update(|cx| {
        gitviber_ui::init(cx);
        let core = gitviber_ui::install_core(cx);
        gitviber_diff::set_core(core, cx);
        gitviber_diff::init(cx);
        crate::init(cx);
    });
}

fn pull(n: u64) -> PullItem {
    PullItem {
        number: n,
        title: format!("PR {n}"),
        state: PullState::Open,
        draft: false,
        author: "me".into(),
        head_ref: "fix".into(),
        head_sha: "abc".into(),
        head_repo: Some("o/r".into()),
        base_ref: "main".into(),
        base_sha: "def".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        url: format!("https://github.com/o/r/pull/{n}"),
    }
}

fn issue(n: u64) -> IssueItem {
    IssueItem {
        number: n,
        title: format!("Issue {n}"),
        open: true,
        state_reason: None,
        author: "me".into(),
        labels: vec![],
        assignees: vec![],
        comments: 0,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        url: format!("https://github.com/o/r/issues/{n}"),
    }
}

fn file(path: &str) -> FileChange {
    FileChange {
        path: path.into(),
        old_path: None,
        status: "M".into(),
        additions: Some(1),
        deletions: Some(1),
        oid: None,
        index_oid: None,
        conflict: None,
        mode: None,
        submodule: None,
        nested: None,
    }
}

#[gpui_kit::test]
fn closed_tabs_and_panels_leave_nothing_behind(cx: &mut TestAppContext) {
    setup(cx);
    let (host, cx) = cx.add_window_view(|window, cx| {
        let ws = cx.new(Workspace::new);
        Workspace::register_window(&ws, window, cx);
        Host(ws)
    });
    let ws = host.read_with(cx, |h, _| h.0.clone());
    let round = |n: u64, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(
                    PathBuf::from(format!("/tmp/gitviber-github-leak-{}", n % 2)),
                    window,
                    cx,
                );
                assert!(ws.panel("pulls", window, cx).is_some());
                assert!(ws.panel("issues", window, cx).is_some());
                assert!(ws.open(
                    Selection::Pull {
                        pull: Arc::new(crate::ui::core_pull(&pull(n)))
                    },
                    false,
                    window,
                    cx
                ));
                assert!(ws.open(
                    Selection::Issue {
                        issue: Arc::new(crate::ui::core_issue(&issue(n)))
                    },
                    false,
                    window,
                    cx
                ));
                let range = PullRange {
                    number: Some(n),
                    pull_url: Some(pull(n).url),
                    label: None,
                    base: "def".into(),
                    head: "abc".into(),
                };
                assert!(ws.open(
                    Selection::PrFile {
                        range,
                        file: Arc::new(file("src/a.rs"))
                    },
                    true,
                    window,
                    cx
                ));
                assert_eq!(ws.tabs().len(), 3);
            })
        });
        cx.run_until_parked();
        // Each tab drawn once, as switching to it would.
        for key in [
            format!("pull:{}", pull(n).url),
            format!("issue:{}", issue(n).url),
        ] {
            cx.update(|window, cx| ws.update(cx, |ws, cx| ws.activate_tab(&key, window, cx)));
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        }
        cx.update(|_, cx| {
            ws.update(cx, |ws, cx| {
                ws.close_group(TabGroup::All, cx);
                ws.close_project(cx);
            })
        });
        // The last frame's element state holds what it drew until the next frame replaces it.
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    };
    // Once first: what's built a single time for the app (globals, the keymap) isn't a leak.
    round(0, cx);
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    for n in 1..=10 {
        round(n, cx);
    }
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    assert_eq!(
        gitviber_ui::memory::live_counts()
            .iter()
            .filter(|(k, _)| [
                "PullView",
                "IssueView",
                "PullFileView",
                "PullsPanel",
                "IssuesPanel"
            ]
            .contains(k))
            .count(),
        0
    );
}
