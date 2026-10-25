//! The views against a real repo in a temporary folder: closing one gives its memory back (no
//! task, subscription or callback keeps an editor, rope or image alive), the explorer's file
//! actions, quick open's list, and ⌘S keeping the file's line endings.

use gitviber_ui::workspace::Workspace;
use gpui_kit::{
    AnyView, AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div,
};

use crate::code::{CodeSource, CodeView};
use crate::media::{MediaKind, MediaView};
use crate::text::{Bar, BarKind};

struct Host(Option<AnyView>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.0.clone())
    }
}

fn setup(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("gitviber-files-test-{}", std::process::id()));
    // SAFETY: the tests that read the config dir all point it at a temporary folder.
    unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
    cx.update(|cx| {
        gitviber_ui::init(cx);
        crate::init(cx);
    });
}

#[gpui_kit::test]
fn closing_code_and_media_views_gives_their_entities_back(cx: &mut TestAppContext) {
    setup(cx);
    let text: String = (0..4000)
        .map(|i| format!("fn f{i}() -> u32 {{ {i} }}\n"))
        .collect();
    let png = std::env::temp_dir().join(format!("gitviber-files-test-{}.png", std::process::id()));
    image::RgbaImage::from_pixel(64, 48, image::Rgba([200, 30, 30, 255]))
        .save(&png)
        .ok();
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    for round in 0..4 {
        let (text, png) = (text.clone(), png.clone());
        cx.update(|window, cx| {
            let view: AnyView = if round % 2 == 0 {
                let source = CodeSource {
                    path: "src/lib.rs".into(),
                    root: Some(std::env::temp_dir()),
                };
                let bars = vec![
                    Bar {
                        line: 3,
                        kind: BarKind::Added,
                    },
                    Bar {
                        line: 9,
                        kind: BarKind::Deleted,
                    },
                ];
                cx.new(|cx| {
                    CodeView::new(
                        gpui_kit::WeakEntity::<Workspace>::new_invalid(),
                        source,
                        text,
                        Some("x".into()),
                        bars,
                        false,
                        window,
                        cx,
                    )
                })
                .into()
            } else {
                cx.new(|cx| MediaView::new(png, MediaKind::Image, window, cx))
                    .into()
            };
            host.update(cx, |h, cx| {
                h.0 = Some(view);
                cx.notify();
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            host.update(cx, |h, cx| {
                h.0 = None;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
}

/// A big image opened and closed again and again keeps nothing: while open only a picture
/// the size it's shown is held, and closing gives back the views and their pictures.
#[gpui_kit::test]
fn a_large_image_opened_and_closed_again_and_again_keeps_nothing(cx: &mut TestAppContext) {
    setup(cx);
    let png = std::env::temp_dir().join(format!("gitviber-files-big-{}.png", std::process::id()));
    image::RgbaImage::from_fn(3000, 2000, |x, y| {
        image::Rgba([(x % 251) as u8, (y % 241) as u8, 90, 255])
    })
    .save(&png)
    .unwrap();
    let live = |kind: &str| {
        gitviber_ui::memory::live_counts()
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(0, |(_, n)| *n)
    };
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    // Smaller than the image even on a 2× display.
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(900.), gpui_kit::px(600.)));
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    let settle = |cx: &mut gpui_kit::VisualTestContext| {
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(200));
            cx.run_until_parked();
        }
    };
    for _ in 0..6 {
        let png = png.clone();
        let view =
            cx.update(|window, cx| cx.new(|cx| MediaView::new(png, MediaKind::Image, window, cx)));
        host.update(cx, |h, cx| {
            h.0 = Some(view.clone().into());
            cx.notify();
        });
        settle(cx);
        let (size, sf) = cx.update(|window, _| (window.viewport_size(), window.scale_factor()));
        let (held, crop) = view.read_with(cx, |m, cx| m.image().unwrap().read(cx).held());
        let (w, h) = held.expect("drawn");
        assert!(!crop);
        assert!(w < 3000 && w as f32 <= f32::from(size.width) * sf, "{w}");
        assert!(h as f32 <= f32::from(size.height) * sf, "{h}");
        assert_eq!(live("ImagePreview"), 1);
        host.update(cx, |h, cx| {
            h.0 = None;
            cx.notify();
        });
        drop(view);
        settle(cx);
        assert_eq!((live("ImagePreview"), live("MediaView")), (0, 0));
    }
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    std::fs::remove_file(png).ok();
}

/// A repo in a temporary folder with one big file, committed, then changed.
fn temp_repo(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gitviber-files-{name}-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let big: String = (0..60_000)
        .map(|i| format!("    let value_{i} = compute(\"item {i}\", {i}); // line {i}\n"))
        .collect();
    std::fs::write(dir.join("src/big.rs"), &big).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-qm",
        "init",
    ]);
    std::fs::write(dir.join("src/big.rs"), format!("// changed\n{big}")).unwrap();
    dir
}

/// Draws the workspace's active tab, as the app's center does.
struct WsHost(gpui_kit::Entity<Workspace>);

impl Render for WsHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self
            .0
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle())
            .map(|h| h.view());
        div().size_full().children(view)
    }
}

