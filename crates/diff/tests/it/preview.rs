//! A markdown diff's preview shows the pictures of the version it shows, not the working tree's.

use crate::review_fixes::{git, repo, setup, window_with};
use gitviber_diff::{Compare, DiffSource, DiffView};
use gpui_kit::{AppContext as _, TestAppContext};

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
