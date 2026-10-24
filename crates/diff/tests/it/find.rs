//! ⌘F finds in every line of a diff, removed ones and folded ones too, as text or a regex; an
//! SVG's Preview draws both versions and gives them back when its diff closes.

use crate::review_fixes::{repo, setup, window_with};
use gitviber_diff::{Compare, DiffData, DiffSource, DiffView, LinePos, Side};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div,
};
use std::sync::Arc;

#[gpui::test]
fn find_goes_through_removed_and_folded_lines(cx: &mut TestAppContext) {
    // A change at each end; "needle" sits in the folded middle and in a removed line.
    let old: String = std::iter::once("needle old\n".to_string())
        .chain((2..=30).map(|i| {
            if i == 15 {
                "a Needle here\n".to_string()
            } else {
                format!("line {i}\n")
            }
        }))
        .collect();
    let new = old
        .replacen("needle old", "fresh", 1)
        .replace("line 30", "end");
    let dir = repo("find", &[("a.txt", &old, &new)]);
    setup(cx, &dir);
    cx.update(gitviber_diff::init);
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| DiffView::new(DiffSource::new(Compare::Unstaged, "a.txt"), cx))
    });
    cx.update(|window, cx| {
        let focus = gpui_kit::Focusable::focus_handle(view.read(cx), cx);
        window.focus(&focus, cx);
    });
    assert!(
        view.read_with(cx, |v, _| v.line_height_at(LinePos {
            side: Side::New,
            line: 15
        }))
        .is_none(),
        "line 15 starts folded"
    );
    cx.dispatch_action(gitviber_ui::keymap::editor::Find);
    cx.run_until_parked();
    cx.update(|window, cx| view.update(cx, |v, cx| v.set_find_query("needle", window, cx)));
    cx.run_until_parked();
    let (current, total, at) = view
        .read_with(cx, |v, _| v.find_state())
        .expect("the box is open");
    assert_eq!(
        (current, total),
        (0, 2),
        "the removed line's and the folded one's"
    );
    assert_eq!(
        at,
        Some(LinePos {
            side: Side::Old,
            line: 1
        })
    );
    cx.update(|_, cx| view.update(cx, |v, cx| v.find_step(true, cx)));
    cx.run_until_parked();
    let (_, _, at) = view.read_with(cx, |v, _| v.find_state()).unwrap();
    assert_eq!(
        at,
        Some(LinePos {
            side: Side::New,
            line: 15
        })
    );
    assert!(
        view.read_with(cx, |v, _| v.line_height_at(LinePos {
            side: Side::New,
            line: 15
        }))
        .is_some(),
        "the match's fold opened"
    );
    // ⌥⌘C: only the lowercase one.
    cx.dispatch_action(gitviber_diff::actions::FindMatchCase);
    cx.run_until_parked();
    let (_, total, _) = view.read_with(cx, |v, _| v.find_state()).unwrap();
    assert_eq!(total, 1);
    // ⌥⌘R: the query is a regex, case still matched.
    cx.dispatch_action(gitviber_diff::actions::FindRegex);
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.set_find_query("[Nn]eedle (old|here)", window, cx)
        })
    });
    cx.run_until_parked();
    let (_, total, _) = view.read_with(cx, |v, _| v.find_state()).unwrap();
    assert_eq!(total, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

struct Host {
    view: Option<Entity<DiffView>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .children(self.view.clone())
    }
}

#[gpui::test]
fn an_svg_preview_draws_both_and_frees_them(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_diff::set_core(Arc::new(gitviber_core::Core::new(Arc::new(|_| {}))), cx);
    });
    let old = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><rect width="24" height="24" fill="#f00"/></svg>"##;
    let new = old.replace("#f00", "#00f");
    let source = DiffSource::new(Compare::Unstaged, "icon.svg");
    let window = cx.add_window(|_, _| Host { view: None });
    cx.run_until_parked();
    let snapshot = cx.update(|cx| cx.leak_detector_snapshot());
    for _ in 0..4 {
        window
            .update(cx, |host, _, cx| {
                let data = DiffData::from_texts(old, &new, &source);
                let source = source.clone();
                host.view = Some(cx.new(|cx| {
                    let mut v = DiffView::with_data(source, data, cx);
                    v.set_preview(true, cx);
                    v
                }));
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        let shown = window
            .update(cx, |host, _, cx| {
                host.view
                    .as_ref()
                    .is_some_and(|v| v.read(cx).is_previewing())
            })
            .unwrap();
        assert!(shown, "the preview replaced the lines");
        window
            .update(cx, |host, _, cx| {
                host.view = None;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
    }
    cx.update(|cx| cx.assert_no_new_leaks(&snapshot));
}