#[gpui_kit::test]
fn file_tabs_opened_and_closed_give_their_entities_back(cx: &mut TestAppContext) {
    setup(cx);
    let repo = temp_repo("tabs");
    cx.update(|cx| {
        gitviber_ui::install_core(cx);
    });
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    let weak = ws.downgrade();
    for _ in 0..3 {
        cx.update(|window, cx| {
            crate::open_file(
                &weak,
                "src/big.rs",
                crate::OpenOptions::default(),
                window,
                cx,
            )
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        let open = cx.update(|_, cx| ws.read(cx).tabs().len());
        assert_eq!(open, 1);
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| ws.close_active_tab(cx));
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    _ = std::fs::remove_dir_all(&repo);
}

#[gpui_kit::test]
fn explorer_lists_creates_and_renames_and_quick_open_lists_files(cx: &mut TestAppContext) {
    setup(cx);
    let repo = temp_repo("explorer");
    cx.update(|cx| {
        gitviber_ui::install_core(cx);
    });
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let panel = cx
        .update(|window, cx| {
            ws.update(cx, |ws, cx| ws.panel(crate::explorer::PANEL_ID, window, cx))
        })
        .unwrap();
    let panel = panel.view().downcast::<crate::ExplorerPanel>().unwrap();
    cx.run_until_parked();
    let rows = cx.update(|_, cx| panel.read(cx).row_paths());
    assert_eq!(rows, vec!["src"]);
    cx.update(|window, cx| panel.update(cx, |p, cx| p.reveal("src/big.rs", false, window, cx)));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| panel.read(cx).row_paths()),
        vec!["src", "src/big.rs"]
    );
    // New File… named with a folder, then renamed.
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.create_for_test("src", "docs/a.md", window, cx)
        })
    });
    cx.run_until_parked();
    assert!(repo.join("src/docs/a.md").is_file());
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.rename_for_test("src/docs/a.md", "b.md", window, cx)
        })
    });
    cx.run_until_parked();
    assert!(repo.join("src/docs/b.md").is_file() && !repo.join("src/docs/a.md").exists());
    // ⌘S writes what the editor holds, with the file's own line endings.
    std::fs::write(repo.join("src/crlf.txt"), "one\r\ntwo\r\n").unwrap();
    let weak = ws.downgrade();
    cx.update(|window, cx| {
        crate::open_file(
            &weak,
            "src/crlf.txt",
            crate::OpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let view = cx.update(|_, cx| {
        ws.read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle())
            .map(|h| h.view())
            .unwrap()
    });
    let code = cx
        .update(|_, cx| view.downcast::<crate::FileView>().unwrap().read(cx).code())
        .expect("the text shows");
    cx.update(|window, cx| {
        let state = code.read(cx).state().clone();
        state.update(cx, |s, cx| s.set_value("one\nthree\n", window, cx));
        code.update(cx, |c, cx| c.save(window, cx));
    });
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(repo.join("src/crlf.txt")).unwrap(),
        "one\r\nthree\r\n"
    );
    // Quick open lists what git lists, the new untracked file too.
    let files: Vec<String> = cx.update(|_, cx| {
        let ctx = ws.update(cx, |ws, cx| ws.context(cx));
        let source = gitviber_ui::workspace::Registry::get(cx)
            .quick_open()
            .unwrap();
        source(&ctx, cx)
            .into_iter()
            .map(|q| q.label.to_string())
            .collect()
    });
    assert!(files.contains(&"src/big.rs".to_string()), "{files:?}");
    _ = std::fs::remove_dir_all(&repo);
}

