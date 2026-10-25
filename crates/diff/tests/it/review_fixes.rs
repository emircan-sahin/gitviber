//! What a review of the diff found, kept fixed: saving over an outside write, a list reordered
//! while its files load, line actions on a diff that ignores whitespace.

use gitviber_diff::{
    Compare, DiffEvent, DiffOptions, DiffSource, DiffView, LineAction, LineRange, Side,
    StackedDiff, StackedFile, Whitespace,
};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
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

pub(crate) fn repo(name: &str, files: &[(&str, &str, &str)]) -> PathBuf {
    let dir = gitviber_ui::test_support::temp_dir(&format!("diff-fix-{name}"));
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Test"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    for (path, before, _) in files {
        std::fs::write(dir.join(path), before).unwrap();
    }
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    for (path, _, after) in files {
        std::fs::write(dir.join(path), after).unwrap();
    }
    dir
}

pub(crate) fn setup(cx: &mut TestAppContext, dir: &Path) {
    let core = Arc::new(gitviber_core::Core::new(Arc::new(|_| {})));
    gitviber_core::api::repo::open_repo(&core, dir.to_string_lossy().into_owned()).unwrap();
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_diff::set_core(core, cx);
    });
}

/// A window whose root is gpui-kit's, which draws dialogs.
pub(crate) fn window_with<V: 'static + gpui_kit::Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App) -> Entity<V>,
) -> (Entity<V>, &mut VisualTestContext) {
    let mut view = None;
    let handle = cx.add_window(|window, cx| {
        let v = build(window, cx);
        view = Some(v.clone());
        gpui_kit::base::Root::new(v, window, cx)
    });
    let cx = VisualTestContext::from_window(*handle, cx).into_mut();
    cx.run_until_parked();
    (view.unwrap(), cx)
}

#[gpui::test]
fn saving_over_an_outside_write_asks_and_leaves_it(cx: &mut TestAppContext) {
    let dir = repo("save", &[("a.txt", "one\n", "two\n")]);
    setup(cx, &dir);
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| DiffView::new(DiffSource::new(Compare::Unstaged, "a.txt"), cx))
    });
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            assert!(v.is_editable());
            v.toggle_edit(window, cx);
            v.set_edit_text("mine\n", window, cx);
        })
    });
    // An agent writes the file meanwhile.
    std::fs::write(dir.join("a.txt"), "agent\n").unwrap();
    cx.update(|window, cx| view.update(cx, |v, cx| v.save(window, cx)));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "agent\n"
    );
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "it asks before overwriting"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn saving_with_the_disk_as_read_writes(cx: &mut TestAppContext) {
    let dir = repo("save-ok", &[("a.txt", "one\n", "two\n")]);
    setup(cx, &dir);
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| DiffView::new(DiffSource::new(Compare::Unstaged, "a.txt"), cx))
    });
    let saved = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = saved.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, e: &DiffEvent, _| {
            seen.set(seen.get() || matches!(e, DiffEvent::Saved(_)))
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.toggle_edit(window, cx);
            v.set_edit_text("mine\n", window, cx);
            v.save(window, cx);
            // A second ⌘S while the first is on its way doesn't race it.
            v.save(window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "mine\n"
    );
    assert!(saved.get());
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn line_actions_refuse_a_diff_that_ignores_whitespace(cx: &mut TestAppContext) {
    // Re-indented and wrapped in `if`: the whitespace-blind diff shows only the `if` lines.
    let before = "a();\nb();\n";
    let after = "if (x) {\n  a();\n  b();\n}\n";
    let dir = repo("ws", &[("a.js", before, after)]);
    setup(cx, &dir);
    cx.update(|cx| DiffOptions::update(cx, |o| o.whitespace = Whitespace::Amount));
    let view =
        cx.update(|cx| cx.new(|cx| DiffView::new(DiffSource::new(Compare::Unstaged, "a.js"), cx)));
    cx.run_until_parked();
    let refused = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = refused.clone();
    let _sub = cx.update(|cx| {
        cx.subscribe(&view, move |_, e: &DiffEvent, _| {
            seen.set(seen.get() || matches!(e, DiffEvent::Notice(_)))
        })
    });
    view.update(cx, |v, cx| {
        v.select_lines(
            LineRange {
                side: Side::New,
                start: 1,
                end: 1,
            },
            cx,
        );
        v.line_action(LineAction::Stage, cx);
    });
    cx.run_until_parked();
    assert!(refused.get(), "it says why");
    assert_eq!(git(&dir, &["diff", "--cached"]), "", "nothing staged");
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn a_list_reordered_while_its_files_load_keeps_each_diff_with_its_file(cx: &mut TestAppContext) {
    let dir = repo(
        "reorder",
        &[
            ("a.txt", "a0\n", "a1\n"),
            ("b.txt", "b0\n", "b1\n"),
            ("c.txt", "c0\n", "c1\n"),
        ],
    );
    setup(cx, &dir);
    let file = |p: &str| StackedFile {
        sig: format!("{p}:1"),
        additions: Some(1),
        deletions: Some(1),
        ..StackedFile::new(DiffSource::new(Compare::Unstaged, p))
    };
    let view = cx.update(|cx| {
        cx.new(|cx| StackedDiff::new(vec![file("a.txt"), file("b.txt"), file("c.txt")], cx))
    });
    // The reads start, then the list comes back in another order before they land.
    view.update(cx, |v, cx| {
        v.schedule_now(cx);
        v.set_files(vec![file("c.txt"), file("a.txt"), file("b.txt")], cx);
    });
    cx.run_until_parked();
    let held = view.read_with(cx, |v, _| v.held());
    for (path, shown) in held {
        let first = path.chars().next().unwrap();
        assert_eq!(
            shown,
            Some(format!("{first}1")),
            "{path} holds its own diff"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
