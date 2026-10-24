//! Staging and unstaging picked lines through the view, in a throwaway repo.

use gitviber_diff::{Compare, DiffSource, DiffView, LineAction, LineRange, Side};
use gpui_kit::{AppContext as _, Entity, TestAppContext};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitviber-diff-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Test"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    std::fs::write(dir.join("a.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    std::fs::write(dir.join("a.txt"), "1\nTWO\n3\n4\n5\n6\n7\n8\nNINE\n").unwrap();
    dir
}

fn open(cx: &mut TestAppContext, dir: &Path, compare: Compare) -> Entity<DiffView> {
    let core = Arc::new(gitviber_core::Core::new(Arc::new(|_| {})));
    gitviber_core::api::repo::open_repo(&core, dir.to_string_lossy().into_owned()).unwrap();
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_diff::set_core(core, cx);
    });
    let view = cx.update(|cx| cx.new(|cx| DiffView::new(DiffSource::new(compare, "a.txt"), cx)));
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.is_loaded()));
    view
}

fn act(cx: &mut TestAppContext, view: &Entity<DiffView>, lines: LineRange, action: LineAction) {
    view.update(cx, |v, cx| {
        v.select_lines(lines, cx);
        v.line_action(action, cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn stages_only_the_picked_lines_and_unstages_them(cx: &mut TestAppContext) {
    let dir = repo("stage");
    let view = open(cx, &dir, Compare::Unstaged);
    act(
        cx,
        &view,
        LineRange {
            side: Side::New,
            start: 2,
            end: 2,
        },
        LineAction::Stage,
    );
    let staged = git(&dir, &["diff", "--cached"]);
    assert!(
        staged.contains("+TWO") && staged.contains("-2") && !staged.contains("NINE"),
        "{staged}"
    );
    assert!(git(&dir, &["diff"]).contains("+NINE"));

    let staged_view = open(cx, &dir, Compare::Staged);
    act(
        cx,
        &staged_view,
        LineRange {
            side: Side::New,
            start: 2,
            end: 2,
        },
        LineAction::Unstage,
    );
    assert_eq!(git(&dir, &["diff", "--cached"]), "");
    let _ = std::fs::remove_dir_all(&dir);
}

// Not discard or stash: core puts the file's old version in the user's Trash outside its own
// tests (fs::trash_copy), and a test must not fill it.