/// At launch the explorer is built while the repo is still opening: it lists once it's open,
/// with no "No repository is open" toast, and a root that can't be listed still says so.
#[gpui_kit::test]
fn explorer_built_while_the_repo_opens_lists_once_open(cx: &mut TestAppContext) {
    setup(cx);
    let repo = temp_repo("explorer-launch");
    cx.update(|cx| {
        gitviber_ui::install_core(cx);
    });
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    let panel = cx
        .update(|window, cx| {
            Workspace::register_window(&ws, window, cx);
            ws.update(cx, |ws, cx| {
                ws.open_project(repo.clone(), window, cx);
                ws.panel(crate::explorer::PANEL_ID, window, cx)
            })
        })
        .unwrap();
    let panel = panel.view().downcast::<crate::ExplorerPanel>().unwrap();
    // Nothing lists yet: on real threads that listing beats the open and fails, while this
    // executor happens to run the open first.
    cx.update(|_, cx| {
        let project = ws.read(cx).project().unwrap().read(cx);
        assert!(!project.is_open() && !panel.read(cx).is_listing());
    });
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| panel.read(cx).row_paths()), vec!["src"]);
    let titles = |cx: &mut gpui_kit::VisualTestContext| -> Vec<String> {
        cx.update(|_, cx| ws.update(cx, |ws, _| ws.take_toasts()))
            .into_iter()
            .filter_map(|t| t.title.map(|t| t.to_string()))
            .collect()
    };
    assert!(titles(cx).is_empty());
    std::fs::rename(&repo, repo.with_extension("moved")).unwrap();
    cx.update(|_, cx| panel.update(cx, |p, cx| p.refresh(cx)));
    cx.run_until_parked();
    assert!(
        titles(cx).contains(&"Could not list repository".to_string()),
        "a real failure still shows"
    );
    _ = std::fs::remove_dir_all(repo.with_extension("moved"));
}

/// ⌘P's files and how they stand, as the palette asks for them.
fn quick_open(
    ws: &gpui_kit::Entity<Workspace>,
    cx: &mut gpui_kit::VisualTestContext,
) -> (Vec<String>, crate::quick_open::QuickOpenStatus) {
    cx.update(|_, cx| {
        let ctx = ws.update(cx, |ws, cx| ws.context(cx));
        let source = gitviber_ui::workspace::Registry::get(cx)
            .quick_open()
            .unwrap();
        let files = source(&ctx, cx)
            .into_iter()
            .map(|q| q.label.to_string())
            .collect();
        (files, crate::quick_open::status(&ctx, cx))
    })
}

/// ⌘P never runs git on the UI thread: the first ask starts a read and gets none, observers hear
/// when the list lands, and a failed read is kept and said rather than run again on each ask.
#[gpui_kit::test]
fn quick_open_reads_in_the_background_and_keeps_a_failure(cx: &mut TestAppContext) {
    use crate::quick_open::{QuickOpenFiles, QuickOpenStatus};

    setup(cx);
    let repo = temp_repo("quick-open");
    let broken = temp_repo("quick-open-broken");
    // Opens (rev-parse doesn't read the index), but ls-files fails.
    std::fs::write(broken.join(".git/index"), "not an index").unwrap();
    cx.update(|cx| {
        gitviber_ui::install_core(cx);
    });
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    let heard = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = heard.clone();
    cx.update(|window, cx| {
        cx.observe_global::<QuickOpenFiles>(move |_| count.set(count.get() + 1))
            .detach();
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    assert_eq!(quick_open(&ws, cx), (vec![], QuickOpenStatus::Listing));
    cx.run_until_parked();
    assert!(heard.get() > 0);
    let (files, status) = quick_open(&ws, cx);
    assert!(files.contains(&"src/big.rs".to_string()), "{files:?}");
    assert_eq!(status, QuickOpenStatus::Listed);

    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_project(broken.clone(), window, cx)));
    cx.run_until_parked();
    assert_eq!(quick_open(&ws, cx), (vec![], QuickOpenStatus::Listing));
    cx.run_until_parked();
    for _ in 0..2 {
        let (files, status) = quick_open(&ws, cx);
        assert!(files.is_empty());
        assert!(
            matches!(&status, QuickOpenStatus::Failed(e) if e.contains("index")),
            "{status:?}"
        );
    }
    _ = std::fs::remove_dir_all(&repo);
    _ = std::fs::remove_dir_all(&broken);
}

/// A file opened with the keyboard (⌘P) has it in its code once the text is read, not on the
/// tab that took it while loading.
#[gpui_kit::test]
fn a_file_opened_with_focus_has_it_in_its_code_once_loaded(cx: &mut TestAppContext) {
    setup(cx);
    let repo = temp_repo("focus");
    cx.update(|cx| {
        gitviber_ui::install_core(cx);
    });
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let weak = ws.downgrade();
    cx.update(|window, cx| {
        let opts = crate::OpenOptions {
            focus: true,
            ..Default::default()
        };
        crate::open_file(&weak, "src/big.rs", opts, window, cx)
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let view = ws.read(cx).tabs().active().unwrap().item.handle().unwrap();
        let code = view.view().downcast::<crate::FileView>().unwrap();
        let code = code.read(cx).code().expect("the text shows");
        assert!(code.read(cx).editor_focus(cx).is_focused(window));
    });
    _ = std::fs::remove_dir_all(&repo);
}

