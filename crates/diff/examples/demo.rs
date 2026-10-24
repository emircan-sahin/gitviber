//! A window with one diff of a repo, read-only, for screenshots and memory checks.
//!
//! demo [--repo DIR] [--commit SHA | --staged | --range BASE..HEAD] [--split] [--all]
//!      [--cycles N] [--stacked] [PATH]
//!
//! `--stacked` shows every changed file as one scroll (Open All) instead of one file.
//!
//! With no PATH it shows the first unstaged file (or the commit's first). `--cycles N` opens
//! and closes the view N times first and prints the process's memory before and after.
//! GITVIBER_BACKGROUND=1 opens the window behind the others without taking the keyboard.

use gitviber_core::{Core, api};
use gitviber_diff::{
    Compare, DiffData, DiffOptions, DiffSource, DiffView, Layout, StackedDiff, StackedFile,
};
use gpui_kit::{
    AnyView, App, AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement as _,
    Render, Styled as _, Task, Window, WindowBounds, WindowOptions, div, px, size,
};
use std::sync::Arc;
use std::time::Duration;

struct Args {
    repo: String,
    compare: Compare,
    path: Option<String>,
    split: bool,
    all: bool,
    cycles: usize,
    stacked: bool,
}

fn args() -> Args {
    let mut a = Args {
        repo: ".".into(),
        compare: Compare::Unstaged,
        path: None,
        split: false,
        all: false,
        cycles: 0,
        stacked: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--repo" => a.repo = it.next().unwrap_or_default(),
            "--commit" => {
                a.compare = Compare::Commit {
                    sha: it.next().unwrap_or_default(),
                }
            }
            "--staged" => a.compare = Compare::Staged,
            "--range" => {
                let r = it.next().unwrap_or_default();
                let (base, head) = r.split_once("..").unwrap_or((&r, "HEAD"));
                a.compare = Compare::Range {
                    base: base.into(),
                    head: head.into(),
                };
            }
            "--split" => a.split = true,
            "--all" => a.all = true,
            "--stacked" => a.stacked = true,
            "--cycles" => a.cycles = it.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            _ => a.path = Some(arg),
        }
    }
    a
}

/// The changed files of what's compared.
fn changed(core: &Core, compare: &Compare) -> Vec<gitviber_core::git::FileChange> {
    let files = match compare {
        Compare::Commit { sha } => api::history::commit_files(core, sha.clone()),
        Compare::Range { base, head } => {
            api::history::range_files(core, base.clone(), head.clone())
        }
        Compare::Staged => api::changes::status(core).map(|s| s.staged),
        _ => api::changes::status(core).map(|s| s.unstaged),
    };
    files.unwrap_or_default()
}

/// Every changed file as one scroll, the first few read before the first frame.
fn open_stacked(compare: &Compare, cx: &mut App) -> Entity<StackedDiff> {
    let core = gitviber_diff::core(cx);
    let files: Vec<StackedFile> = changed(&core, compare)
        .iter()
        .map(|f| StackedFile {
            additions: f.additions,
            deletions: f.deletions,
            sig: f.oid.clone().unwrap_or_default(),
            viewed: false,
            source: DiffSource::from_change(compare.clone(), f),
        })
        .collect();
    let first: Vec<_> = files
        .iter()
        .take(6)
        .map(|f| DiffData::load(&core, &f.source, DiffOptions::get(cx).whitespace))
        .collect();
    cx.new(|cx| {
        let mut s = StackedDiff::new(files, cx);
        s.set_title(Some("All Changes".into()), cx);
        for (i, data) in first.into_iter().enumerate() {
            if let Ok(data) = data {
                s.preload(i, data, true, cx);
            }
        }
        s
    })
}

fn footprint() -> String {
    let pid = std::process::id().to_string();
    let rss = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok();
    let rss = rss
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let phys = std::process::Command::new("footprint")
        .args(["-p", &pid])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.lines()
                .find(|l| l.contains("phys_footprint:"))
                .map(|l| l.trim().to_string())
        })
        .unwrap_or_default();
    format!("rss={}MB {phys}", rss / 1024)
}

struct Host {
    view: Option<AnyView>,
    _cycle: Task<()>,
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

/// A window behind others draws no frames (macOS stops its display link), so the screenshot
/// shows the first one: everything is read and parsed before it.
fn open(source: &DiffSource, cx: &mut App) -> Entity<DiffView> {
    let data = DiffData::load(
        &gitviber_diff::core(cx),
        source,
        DiffOptions::get(cx).whitespace,
    );
    let source = source.clone();
    cx.new(|cx| {
        let mut v = match data {
            Ok(data) => DiffView::with_data(source, data, cx),
            Err(_) => DiffView::new(source, cx),
        };
        v.highlight_blocking(cx);
        v.set_viewed(Some(false), cx);
        v
    })
}

fn main() {
    let a = args();
    let core = Arc::new(Core::new(Arc::new(|_| {})));
    let root = match api::repo::open_repo(&core, a.repo.clone()) {
        Ok(r) => r.root,
        Err(e) => {
            eprintln!("demo: {e}");
            std::process::exit(1);
        }
    };
    let source = match a.path.clone() {
        Some(path) => DiffSource::new(a.compare.clone(), path),
        None => match changed(&core, &a.compare).into_iter().next() {
            Some(f) => DiffSource::from_change(a.compare.clone(), &f),
            None => {
                eprintln!("demo: nothing changed in {root}");
                std::process::exit(1);
            }
        },
    };
    eprintln!("demo: {} {}", source.compare.kind(), source.path);
    let background = std::env::var("GITVIBER_BACKGROUND").is_ok_and(|v| v == "1");
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gitviber_diff::init(cx);
            gitviber_diff::set_core(core.clone(), cx);
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
            DiffOptions::set(
                DiffOptions {
                    layout: if a.split {
                        Layout::Split
                    } else {
                        Layout::Unified
                    },
                    collapse: !a.all,
                    ..Default::default()
                },
                cx,
            );
            let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                focus: !background,
                show: true,
                ..Default::default()
            };
            let cycles = a.cycles;
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                let stacked = a.stacked;
                let compare = a.compare.clone();
                let first: Option<AnyView> = (cycles == 0).then(|| {
                    if stacked {
                        open_stacked(&compare, cx).into()
                    } else {
                        open(&source, cx).into()
                    }
                });
                cx.new(|cx: &mut Context<Host>| {
                    let cycle = if cycles == 0 {
                        Task::ready(())
                    } else {
                        cx.spawn_in(window, async move |this, cx| {
                            eprintln!("demo: before {}", footprint());
                            for i in 0..=cycles {
                                this.update(cx, |host, cx| {
                                    host.view = Some(if stacked {
                                        open_stacked(&compare, cx).into()
                                    } else {
                                        open(&source, cx).into()
                                    });
                                    cx.notify();
                                })
                                .ok();
                                cx.background_executor()
                                    .timer(Duration::from_millis(400))
                                    .await;
                                if i < cycles {
                                    this.update(cx, |host, cx| {
                                        host.view = None;
                                        cx.notify();
                                    })
                                    .ok();
                                    cx.background_executor()
                                        .timer(Duration::from_millis(100))
                                        .await;
                                }
                                if i == 0 {
                                    eprintln!("demo: first open {}", footprint());
                                }
                            }
                            eprintln!("demo: after {cycles} open-close cycles {}", footprint());
                        })
                    };
                    Host {
                        view: first,
                        _cycle: cycle,
                    }
                })
            });
            if opened.is_err() {
                cx.quit();
            } else if !background {
                cx.activate(true);
            }
        });
}
