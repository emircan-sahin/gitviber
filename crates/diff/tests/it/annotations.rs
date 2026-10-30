//! Blocks between lines: they open the fold they'd be hidden in, take the room they declare and
//! then the room they measure, and go with their view. In Open All, one on a line the diff
//! doesn't draw shows above it.

use gitviber_diff::{
    Annotation, Compare, DiffData, DiffOptions, DiffSource, DiffView, LinePos, Side,
};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div, px,
};
use std::sync::Arc;

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
fn a_block_opens_its_fold_and_takes_the_room_it_measures(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_ui::test_support::install_theme(cx);
        gitviber_diff::set_core(Arc::new(gitviber_core::Core::new(Arc::new(|_| {}))), cx);
        DiffOptions::update(cx, |o| o.collapse = true);
    });
    let old: String = (1..=100).map(|i| format!("line {i}\n")).collect();
    let new = old.replace("line 50\n", "line fifty\n");
    let source = DiffSource::new(Compare::Unstaged, "a.txt");
    let window = cx.add_window(|_, _| Host { view: None });
    let snapshot = cx.update(|cx| cx.leak_detector_snapshot());
    let view = cx.update(|cx| {
        cx.new(|cx| {
            DiffView::with_data(
                source.clone(),
                DiffData::from_texts(&old, &new, &source),
                cx,
            )
        })
    });
    let at = LinePos {
        side: Side::New,
        line: 10,
    };
    let line = view.read_with(cx, |v, _| {
        v.line_height_at(LinePos {
            side: Side::New,
            line: 50,
        })
        .unwrap()
    });
    // Line 10 is far from the change: folded.
    assert_eq!(view.read_with(cx, |v, _| v.line_height_at(at)), None);
    view.update(cx, |v, cx| {
        v.set_annotations(
            vec![Annotation::new("thread-1", at, px(40.), |_, _| {
                div().h(px(100.)).into_any_element()
            })],
            cx,
        );
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.line_height_at(at)),
        Some(line + px(40.)),
        "declared before it's drawn"
    );
    window
        .update(cx, |host, _, cx| {
            host.view = Some(view.clone());
            cx.notify();
        })
        .unwrap();
    // Drawn near the top of the view, it measures what it is.
    cx.run_until_parked();
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.line_height_at(at)),
        Some(line + px(100.)),
        "as measured once drawn"
    );
    view.update(cx, |v, cx| v.set_annotations(vec![], cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.line_height_at(at)),
        None,
        "folded again"
    );
    window
        .update(cx, |host, _, cx| {
            host.view = None;
            cx.notify();
        })
        .unwrap();
    drop(view);
    cx.run_until_parked();
    cx.update(|cx| cx.assert_no_new_leaks(&snapshot));
}

#[gpui::test]
fn open_all_shows_blocks_on_lines_it_doesnt_draw_above_the_diff(cx: &mut TestAppContext) {
    use crate::review_fixes::{repo, setup, window_with};
    use gitviber_diff::{StackedDiff, StackedFile};
    use std::cell::Cell;
    use std::rc::Rc;
    // An image (drawn as a message, no lines) and a text file of three lines.
    let dir = repo(
        "loose",
        &[
            ("a.png", "\u{0}one", "\u{0}two"),
            ("b.txt", "one\ntwo\nthree\n", "one\nTWO\nthree\n"),
        ],
    );
    setup(cx, &dir);
    cx.update(gitviber_diff::init);
    let sources = ["a.png", "b.txt"].map(|p| DiffSource::new(Compare::Unstaged, p));
    let files: Vec<StackedFile> = sources.iter().cloned().map(StackedFile::new).collect();
    let (view, cx) = window_with(cx, |_, cx| cx.new(|cx| StackedDiff::new(files, cx)));
    let drawn: [Rc<Cell<bool>>; 3] = Default::default();
    let block = |id: &str, line: u32, flag: &Rc<Cell<bool>>| {
        let flag = flag.clone();
        Annotation::new(
            id.to_string(),
            LinePos {
                side: Side::New,
                line,
            },
            px(30.),
            move |_, _| {
                flag.set(true);
                div().h(px(30.)).into_any_element()
            },
        )
    };
    view.update(cx, |v, cx| {
        v.set_annotations(&sources[0].key(), vec![block("img", 4, &drawn[0])], cx);
        v.set_annotations(
            &sources[1].key(),
            vec![block("gone", 40, &drawn[1]), block("two", 2, &drawn[2])],
            cx,
        );
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    assert_eq!(view.read_with(cx, |v, _| v.loaded_count()), 2);
    assert_eq!(
        drawn.each_ref().map(|d| d.get()),
        [true, true, true],
        "on the image, on line 40 of three, and on line 2"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