#[gpui_kit::test]
fn blame_names_each_lines_commit_and_new_lines(cx: &mut TestAppContext) {
    setup(cx);
    let repo = temp_repo("blame");
    cx.update(|cx| {
        gitviber_ui::install_core(cx);
        gitviber_ui::Settings::update(cx, |s| s.blame = true);
    });
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let weak = ws.downgrade();
    cx.update(|window, cx| {
        crate::open_file(
            &weak,
            "src/big.rs",
            crate::OpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let code = cx.update(|_, cx| {
        let view = ws
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle())
            .map(|h| h.view())
            .unwrap();
        view.downcast::<crate::FileView>().unwrap().read(cx).code()
    });
    let shas = cx
        .update(|_, cx| code.unwrap().read(cx).blamed_shas())
        .expect("blame came");
    // temp_repo committed the file, then put "// changed" on a new first line.
    assert!(shas[0].bytes().all(|b| b == b'0'), "{}", shas[0]);
    assert!(shas[1].len() == 40 && !shas[1].bytes().all(|b| b == b'0'));
    assert_eq!(shas.len(), 60_001);
    // Off again: dropped.
    cx.update(|_, cx| gitviber_ui::Settings::update(cx, |s| s.blame = false));
    cx.run_until_parked();
    let code = cx.update(|_, cx| {
        let view = ws
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle())
            .map(|h| h.view())
            .unwrap();
        view.downcast::<crate::FileView>().unwrap().read(cx).code()
    });
    assert!(
        cx.update(|_, cx| code.unwrap().read(cx).blamed_shas())
            .is_none()
    );
    _ = std::fs::remove_dir_all(&repo);
}

/// Copy GitHub Link from a file's code: the picked lines at the pushed commit, else the file.
#[gpui_kit::test]
fn copy_github_link_takes_the_picked_lines_or_the_file(cx: &mut TestAppContext) {
    setup(cx);
    let repo = std::env::temp_dir().join(format!("gitviber-files-link-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a b.rs"), "one\ntwo\nthree\nfour\n").unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-qm",
        "init",
    ]);
    git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/owner/name.git",
    ]);
    // As if pushed: origin has HEAD.
    git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let sha = git(&["rev-parse", "HEAD"]);
    cx.update(gitviber_ui::install_core);
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let weak = ws.downgrade();
    cx.update(|window, cx| {
        crate::open_file(
            &weak,
            "src/a b.rs",
            crate::OpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let code = cx.update(|_, cx| {
        let view = ws
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle())
            .map(|h| h.view())
            .unwrap();
        view.downcast::<crate::FileView>().unwrap().read(cx).code()
    });
    let code = code.unwrap();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let copy = |range: std::ops::Range<usize>, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            let state = code.read(cx).state().clone();
            state.update(cx, |s, cx| s.set_selected_range(range, cx));
            code.read(cx).editor_focus(cx).focus(window, cx);
        });
        cx.dispatch_action(gitviber_ui::keymap::editor::CopyGitHubLink);
        cx.run_until_parked();
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()))
    };
    let blob = format!("https://github.com/owner/name/blob/{sha}/src/a%20b.rs");
    // "two\nthree\n": the selection ends at the start of line 4, which isn't picked.
    assert_eq!(copy(4..14, cx), Some(format!("{blob}#L2-L3")));
    assert_eq!(copy(4..6, cx), Some(format!("{blob}#L2")));
    assert_eq!(copy(0..0, cx), Some(blob));
    _ = std::fs::remove_dir_all(&repo);
}

