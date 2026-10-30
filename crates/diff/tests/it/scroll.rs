//! A focused diff scrolls with the old diff's keys (↑/↓, PageUp/PageDown, Space/⇧Space, Home/End
//! and ⌘↑/⌘↓, ←/→), unified or split and as one scroll of files, and never takes them from a
//! field being typed into.

use crate::review_fixes::window_with;
use gitviber_diff::{
    Compare, DiffData, DiffOptions, DiffSource, DiffStyle, DiffView, Layout, StackedDiff,
    StackedFile,
};
use gpui_kit::{
    AppContext as _, Entity, Focusable, Modifiers, Pixels, TestAppContext, VisualTestContext,
    point, px,
};
use std::sync::Arc;

/// To the top and the bottom besides Home and End.
const ENDS: (&str, &str) = if cfg!(target_os = "macos") {
    ("cmd-up", "cmd-down")
} else {
    ("ctrl-home", "ctrl-end")
};

fn setup(cx: &mut TestAppContext, layout: Layout) {
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_ui::test_support::install_theme(cx);
        // The keys as the app binds them.
        gitviber_diff::actions::init_local(cx);
        gitviber_diff::set_core(Arc::new(gitviber_core::Core::new(Arc::new(|_| {}))), cx);
        DiffOptions::set(
            DiffOptions {
                collapse: false,
                layout,
                ..DiffOptions::default()
            },
            cx,
        );
    });
}

/// `lines` lines: one changed, one far wider than the window.
fn data(lines: usize) -> (DiffSource, DiffData) {
    let old: String = (1..=lines)
        .map(|i| match i {
            5 => format!("{}\n", "wide ".repeat(400)),
            _ => format!("line {i}\n"),
        })
        .collect();
    let new = old.replace("line 500\n", "changed\n");
    let source = DiffSource::new(Compare::Unstaged, "a.txt");
    let data = DiffData::from_texts(&old, &new, &source);
    (source, data)
}

fn focus<V: Focusable>(view: &Entity<V>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let focus = view.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
}

#[track_caller]
fn assert_near(a: Pixels, b: Pixels) {
    assert!((a - b).abs() < px(0.01), "{a:?} != {b:?}");
}

fn open_diff(
    cx: &mut TestAppContext,
    layout: Layout,
) -> (Entity<DiffView>, &mut VisualTestContext) {
    setup(cx, layout);
    let (source, data) = data(2000);
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| DiffView::with_data(source, data, cx))
    });
    focus(&view, cx);
    (view, cx)
}

#[gpui::test]
fn keys_scroll_a_unified_diff(cx: &mut TestAppContext) {
    keys_scroll_a_diff(cx, Layout::Unified);
}

#[gpui::test]
fn keys_scroll_a_split_diff(cx: &mut TestAppContext) {
    keys_scroll_a_diff(cx, Layout::Split);
}

fn keys_scroll_a_diff(cx: &mut TestAppContext, layout: Layout) {
    let (view, cx) = open_diff(cx, layout);
    let l = cx.update(|_, cx| DiffStyle::get(cx).line_px());
    let height = cx.update(|window, _| window.viewport_size().height);
    let press = |keys: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(keys);
        view.read_with(cx, |v, _| v.scroll_top())
    };
    assert_eq!(press("up", cx), px(0.), "already at the top");
    assert_near(press("down", cx), l);
    assert_near(press("down down", cx), l * 3.);
    assert_near(press("up", cx), l * 2.);
    assert_eq!(press("home", cx), px(0.));
    // A page is the screen less a line kept in view.
    let page = press("pagedown", cx);
    assert!(page > l * 10. && page <= height - l, "{page:?}");
    assert_near(press("space", cx), page * 2.);
    assert_near(press("shift-space", cx), page);
    assert_eq!(press("pageup", cx), px(0.));
    let end = press("end", cx);
    assert!(end > l * 1900., "{end:?}");
    assert_eq!(
        press("down pagedown space", cx),
        end,
        "nothing past the end"
    );
    assert_eq!(press(ENDS.0, cx), px(0.));
    assert_eq!(press(ENDS.1, cx), end);
    // Sideways, for the wide line.
    let left = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.scroll_left());
    cx.simulate_keystrokes("right right");
    assert_eq!(left(cx), px(80.));
    cx.simulate_keystrokes("left");
    assert_eq!(left(cx), px(40.));
}

#[gpui::test]
fn the_find_box_and_the_editor_keep_their_keys(cx: &mut TestAppContext) {
    let (view, cx) = open_diff(cx, Layout::Unified);
    let top = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.scroll_top());
    cx.dispatch_action(gitviber_ui::keymap::editor::Find);
    cx.simulate_keystrokes("space");
    let (_, found, _) = view
        .read_with(cx, |v, _| v.find_state())
        .expect("the box is open");
    assert!(found > 0, "Space typed a space");
    let before = top(cx);
    cx.simulate_keystrokes("pagedown down end");
    assert_eq!(top(cx), before, "the box's keys");
    // Out of the box, on the lines again.
    cx.simulate_keystrokes("escape pagedown");
    assert!(top(cx) > before);

    cx.update(|window, cx| view.update(cx, |v, cx| v.toggle_edit(window, cx)));
    cx.run_until_parked();
    let size = cx.update(|window, _| window.viewport_size());
    cx.simulate_click(point(size.width / 2., size.height / 2.), Modifiers::none());
    cx.simulate_keystrokes("pagedown space");
    assert!(
        view.read_with(cx, |v, cx| v.has_unsaved_edits(cx)),
        "Space typed into the editor"
    );
}

#[gpui::test]
fn keys_scroll_a_list_of_files(cx: &mut TestAppContext) {
    setup(cx, Layout::Unified);
    // Short of what a list holds back as "A large diff".
    let (source, data) = data(1000);
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| {
            let mut s = StackedDiff::new(vec![StackedFile::new(source)], cx);
            s.preload(0, data, false, cx);
            s
        })
    });
    focus(&view, cx);
    let l = cx.update(|_, cx| DiffStyle::get(cx).line_px());
    let press = |keys: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(keys);
        view.read_with(cx, |v, _| v.scroll_top())
    };
    assert_near(press("down", cx), l);
    assert_eq!(press("up", cx), px(0.));
    let page = press("pagedown", cx);
    assert!(page > l * 10., "{page:?}");
    assert_near(press("space", cx), page * 2.);
    assert_near(press("shift-space pageup", cx), px(0.));
    let end = press(ENDS.1, cx);
    assert!(end > l * 900., "{end:?}");
    assert_eq!(press("down", cx), end);
    assert_eq!(press("home", cx), px(0.));
}
