//! A markdown diff's preview shows the pictures of the version it shows, not the working tree's,
//! and takes the keyboard from the diff for its scroll keys.

use std::sync::Arc;

use crate::review_fixes::{git, repo, setup, window_with};
use gitviber_diff::{Compare, DiffData, DiffSource, DiffView};
use gpui_kit::{AppContext as _, Focusable as _, TestAppContext, VisualTestContext};

#[gpui::test]
fn a_commits_preview_reads_its_own_pictures(cx: &mut TestAppContext) {
    let dir = repo(
        "preview-images",
        &[
            ("a.md", "![](pic.png)\n", "![](pic.png)\nmore\n"),
            ("pic.png", "one", "two"),
        ],
    );
    git(&dir, &["commit", "-qam", "second"]);
    // The working tree has moved on since.
    std::fs::write(dir.join("pic.png"), "three").unwrap();
    let sha = git(&dir, &["rev-parse", "HEAD"]).trim().to_string();
    setup(cx, &dir);
    let source = DiffSource::new(Compare::Commit { sha }, "a.md");
    let (commit, cx) = window_with(cx, |_, cx| cx.new(|cx| DiffView::new(source, cx)));
    let read = |path: &str, cx: &mut gpui_kit::VisualTestContext| {
        commit.read_with(cx, |v, _| v.preview_image(path))
    };
    assert_eq!(read("pic.png", cx), Some(Ok(b"two".to_vec())));
    assert!(matches!(read("gone.png", cx), Some(Err(_))));
    // The working tree's come from disk, which the preview reads itself.
    let source = DiffSource::new(Compare::Unstaged, "a.md");
    let (worktree, cx) = window_with(cx, |_, cx| cx.new(|cx| DiffView::new(source, cx)));
    assert_eq!(
        worktree.read_with(cx, |v, _| v.preview_image("pic.png")),
        None
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn the_preview_takes_the_keys_from_the_diff_and_gives_them_back(cx: &mut TestAppContext) {
    // The app's theme and keys, a rendered document's among them.
    gitviber_ui::test_support::config_dir("diff-preview-keys");
    cx.update(|cx| {
        gitviber_ui::init(cx);
        gitviber_diff::actions::init_local(cx);
        gitviber_diff::set_core(Arc::new(gitviber_core::Core::new(Arc::new(|_| {}))), cx);
    });
    let old: String = (1..=300)
        .map(|i| format!("Paragraph {i}, a line of words to read.\n\n"))
        .collect();
    let new = old.replace("Paragraph 150,", "Changed 150,");
    let source = DiffSource::new(Compare::Unstaged, "a.md");
    let data = DiffData::from_texts(&old, &new, &source);
    let (diff, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| DiffView::with_data(source, data, cx))
    });
    let draw = |cx: &mut VisualTestContext| {
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
    };
    let own = cx.update(|_, cx| diff.read(cx).focus_handle(cx));
    let focused = |cx: &mut VisualTestContext| {
        let preview = diff.read_with(cx, |d, _| d.markdown_preview().cloned());
        cx.update(|window, cx| {
            (
                own.is_focused(window),
                preview.is_some_and(|p| p.focus_handle(cx).is_focused(window)),
            )
        })
    };
    // Focus events reach an active window only.
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&own, cx);
    });
    draw(cx);
    diff.update(cx, |d, cx| d.set_preview(true, cx));
    draw(cx);
    assert_eq!(focused(cx), (false, true), "the preview took the keys");
    let preview = diff.read_with(cx, |d, _| d.markdown_preview().cloned().unwrap());
    let top = |cx: &mut VisualTestContext| {
        draw(cx);
        f32::from(preview.read_with(cx, |p, cx| p.scroll_top(cx)))
    };
    cx.simulate_keystrokes("pagedown");
    let page = top(cx);
    assert!(page > 100., "{page}");
    cx.simulate_keystrokes("end");
    let end = top(cx);
    assert!(end > page * 4., "{end}");
    cx.simulate_keystrokes("pagedown");
    assert_eq!(top(cx), end, "at the end");
    // The diff's own focus, as a tab shown again gives it, goes on to the preview.
    cx.update(|window, cx| window.focus(&own, cx));
    draw(cx);
    assert_eq!(focused(cx), (false, true));
    cx.simulate_keystrokes("home");
    assert_eq!(top(cx), 0.);
    // Back to the lines: the diff has the keys again.
    diff.update(cx, |d, cx| d.set_preview(false, cx));
    draw(cx);
    assert_eq!(focused(cx), (true, false));
}