#[gpui_kit::test]
fn find_in_the_rendered_markdown_preview(cx: &mut TestAppContext) {
    setup(cx);
    let repo = std::env::temp_dir().join(format!("gitviber-files-mdfind-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(
        repo.join("README.md"),
        "# Foo\n\nfoo and **foo**, then a food.\n",
    )
    .unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .output()
        .unwrap();
    cx.update(gitviber_ui::install_core);
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let weak = ws.downgrade();
    cx.update(|window, cx| {
        crate::open_file(
            &weak,
            "README.md",
            crate::OpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let file = cx.update(|_, cx| {
        let view = ws
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle())
            .unwrap()
            .view();
        view.downcast::<crate::FileView>().unwrap()
    });
    cx.update(|window, cx| file.update(cx, |f, cx| f.open_find(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // Typed into the box (focused by open_find): the heading, the plain and the bold one, and
    // "food"'s start; the first is current.
    cx.simulate_input("foo");
    cx.run_until_parked();
    assert_eq!(
        file.read_with(cx, |f, cx| f.preview_find(cx)),
        Some((Some(0), 4))
    );
    // ↵ steps on, and the key isn't typed into the box (which would start over at the first).
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        file.read_with(cx, |f, cx| f.preview_find(cx)),
        Some((Some(1), 4))
    );
    cx.simulate_keystrokes("shift-enter");
    cx.run_until_parked();
    assert_eq!(
        file.read_with(cx, |f, cx| f.preview_find(cx)),
        Some((Some(0), 4))
    );
    // ⌥⌘C: Match Case, found again from the first ("Foo" in the heading drops out).
    let flip = if cfg!(target_os = "macos") {
        "alt-cmd-c"
    } else {
        "alt-c"
    };
    cx.simulate_keystrokes(flip);
    cx.run_until_parked();
    assert_eq!(
        file.read_with(cx, |f, cx| f.preview_find(cx)),
        Some((Some(0), 3))
    );
    // The file changes on disk: the count follows.
    std::fs::write(repo.join("README.md"), "Just one foo.\n").unwrap();
    cx.update(|window, cx| file.update(cx, |f, cx| f.load(window, cx)));
    cx.run_until_parked();
    assert_eq!(
        file.read_with(cx, |f, cx| f.preview_find(cx)),
        Some((Some(0), 1))
    );
    cx.update(|window, cx| file.update(cx, |f, cx| f.close_find(window, cx)));
    assert_eq!(file.read_with(cx, |f, cx| f.preview_find(cx)), None);
    // ⌘F again: the last query, found again.
    cx.update(|window, cx| file.update(cx, |f, cx| f.open_find(window, cx)));
    assert_eq!(
        file.read_with(cx, |f, cx| f.preview_find(cx)),
        Some((Some(0), 1))
    );
    _ = std::fs::remove_dir_all(&repo);
}

/// The workspace's active tab and a panel, side by side.
struct PanelHost(gpui_kit::Entity<Workspace>, Option<AnyView>);

impl Render for PanelHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self
            .0
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle())
            .map(|h| h.view());
        div()
            .size_full()
            .flex()
            .children(self.1.clone().map(|p| div().w_1_2().h_full().child(p)))
            .children(tab.map(|t| div().w_1_2().h_full().child(t)))
    }
}

#[gpui_kit::test]
fn search_panel_keys_move_through_results_and_back(cx: &mut TestAppContext) {
    setup(cx);
    let repo =
        std::env::temp_dir().join(format!("gitviber-files-searchkeys-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("a.txt"), "foo one\nbar\nfoo two\n").unwrap();
    std::fs::write(repo.join("b.txt"), "foo three\n").unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-qm",
        "init",
    ]);
    cx.update(gitviber_ui::install_core);
    let (host, cx) = cx.add_window_view(|_, cx| PanelHost(cx.new(Workspace::new), None));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let panel = cx
        .update(|window, cx| ws.update(cx, |ws, cx| ws.panel(crate::search::PANEL_ID, window, cx)))
        .unwrap()
        .view();
    host.update(cx, |h, cx| {
        h.1 = Some(panel.clone());
        cx.notify();
    });
    let panel = panel.downcast::<crate::SearchPanel>().unwrap();
    cx.update(|window, cx| panel.update(cx, |p, cx| p.ask(Some("foo".into()), window, cx)));
    cx.run_until_parked();
    let draw = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    };
    draw(cx);
    let keys = |cx: &mut gpui_kit::VisualTestContext| panel.read_with(cx, |p, _| p.keyboard());
    // a.txt, its two matches, b.txt, its one.
    assert_eq!(keys(cx), (None, 5, false));
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(keys(cx), (Some(0), 5, false));
    // Landing on a match previews it.
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(keys(cx), (Some(1), 5, false));
    assert_eq!(cx.update(|_, cx| ws.read(cx).tabs().len()), 1);
    cx.simulate_keystrokes("left");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(keys(cx), (Some(0), 3, false));
    cx.simulate_keystrokes("end");
    assert_eq!(keys(cx), (Some(2), 3, false));
    // Back in the box: ⌥⌘C flips Match Case, Esc clears, then goes back to the Explorer.
    cx.update(|window, cx| panel.update(cx, |p, cx| p.ask(None, window, cx)));
    draw(cx);
    let flip = if cfg!(target_os = "macos") {
        "alt-cmd-c"
    } else {
        "alt-c"
    };
    cx.simulate_keystrokes(flip);
    draw(cx);
    assert!(keys(cx).2);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(keys(cx).1, 0);
    cx.simulate_keystrokes("escape");
    draw(cx);
    let explorer = cx
        .update(|window, cx| {
            ws.update(cx, |ws, cx| ws.panel(crate::explorer::PANEL_ID, window, cx))
        })
        .unwrap();
    let focused = cx.update(|window, cx| {
        explorer
            .focus_handle(cx)
            .is_some_and(|f| f.is_focused(window))
    });
    assert!(focused);
    _ = std::fs::remove_dir_all(&repo);
}

/// Reveal in Explorer and Find in Files build their panel the first time outside the
/// workspace's update: a panel that reads the workspace as it's made would abort the app.
#[gpui_kit::test]
fn reveal_and_find_in_files_build_their_panels_the_first_time(cx: &mut TestAppContext) {
    use gitviber_ui::workspace::Slot;
    setup(cx);
    let repo = small_repo("panels");
    cx.update(gitviber_ui::install_core);
    let (host, cx) = cx.add_window_view(|_, cx| PanelHost(cx.new(Workspace::new), None));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let weak = ws.downgrade();
    cx.update(|window, cx| crate::reveal_in_explorer(&weak, "src/b.rs", window, cx));
    cx.run_until_parked();
    let shown = cx.update(|_, cx| ws.read(cx).active_panel(Slot::Right, cx));
    assert_eq!(shown, Some(crate::explorer::PANEL_ID));
    let explorer = cx
        .update(|window, cx| Workspace::ensure_panel(&ws, crate::explorer::PANEL_ID, window, cx))
        .and_then(|p| p.view().downcast::<crate::ExplorerPanel>().ok())
        .expect("built by the reveal");
    let rows = cx.update(|_, cx| explorer.read(cx).row_paths());
    assert!(rows.contains(&"src/b.rs".to_string()), "{rows:?}");
    cx.update(|window, cx| crate::find_in_files(&weak, Some("b".into()), window, cx));
    cx.run_until_parked();
    let shown = cx.update(|_, cx| ws.read(cx).active_panel(Slot::Right, cx));
    assert_eq!(shown, Some(crate::search::PANEL_ID));
    let built = cx.update(|window, cx| {
        Workspace::ensure_panel(&ws, crate::search::PANEL_ID, window, cx)
            .is_some_and(|p| p.view().downcast::<crate::SearchPanel>().is_ok())
    });
    assert!(built);
    _ = std::fs::remove_dir_all(&repo);
}

/// A repo with a.txt and src/b.rs, added but not committed.
fn small_repo(name: &str) -> std::path::PathBuf {
    let repo = std::env::temp_dir().join(format!("gitviber-files-{name}-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.join("src/b.rs"), "b\n").unwrap();
    for args in [&["init", "-q"][..], &["add", "-A"]] {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
    }
    repo
}

/// Esc in the explorer's filter clears it, showing the whole tree again, and a second Esc
/// closes it: clearing the box sends no change of its own.
#[gpui_kit::test]
fn escape_clears_the_explorer_filter_then_closes_it(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("filter");
    cx.update(gitviber_ui::install_core);
    let (host, cx) = cx.add_window_view(|_, cx| PanelHost(cx.new(Workspace::new), None));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let panel = cx
        .update(|window, cx| {
            ws.update(cx, |ws, cx| ws.panel(crate::explorer::PANEL_ID, window, cx))
        })
        .unwrap()
        .view();
    host.update(cx, |h, cx| {
        h.1 = Some(panel.clone());
        cx.notify();
    });
    let panel = panel.downcast::<crate::ExplorerPanel>().unwrap();
    let draw = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    };
    draw(cx);
    let whole = cx.update(|_, cx| panel.read(cx).row_paths());
    assert_eq!(whole, vec!["src", "a.txt"]);
    cx.update(|window, cx| panel.update(cx, |p, cx| p.open_filter(window, cx)));
    draw(cx);
    cx.simulate_input("b.rs");
    draw(cx);
    let state = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| {
            let p = panel.read(cx);
            (p.filter_needle(), p.row_paths())
        })
    };
    assert_eq!(
        state(cx),
        (Some("b.rs".into()), vec!["src".into(), "src/b.rs".into()])
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(state(cx), (Some(String::new()), whole.clone()));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(state(cx).0, None);
    // ↓ from the box goes to the rows.
    cx.update(|window, cx| panel.update(cx, |p, cx| p.open_filter(window, cx)));
    draw(cx);
    cx.simulate_keystrokes("down");
    draw(cx);
    let focused = cx.update(|window, cx| {
        use gitviber_ui::workspace::Panel as _;
        let focus = panel.read(cx).panel_focus(cx).unwrap();
        focus.is_focused(window)
    });
    assert!(focused);
    _ = std::fs::remove_dir_all(&repo);
}

/// The scrollbar's map shows the whole file's bars and find matches on the first frame, with
/// nothing scrolled, and again after soft wrap, an edit and a query: each time the same as every
/// bar and match placed on its own from the display map.
#[gpui_kit::test]
fn the_code_map_marks_the_whole_file_without_scrolling(cx: &mut TestAppContext) {
    use crate::code_map::tests::naive;
    use crate::code_map::{EditorRows, Mark, MarkKind, track};

    setup(cx);
    let text: String = (0..3000)
        .map(|i| format!("let v{i} = \"{}\";\n", "x".repeat(i % 7 * 90)))
        .collect();
    let mut bars = vec![Bar {
        line: 1,
        kind: BarKind::Added,
    }];
    bars.extend((1500..=1510).map(|line| Bar {
        line,
        kind: BarKind::Modified,
    }));
    bars.push(Bar {
        line: 2999,
        kind: BarKind::Deleted,
    });
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| {
        let source = CodeSource {
            path: "src/lib.rs".into(),
            root: None,
        };
        let view = cx.new(|cx| {
            CodeView::new(
                gpui_kit::WeakEntity::<Workspace>::new_invalid(),
                source,
                text,
                None,
                bars.clone(),
                false,
                window,
                cx,
            )
        });
        host.update(cx, |h, cx| {
            h.0 = Some(view.clone().into());
            cx.notify();
        });
        view
    });
    // One frame, then the map against the bars and matches placed one by one.
    let check = |cx: &mut gpui_kit::VisualTestContext| -> (Vec<Mark>, u32, bool) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let v = view.read(cx);
            let s = v.state().read(cx);
            assert_eq!(s.scroll_offset().y, gpui_kit::px(0.));
            let (_, scale) = track(s).expect("laid out");
            let rows = EditorRows {
                map: s.display_map(),
                text: s.text(),
            };
            let session = s.search_session();
            let found = if session.is_active() {
                session.matcher.matched_ranges().to_vec()
            } else {
                vec![]
            };
            let marks = v.code_map();
            assert_eq!(marks, naive(&bars, &found, &rows, scale));
            let wrapped = s.display_map().wrap_row_count() > s.display_map().buffer_line_count();
            (marks, scale.height, wrapped)
        })
    };
    let low = |marks: &[Mark], kind, height: u32| {
        marks
            .iter()
            .any(|m| m.kind == kind && m.start > height * 9 / 10)
    };

    let (marks, height, wrapped) = check(cx);
    assert!(!wrapped);
    assert!(
        marks
            .iter()
            .any(|m| m.kind == MarkKind::Added && m.start == 0)
    );
    assert!(marks.iter().any(|m| m.kind == MarkKind::Modified));
    assert!(low(&marks, MarkKind::Deleted, height));

    cx.update(|_, cx| gitviber_ui::Settings::update(cx, |s| s.word_wrap = true));
    let (marks, height, wrapped) = check(cx);
    assert!(wrapped);
    assert!(low(&marks, MarkKind::Deleted, height));

    cx.update(|window, cx| {
        let state = view.read(cx).state().clone();
        state.update(cx, |s, cx| s.insert("// a\n".repeat(5), window, cx));
    });
    check(cx);

    cx.update(|_, cx| {
        let state = view.read(cx).state().clone();
        state.update(cx, |s, cx| s.set_search_query("let v29", false, cx));
    });
    let (marks, height, _) = check(cx);
    assert!(low(&marks, MarkKind::Found, height));
    assert!(
        marks
            .iter()
            .any(|m| m.kind == MarkKind::Found && m.end < height / 5)
    );
    cx.update(|_, cx| gitviber_ui::Settings::update(cx, |s| s.word_wrap = false));
}

