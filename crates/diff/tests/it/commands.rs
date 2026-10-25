//! The app's own actions, named with `answer`, reach a focused diff whatever view shows it; what
//! a single file's diff can't do goes on to the view around it.

use crate::review_fixes::{repo, setup, window_with};
use gitviber_diff::{Command, Compare, DiffSource, DiffView, answer};
use gpui_kit::{
    AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, Window, div,
};

gpui_kit::actions!(app_keys, [Next, NextFile]);

struct Host {
    diff: Entity<DiffView>,
    walked: usize,
    focus: FocusHandle,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .size_full()
            .child(self.diff.clone())
            .on_action(cx.listener(|this, _: &NextFile, _, _| this.walked += 1))
    }
}

#[gpui::test]
fn the_apps_keys_reach_the_focused_diff_and_the_rest_go_on(cx: &mut TestAppContext) {
    let dir = repo("commands", &[("a.txt", "one\ntwo\n", "ONE\ntwo\n")]);
    setup(cx, &dir);
    cx.update(|cx| {
        answer(
            cx,
            vec![
                (Box::new(Next), Command::NextChange),
                (Box::new(NextFile), Command::NextFile),
            ],
        )
    });
    let (host, cx) = window_with(cx, |_, cx| {
        let diff = cx.new(|cx| DiffView::new(DiffSource::new(Compare::Unstaged, "a.txt"), cx));
        cx.new(|cx| Host {
            diff,
            walked: 0,
            focus: cx.focus_handle(),
        })
    });
    let diff = host.read_with(cx, |h, _| h.diff.clone());
    cx.update(|window, cx| {
        let focus = gpui_kit::Focusable::focus_handle(diff.read(cx), cx);
        window.focus(&focus, cx);
    });
    cx.dispatch_action(Next);
    cx.run_until_parked();
    assert!(
        diff.read_with(cx, |d, _| d.selection()).is_some(),
        "the diff went to its change"
    );
    cx.dispatch_action(NextFile);
    cx.run_until_parked();
    assert_eq!(
        host.read_with(cx, |h, _| h.walked),
        1,
        "J went on to the host"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
