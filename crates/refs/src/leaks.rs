//! Opening and closing the pickers, over a project that comes and goes, gives every entity back.

use gitviber_ui::Project;
use gpui_kit::{AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div};

use crate::RepoState;
use crate::branches::BranchPicker;
use crate::worktrees::WorktreePicker;

struct Host;
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui_kit::test]
fn pickers_and_repo_state_give_their_entities_back(cx: &mut TestAppContext) {
    let dir = gitviber_ui::test_support::config_dir("refs-leaks");
    cx.update(|cx| {
        gitviber_ui::init(cx);
        gitviber_ui::install_core(cx);
    });
    let (_, cx) = cx.add_window_view(|_, _| Host);
    cx.run_until_parked();
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    for _ in 0..4 {
        // Not a repository: opening fails, and the views still come and go whole.
        cx.update(|window, cx| {
            let project = cx.new(|cx| Project::new(dir.clone(), cx));
            let state = RepoState::get(&project, cx);
            let branches = cx.new(|cx| BranchPicker::new(state.clone(), None, window, cx));
            let worktrees = cx.new(|cx| WorktreePicker::new(state.clone(), None, window, cx));
            drop((branches, worktrees, state, project));
        });
        cx.run_until_parked();
    }
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
}
