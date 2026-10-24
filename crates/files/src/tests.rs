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