/// Blocks between lines (review notes): rows are left for them under their lines, as tall as
/// they measure; the caret steps over them; typing moves them with their lines; closing the view
/// gives everything back.
#[gpui_kit::test]
fn blocks_between_lines_take_rows_the_caret_steps_over(cx: &mut TestAppContext) {
    use crate::code::CodeBlock;
    use gpui_kit::component::input::{MoveDown, MoveUp, RopeExt as _};
    use gpui_kit::{Bounds, Pixels, px};
    use std::cell::Cell;
    use std::rc::Rc;

    setup(cx);
    let text: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    let view = cx.update(|window, cx| {
        let source = CodeSource {
            path: "src/lib.rs".into(),
            root: None,
        };
        let view = cx.new(|cx| {
            CodeView::new(
                gpui_kit::WeakEntity::<Workspace>::new_invalid(),
                source,
                text,
                None,
                vec![],
                false,
                window,
                cx,
            )
        });
        host.update(cx, |h, cx| {
            h.0 = Some(view.clone().into());
            cx.notify();
        });
        view
    });
    let drawn: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::default();
    let block = |id: &str, line: u32, tall: f32, drawn: &Rc<Cell<Option<Bounds<Pixels>>>>| {
        let drawn = Rc::downgrade(drawn);
        CodeBlock::new(id.to_string(), line, px(20.), move |_, _| {
            let drawn = drawn.clone();
            gpui_kit::canvas(
                move |bounds, _, _| {
                    if let Some(d) = drawn.upgrade() {
                        d.set(Some(bounds));
                    }
                },
                |_, _, _, _| {},
            )
            .w_full()
            .h(px(tall))
            .into_any_element()
        })
    };
    let other: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::default();
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.set_blocks(
                vec![block("a", 3, 50., &drawn), block("b", 10, 10., &other)],
                cx,
            )
        })
    });
    let frame = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    };
    // The first frame measures; the second has the rows it measured.
    frame(cx);
    frame(cx);
    let (lh, rows, gap3, gap10, top) = cx.update(|_, cx| {
        let s = view.read(cx).state().read(cx);
        let lh = s.line_height().unwrap();
        let map = s.display_map();
        (
            lh,
            map.display_row_count(),
            map.gap_rows(2),
            map.gap_rows(9),
            s.text_bounds().unwrap().origin.y,
        )
    });
    let want = (50. / f32::from(lh)).ceil() as usize;
    assert_eq!((gap3, gap10), (want, 1));
    assert_eq!(
        rows,
        41 + want + 1,
        "40 lines, the empty last one and the gaps"
    );
    // Under line 3's text (three rows down), centred in its rows.
    let at = drawn.get().expect("block drawn");
    let centred = top + lh * 3. + (lh * want as f32 - px(50.)) / 2.;
    assert!((at.origin.y - centred).abs() < px(0.5), "{at:?}");
    // Line 4's text is below the gap, where the block ends.
    let below = cx.update(|_, cx| {
        let s = view.read(cx).state().read(cx);
        s.display_map().buffer_line_to_display_row_range(3).unwrap()
    });
    assert_eq!(below.start, 3 + want);

    // ↓ from line 3 lands on line 4, ↑ comes back.
    let line_of = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| {
            let s = view.read(cx).state().read(cx);
            s.text().offset_to_point(s.cursor()).row + 1
        })
    };
    cx.update(|window, cx| {
        let state = view.read(cx).state().clone();
        let at = state.read(cx).text().line_start_offset(2);
        state.update(cx, |s, cx| s.set_selected_range(at..at, cx));
        view.read(cx).editor_focus(cx).focus(window, cx);
    });
    frame(cx);
    cx.dispatch_action(MoveDown);
    assert_eq!(line_of(cx), 4);
    cx.dispatch_action(MoveUp);
    assert_eq!(line_of(cx), 3);
    // A click on a block is the block's: the caret stays.
    cx.simulate_click(at.center(), gpui_kit::Modifiers::none());
    assert_eq!(line_of(cx), 3);
    // A click on the text below it is the editor's.
    let six = gpui_kit::point(at.center().x, top + lh * (5 + want) as f32 + lh / 2.);
    cx.simulate_click(six, gpui_kit::Modifiers::none());
    assert_eq!(line_of(cx), 6);
    // Add Review Note, once notes are on: the caret's line comes back.
    let asked: Rc<Cell<Option<(u32, u32)>>> = Rc::default();
    let _sub = cx.update(|_, cx| {
        let asked = asked.clone();
        view.update(cx, |v, _| v.set_notes(true));
        cx.subscribe(&view, move |_, e: &crate::CodeEvent, _| {
            if let crate::CodeEvent::AddNote { lines } = e {
                asked.set(Some(*lines));
            }
        })
    });
    cx.dispatch_action(gitviber_ui::keymap::review::AddNote);
    assert_eq!(asked.get(), Some((6, 6)));

    // A new first line moves both blocks down with their lines.
    cx.update(|window, cx| {
        let state = view.read(cx).state().clone();
        state.update(cx, |s, cx| {
            s.set_selected_range(0..0, cx);
            s.insert("new\n", window, cx);
        });
    });
    frame(cx);
    assert_eq!(cx.update(|_, cx| view.read(cx).block_lines(cx)), [4, 11]);
    // The file read again from disk: the blocks stay at their lines.
    cx.update(|window, cx| {
        let text: String = (1..=20).map(|i| format!("again {i}\n")).collect();
        view.update(cx, |v, cx| v.file_changed(text, None, vec![], window, cx));
    });
    frame(cx);
    assert_eq!(cx.update(|_, cx| view.read(cx).block_lines(cx)), [4, 11]);

    drop(view);
    cx.update(|window, cx| {
        host.update(cx, |h, cx| {
            h.0 = None;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
}
