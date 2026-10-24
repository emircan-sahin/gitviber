//! Open All over many files holds only the files near the screen, and gives everything back
//! when it closes.

use gitviber_diff::{Compare, DiffSource, StackedDiff, StackedFile};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div, px,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

const FILES: usize = 120;

fn git(dir: &Path, args: &[&str]) {
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
}

fn repo() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitviber-diff-stacked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Test"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    let text = |f: usize, changed: bool| {
        (0..60)
            .map(|i| {
                if changed && i % 10 == 3 {
                    format!("let f{f}_{i} = {};\n", i + 1)
                } else {
                    format!("let f{f}_{i} = {i};\n")
                }
            })
            .collect::<String>()
    };
    for f in 0..FILES {
        std::fs::write(dir.join(format!("f{f:03}.rs")), text(f, false)).unwrap();
    }
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    for f in 0..FILES {
        std::fs::write(dir.join(format!("f{f:03}.rs")), text(f, true)).unwrap();
    }
    dir
}

struct Host {
    view: Option<Entity<StackedDiff>>,
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
fn holds_only_the_files_near_the_screen(cx: &mut TestAppContext) {
    let dir = repo();
    let core = Arc::new(gitviber_core::Core::new(Arc::new(|_| {})));
    gitviber_core::api::repo::open_repo(&core, dir.to_string_lossy().into_owned()).unwrap();
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_diff::init(cx);
        gitviber_diff::set_core(core, cx);
    });
    let files: Vec<StackedFile> = (0..FILES)
        .map(|f| StackedFile {
            additions: Some(6),
            deletions: Some(6),
            ..StackedFile::new(DiffSource::new(Compare::Unstaged, format!("f{f:03}.rs")))
        })
        .collect();
    let window = cx.add_window(|_, _| Host { view: None });
    cx.run_until_parked();
    let snapshot = cx.update(|cx| cx.leak_detector_snapshot());
    let view = window
        .update(cx, |host, _, cx| {
            let v = cx.new(|cx| StackedDiff::new(files, cx));
            host.view = Some(v.clone());
            cx.notify();
            v
        })
        .unwrap();
    cx.run_until_parked();
    // The overview has every file from the first frame: those read with each of their six
    // changes, the rest with one span their line counts make.
    let added = |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.overview_spans()[1].len());
    let read = view.read_with(cx, |v, _| v.loaded_count());
    assert!(read > 0 && read < FILES);
    assert_eq!(added(cx), read * 6 + (FILES - read));
    let mut most = 0;
    let mut seen_last = false;
    // Down the whole list a screen at a time; each step loads what came near.
    for _ in 0..200 {
        view.update(cx, |v, cx| v.scroll_by(px(1400.), cx));
        cx.run_until_parked();
        let (loaded, top) = view.read_with(cx, |v, _| (v.loaded_count(), v.top_file()));
        most = most.max(loaded);
        // The last file is shorter than the window: at the end, the one before it is on top.
        seen_last |= top.is_some_and(|t| t >= FILES - 2);
    }
    assert!(seen_last, "never got to the last file");
    assert!(most > 0 && most <= 32, "held {most} files at once");
    // Every file's six changes stay on it, those given back too.
    assert_eq!(added(cx), FILES * 6);
    window
        .update(cx, |host, _, cx| {
            host.view = None;
            cx.notify();
        })
        .unwrap();
    drop(view);
    cx.run_until_parked();
    cx.update(|cx| cx.assert_no_new_leaks(&snapshot));
    let _ = std::fs::remove_dir_all(&dir);
}
