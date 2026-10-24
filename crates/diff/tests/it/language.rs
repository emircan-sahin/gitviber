//! The status bar names the language of the diff a tab shows: its file's, or in a list the
//! file on top's; nothing while an image shows.

use crate::review_fixes::{repo, setup, window_with};
use gitviber_diff::{Compare, DiffSource, DiffView, StackedDiff, StackedFile, shown_language};
use gpui_kit::{AppContext as _, Entity, Focusable, TestAppContext, VisualTestContext};

fn named<V: Focusable>(view: &Entity<V>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| shown_language(&view.read(cx).focus_handle(cx), cx))
}

#[gpui::test]
fn a_diff_and_the_file_on_top_of_a_list_name_their_language(cx: &mut TestAppContext) {
    // Longer than the window, so the list can bring its header to the top.
    let (old, new): (String, String) = (0..200)
        .map(|i| (format!("let a{i} = 0;\n"), format!("let a{i} = 1;\n")))
        .unzip();
    let dir = repo(
        "language",
        &[
            ("a.rs", &old, &new),
            ("run", "#!/bin/sh\necho 1\n", "#!/bin/sh\necho 2\n"),
            ("b.png", "one", "two"),
        ],
    );
    setup(cx, &dir);
    let diff = |path: &str| {
        let source = DiffSource::new(Compare::Unstaged, path);
        move |_: &mut gpui_kit::Window, cx: &mut gpui_kit::App| {
            cx.new(|cx| DiffView::new(source, cx))
        }
    };
    let (rust, cx) = window_with(cx, diff("a.rs"));
    assert_eq!(named(&rust, cx).as_deref(), Some("Rust"));
    let (script, cx) = window_with(cx, diff("run"));
    assert_eq!(named(&script, cx).as_deref(), Some("Shell"));
    let (image, cx) = window_with(cx, diff("b.png"));
    assert_eq!(named(&image, cx), None);

    let files = ["run", "a.rs"]
        .map(|p| StackedFile::new(DiffSource::new(Compare::Unstaged, p)))
        .to_vec();
    let (list, cx) = window_with(cx, move |_, cx| cx.new(|cx| StackedDiff::new(files, cx)));
    cx.run_until_parked();
    assert_eq!(named(&list, cx).as_deref(), Some("Shell"));
    list.update(cx, |v, cx| v.step(true, cx));
    cx.run_until_parked();
    assert_eq!(named(&list, cx).as_deref(), Some("Rust"));
    let _ = std::fs::remove_dir_all(&dir);
}
