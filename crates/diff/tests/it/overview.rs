//! The overview beside a diff's lines marks the whole file's changes and find's matches from the
//! first frame, follows find and folds, and scrolls where it's clicked or dragged.

use crate::review_fixes::window_with;
use gitviber_diff::{Compare, DiffData, DiffOptions, DiffSource, DiffStyle, DiffView};
use gpui_kit::{
    AppContext as _, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px,
};
use std::sync::Arc;

const REMOVED: usize = 0;
const ADDED: usize = 1;
const FOUND_OLD: usize = 2;
const FOUND_NEW: usize = 3;

fn open<'a>(
    cx: &'a mut TestAppContext,
    old: &str,
    new: &str,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_ui::test_support::install_theme(cx);
        gitviber_diff::set_core(Arc::new(gitviber_core::Core::new(Arc::new(|_| {}))), cx);
    });
    let source = DiffSource::new(Compare::Unstaged, "a.txt");
    let data = DiffData::from_texts(old, new, &source);
    window_with(cx, |_, cx| {
        cx.new(|cx| DiffView::with_data(source, data, cx))
    })
}

#[gpui::test]
fn the_overview_marks_the_whole_file_and_follows_find_and_folds(cx: &mut TestAppContext) {
    // A change at each end; "needle" on lines 8 and 14, unchanged, in the fold between them.
    let old: String = (1..=21)
        .map(|i| match i {
            1 => "one\n".to_string(),
            21 => "end\n".to_string(),
            8 | 14 => format!("needle {i}\n"),
            _ => format!("line {i}\n"),
        })
        .collect();
    let new = old
        .replacen("one\n", "ONE\n", 1)
        .replacen("end\n", "END\n", 1);
    let (view, cx) = open(cx, &old, &new);
    let l = cx.update(|_, cx| DiffStyle::get(cx).line_px());
    let spans = |cx: &mut VisualTestContext| view.update(cx, |v, _| v.overview_spans());
    // Folded: the first change, three lines of context, the fold, three more, the last change.
    let s = spans(cx);
    assert_eq!(s[REMOVED], vec![(px(0.), l), (l * 9., l * 10.)]);
    assert_eq!(s[ADDED], vec![(l, l * 2.), (l * 10., l * 11.)]);
    assert!(s[FOUND_OLD].is_empty() && s[FOUND_NEW].is_empty());

    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.open_find(window, cx);
            v.set_find_query("needle", window, cx);
        })
    });
    cx.run_until_parked();
    // The first match's fold opened around it; the second is still folded, marked on its fold.
    // Unchanged lines: on both halves.
    let s = spans(cx);
    assert_eq!(s[FOUND_OLD], vec![(l * 6., l * 8.)]);
    assert_eq!(s[FOUND_NEW], s[FOUND_OLD]);
    assert_eq!(s[REMOVED], vec![(px(0.), l), (l * 11., l * 12.)]);

    // The whole file: every line where it is.
    cx.update(|_, cx| DiffOptions::update(cx, |o| o.collapse = false));
    cx.run_until_parked();
    let s = spans(cx);
    assert_eq!(s[REMOVED], vec![(px(0.), l), (l * 21., l * 22.)]);
    assert_eq!(s[ADDED], vec![(l, l * 2.), (l * 22., l * 23.)]);
    assert_eq!(s[FOUND_NEW], vec![(l * 8., l * 9.), (l * 14., l * 15.)]);

    cx.update(|window, cx| view.update(cx, |v, cx| v.set_find_query("", window, cx)));
    let s = spans(cx);
    assert!(s[FOUND_OLD].is_empty() && s[FOUND_NEW].is_empty());
}

#[gpui::test]
fn a_click_on_the_overview_scrolls_there_and_a_drag_follows(cx: &mut TestAppContext) {
    cx.update(|cx| {
        DiffOptions::set(
            DiffOptions {
                collapse: false,
                ..DiffOptions::default()
            },
            cx,
        )
    });
    let old: String = (1..=2000).map(|i| format!("line {i}\n")).collect();
    let new = old.replace("line 1000\n", "changed\n");
    let (view, cx) = open(cx, &old, &new);
    let size = cx.update(|window, _| window.viewport_size());
    let (x, bottom) = (size.width - px(7.), size.height - px(4.));
    let no = Modifiers::none();
    cx.simulate_mouse_down(point(x, bottom), MouseButton::Left, no);
    cx.simulate_mouse_up(point(x, bottom), MouseButton::Left, no);
    cx.run_until_parked();
    let l = cx.update(|_, cx| DiffStyle::get(cx).line_px());
    let top = view.read_with(cx, |v, _| v.scroll_top());
    assert!(top > l * 1800., "scrolled to the end: {top:?}");
    // Its thumb is at the bottom now: dragged to the top, the lines follow.
    cx.simulate_mouse_down(point(x, bottom), MouseButton::Left, no);
    cx.simulate_mouse_move(point(x, px(0.)), MouseButton::Left, no);
    cx.simulate_mouse_up(point(x, px(0.)), MouseButton::Left, no);
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.scroll_top()), px(0.));
}
