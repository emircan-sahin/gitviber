//! Opening and closing a diff gives everything back: no entity of it outlives its tab.

use gitviber_diff::{Compare, DiffData, DiffOptions, DiffSource, DiffView, Layout};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div,
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

fn texts() -> (String, String) {
    let old: String = (0..3000)
        .map(|i| format!("fn line_{i}() {{ let x = {i}; }}\n"))
        .collect();
    let new: String = (0..3000)
        .map(|i| {
            if i % 50 == 7 {
                format!("fn line_{i}() {{ let x = {}; }}\n", i + 1)
            } else {
                format!("fn line_{i}() {{ let x = {i}; }}\n")
            }
        })
        .collect();
    (old, new)
}

#[gpui::test]
fn closing_a_diff_frees_it(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_ui::test_support::install_theme(cx);
        gitviber_diff::init(cx);
        gitviber_diff::set_core(Arc::new(gitviber_core::Core::new(Arc::new(|_| {}))), cx);
    });
    let window = cx.add_window(|_, _| Host { view: None });
    cx.run_until_parked();
    let (old, new) = texts();
    let source = DiffSource::new(Compare::Unstaged, "src/lib.rs");
    let snapshot = cx.update(|cx| cx.leak_detector_snapshot());
    for round in 0..8 {
        let layout = if round % 2 == 0 {
            Layout::Unified
        } else {
            Layout::Split
        };
        cx.update(|cx| DiffOptions::update(cx, |o| o.layout = layout));
        window
            .update(cx, |host, _, cx| {
                let data = DiffData::from_texts(&old, &new, &source);
                let source = source.clone();
                host.view = Some(cx.new(|cx| {
                    let mut v = DiffView::with_data(source, data, cx);
                    v.highlight_blocking(cx);
                    v.set_viewed(Some(false), cx);
                    v
                }));
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        let drawn = window
            .update(cx, |host, _, cx| {
                host.view
                    .as_ref()
                    .is_some_and(|v| v.read(cx).is_loaded() && v.read(cx).is_drawn())
            })
            .unwrap();
        assert!(drawn);
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

#[gpui::test]
fn the_detector_sees_a_view_kept_alive(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_ui::test_support::install_theme(cx);
        gitviber_diff::set_core(Arc::new(gitviber_core::Core::new(Arc::new(|_| {}))), cx);
    });
    let snapshot = cx.update(|cx| cx.leak_detector_snapshot());
    let source = DiffSource::new(Compare::Unstaged, "a.txt");
    let kept = cx.update(|cx| {
        cx.new(|cx| {
            DiffView::with_data(
                source.clone(),
                DiffData::from_texts("a\n", "b\n", &source),
                cx,
            )
        })
    });
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.update(|cx| cx.assert_no_new_leaks(&snapshot))
    }));
    assert!(caught.is_err(), "leak detection isn't compiled in");
    drop(kept);
}
