//! The views against a vault in a temporary folder, listed in an obsidian.json under a temporary
//! $HOME: closing a note or a canvas tab gives everything it built back, a change core reports
//! is shown, ⌘S keeps the note's line endings, and the panel lists the vault as Obsidian would.

use std::path::{Path, PathBuf};

use gitviber_core::Event;
use gitviber_core::obsidian::VaultChanged;
use gitviber_ui::workspace::{PanelContext, TabItem, Workspace};
use gitviber_ui::{CoreEvents, memory};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{
    AnyView, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, WeakEntity, Window,
    div,
};

use crate::panel::VaultPanel;
use crate::vaults::Vaults;
use crate::view::{Mode, VaultTab};

struct Host(Option<AnyView>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The app's root context: the global commands' bindings (⌘F) ask for it.
        div()
            .size_full()
            .key_context(gitviber_ui::keymap::ROOT_CONTEXT)
            .children(self.0.clone())
    }
}

const HOME: &str = "---\ntags: [a, b]\n---\n# Home\n\nSee [[Plan#Goals]], [[Missing]] and ==this== #tag.\n\n![[Plan#Goals]]\n\n![[pic.png|40]]\n\n> [!tip] Tip\n> In a [[Plan|callout]].\n\n## Later\n\nEnd ^end\n";
const PLAN: &str = "# Plan\n\n## Goals\n\n- one\n- two ^b2\n\n## Later\n\n![[Home]]\n";
const BOARD: &str = r##"{"nodes":[
 {"id":"g","type":"group","x":-20,"y":-20,"width":700,"height":300,"label":"G"},
 {"id":"t","type":"text","x":0,"y":0,"width":250,"height":120,"text":"# Card\n[[Plan]] ==hi=="},
 {"id":"f","type":"file","x":300,"y":0,"width":250,"height":200,"file":"Plan.md","subpath":"#Goals"},
 {"id":"i","type":"file","x":0,"y":150,"width":120,"height":80,"file":"pic.png"},
 {"id":"l","type":"link","x":600,"y":0,"width":200,"height":60,"url":"https://example.com"}],
 "edges":[{"id":"e","fromNode":"t","toNode":"f","label":"x"},{"id":"e2","fromNode":"f","toNode":"l","fromEnd":"arrow"}]}"##;

/// A 2×2 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xc0,
    0xf0, 0x9f, 0x81, 0x81, 0x01, 0x48, 0x31, 0x30, 0x00, 0x00, 0x2a, 0xec, 0x05, 0xfb, 0x8b, 0x6a,
    0x12, 0x66, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A vault Obsidian lists (through $HOME), its config and state in temporary folders.
fn setup(name: &str, cx: &mut TestAppContext) -> String {
    let root = gitviber_ui::test_support::temp_dir(&format!("obsidian-{name}"));
    let vault = root.join("Vault");
    let write = |rel: &str, bytes: &[u8]| {
        let p = vault.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    };
    write("Home.md", HOME.as_bytes());
    write("Plan.md", PLAN.as_bytes());
    write("Board.canvas", BOARD.as_bytes());
    write("assets/pic.png", PNG);
    write("Folder/Deep.md", b"deep");
    write(".obsidian/app.json", b"{}");
    let vault = vault.canonicalize().unwrap().to_string_lossy().into_owned();
    let config = root.join("home/Library/Application Support/obsidian");
    std::fs::create_dir_all(&config).unwrap();
    let list = serde_json::json!({ "vaults": { "a": { "path": vault, "ts": 1 } } });
    std::fs::write(config.join("obsidian.json"), list.to_string()).unwrap();
    // SAFETY: each test runs in its own process (nextest), and only these tests read them.
    unsafe {
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("GITVIBER_CONFIG_DIR", root.join("config"));
    }
    cx.update(|cx| {
        gitviber_ui::init(cx);
        gitviber_ui::install_core(cx);
        gitviber_files::init(cx);
        crate::init(cx);
        Vaults::global(cx);
    });
    cx.run_until_parked();
    vault
}

fn tab(vault: &str, path: &str, window: &mut Window, cx: &mut gpui_kit::App) -> Entity<VaultTab> {
    cx.new(|cx| {
        VaultTab::new(
            WeakEntity::<Workspace>::new_invalid(),
            vault.into(),
            path.into(),
            window,
            cx,
        )
    })
}

fn show(host: &Entity<Host>, view: Option<AnyView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        host.update(cx, |h, cx| {
            h.0 = view;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn live(kind: &str) -> usize {
    memory::live_counts()
        .into_iter()
        .find(|(k, _)| *k == kind)
        .map_or(0, |(_, n)| n)
}

#[gpui_kit::test]
fn closing_note_and_canvas_tabs_gives_their_entities_back(cx: &mut TestAppContext) {
    let vault = setup("leak", cx);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    for round in 0..6 {
        let path = if round % 2 == 0 {
            "Home.md"
        } else {
            "Board.canvas"
        };
        let view = cx.update(|window, cx| tab(&vault, path, window, cx));
        show(&host, Some(view.clone().into()), cx);
        // It drew what it shows: the note's parts and embeds, the canvas's cards.
        cx.update(|_, cx| {
            let t = view.read(cx);
            if round % 2 == 0 {
                let note = t.note().expect("the note");
                assert!(note.read(cx).doc().built_count() >= 3);
                assert!(live("EmbedView") >= 1, "{:?}", memory::live_counts());
            } else {
                assert!(t.canvas().expect("the canvas").read(cx).built_cards() >= 2);
            }
        });
        drop(view);
        show(&host, None, cx);
        for kind in [
            "VaultTab",
            "NoteView",
            "CanvasView",
            "EmbedView",
            "MarkdownView",
            "BoundedImageCache",
        ] {
            assert_eq!(
                live(kind),
                0,
                "{kind} left after round {round}: {:?}",
                memory::live_counts()
            );
        }
    }
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
}

/// Writes `text` to `path` in the vault and says so as core's watcher would.
fn change(vault: &str, path: &str, text: &str, cx: &mut VisualTestContext) {
    std::fs::write(Path::new(vault).join(path), text).unwrap();
    cx.update(|_, cx| {
        let change = VaultChanged {
            vault: vault.to_string(),
            files: false,
            paths: vec![path.to_string()],
        };
        CoreEvents::global(cx).update(cx, |_, cx| cx.emit(Event::VaultChanged(change)));
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn an_open_note_follows_the_vaults_changes(cx: &mut TestAppContext) {
    let vault = setup("follow", cx);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Plan.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    let source = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| view.read(cx).note().unwrap().read(cx).source().to_string())
    };
    assert_eq!(source(cx), PLAN);
    // Another file's change isn't this one's.
    change(&vault, "Home.md", "other", cx);
    assert_eq!(source(cx), PLAN);
    change(&vault, "Plan.md", "# Plan\n\nNew text\n", cx);
    assert_eq!(source(cx), "# Plan\n\nNew text\n");
    show(&host, Some(view.clone().into()), cx);
}

#[gpui_kit::test]
fn saving_a_note_keeps_its_line_endings(cx: &mut TestAppContext) {
    let vault = setup("save", cx);
    std::fs::write(Path::new(&vault).join("Crlf.md"), "one\r\ntwo\r\n").unwrap();
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Crlf.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    cx.update(|window, cx| view.update(cx, |t, cx| t.set_mode(Mode::Code, window, cx)));
    show(&host, Some(view.clone().into()), cx);
    let editor = cx.update(|_, cx| view.read(cx).editor().expect("the code view"));
    // Typed, as the keyboard types: select all, then the new text.
    cx.update(|window, cx| {
        let focus = editor.read(cx).editor_focus(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("one\nthree\n");
    assert!(cx.update(|_, cx| editor.read(cx).is_dirty()));
    cx.update(|window, cx| editor.update(cx, |e, cx| e.save(window, cx)));
    cx.run_until_parked();
    let file: PathBuf = Path::new(&vault).join("Crlf.md");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\r\nthree\r\n");
    assert!(!cx.update(|_, cx| editor.read(cx).is_dirty()));
    // Back to the rendered note: it shows what was saved.
    cx.update(|window, cx| view.update(cx, |t, cx| t.set_mode(Mode::Preview, window, cx)));
    show(&host, Some(view.clone().into()), cx);
    let shown = cx.update(|_, cx| view.read(cx).note().unwrap().read(cx).source().to_string());
    assert_eq!(shown, "one\r\nthree\r\n");
}

#[gpui_kit::test]
fn the_panel_lists_the_vault_as_obsidian_does(cx: &mut TestAppContext) {
    setup("panel", cx);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let ctx = PanelContext {
        workspace: WeakEntity::new_invalid(),
        project: None,
    };
    let panel = cx.update(|window, cx| cx.new(|cx| VaultPanel::new(&ctx, window, cx)));
    show(&host, Some(panel.clone().into()), cx);
    let rows = cx.update(|_, cx| panel.read(cx).row_paths());
    // Folders first, nothing hidden.
    assert_eq!(
        rows,
        ["assets", "Folder", "Board.canvas", "Home.md", "Plan.md"]
    );
    // A vault turned off in Settings isn't shown, nor watched.
    cx.update(|_, cx| {
        let vaults: Vec<String> = Vaults::global(cx)
            .read(cx)
            .vaults(cx)
            .into_iter()
            .map(|v| v.path)
            .collect();
        gitviber_ui::Settings::update(cx, |s| s.hidden_vaults = vaults);
    });
    show(&host, Some(panel.clone().into()), cx);
    assert!(cx.update(|_, cx| panel.read(cx).row_paths()).is_empty());
    drop(panel);
    show(&host, None, cx);
    assert_eq!(live("VaultPanel"), 0);
}

/// A window whose root is gpui-kit's (it draws dialogs) showing a code view of `note`.
fn code_view<'a>(
    vault: &str,
    note: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<VaultTab>, &'a mut VisualTestContext) {
    let mut view = None;
    let handle = cx.add_window(|window, cx| {
        let v = tab(vault, note, window, cx);
        view = Some(v.clone());
        gpui_kit::base::Root::new(v, window, cx)
    });
    let cx = VisualTestContext::from_window(*handle, cx).into_mut();
    let view = view.unwrap();
    cx.run_until_parked();
    cx.update(|window, cx| view.update(cx, |t, cx| t.set_mode(Mode::Code, window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx)
}

/// Types over the whole text, as the keyboard would.
fn type_all(view: &Entity<VaultTab>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let focus = view
            .read(cx)
            .editor()
            .expect("the code view")
            .read(cx)
            .editor_focus(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
}

fn dirty(view: &Entity<VaultTab>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| gitviber_ui::workspace::TabItem::is_dirty(view.read(cx), cx))
}

fn save(view: &Entity<VaultTab>, cx: &mut VisualTestContext) {
    let editor = cx.update(|_, cx| view.read(cx).editor().unwrap());
    cx.update(|window, cx| editor.update(cx, |e, cx| e.save(window, cx)));
}

#[gpui_kit::test]
fn a_lossy_note_stays_read_only_whichever_way_it_gets_to_code(cx: &mut TestAppContext) {
    let vault = setup("lossy", cx);
    let file = Path::new(&vault).join("Latin.md");
    // Latin-1: not UTF-8, so what's shown isn't the file's own bytes.
    std::fs::write(&file, b"caf\xe9 au lait\n").unwrap();
    let (view, cx) = code_view(&vault, "Latin.md", cx);
    let readonly = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| view.read(cx).editor().unwrap().read(cx).is_readonly())
    };
    assert!(readonly(cx));
    // Preview, then Code again: the editor is made again from the rendered note's text.
    for mode in [Mode::Preview, Mode::Code] {
        cx.update(|window, cx| view.update(cx, |t, cx| t.set_mode(mode, window, cx)));
        cx.run_until_parked();
    }
    assert!(readonly(cx));
    type_all(&view, "mine\n", cx);
    save(&view, cx);
    cx.run_until_parked();
    assert_eq!(std::fs::read(&file).unwrap(), b"caf\xe9 au lait\n");
    // The tab can't save it, so the ask before closing offers only to drop it.
    assert!(!cx.update(|_, cx| TabItem::can_save(view.read(cx), cx)));
    assert!(cx.update(|window, cx| view.update(cx, |t, cx| {
        gitviber_ui::workspace::TabItem::save(t, window, cx).is_none()
    })));
}

#[gpui_kit::test]
fn an_unsaved_note_asks_and_dont_save_puts_it_back(cx: &mut TestAppContext) {
    let vault = setup("discard", cx);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Plan.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    let mode = |m: Mode, cx: &mut VisualTestContext| {
        cx.update(|window, cx| view.update(cx, |t, cx| t.set_mode(m, window, cx)));
        show(&host, Some(view.clone().into()), cx);
    };
    let shown = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| view.read(cx).note().unwrap().read(cx).source().to_string())
    };
    mode(Mode::Code, cx);
    assert!(!dirty(&view, cx));
    type_all(&view, "typed\n", cx);
    // The close and quit guards ask about it, and may offer Save.
    assert!(dirty(&view, cx));
    assert!(cx.update(|_, cx| TabItem::can_save(view.read(cx), cx)));
    mode(Mode::Preview, cx);
    assert_eq!(shown(cx), "typed\n");
    // Don't Save: clean, the note as the file has it, nothing kept for next time.
    cx.update(|_, cx| view.update(cx, TabItem::discard));
    assert!(!dirty(&view, cx));
    assert_eq!(shown(cx), PLAN);
    drop(view);
    show(&host, None, cx);
    let view = cx.update(|window, cx| tab(&vault, "Plan.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    assert!(!dirty(&view, cx));
    let source = cx.update(|_, cx| view.read(cx).note().unwrap().read(cx).source().to_string());
    assert_eq!(source, PLAN);
    assert_eq!(
        std::fs::read_to_string(Path::new(&vault).join("Plan.md")).unwrap(),
        PLAN
    );
}

#[gpui_kit::test]
fn saving_over_a_change_made_on_disk_asks_first(cx: &mut TestAppContext) {
    let vault = setup("outside", cx);
    let file = Path::new(&vault).join("Plan.md");
    let (view, cx) = code_view(&vault, "Plan.md", cx);
    type_all(&view, "mine\n", cx);
    // Obsidian (or a sync) writes the note meanwhile; no watcher tells this window.
    std::fs::write(&file, "theirs\n").unwrap();
    save(&view, cx);
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "theirs\n");
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "it asks before overwriting"
    );
    assert!(dirty(&view, cx));
}

#[gpui_kit::test]
fn typing_undone_is_clean_and_a_second_save_waits_for_the_first(cx: &mut TestAppContext) {
    let vault = setup("undo", cx);
    let file = Path::new(&vault).join("Plan.md");
    let (view, cx) = code_view(&vault, "Plan.md", cx);
    type_all(&view, &format!("{PLAN}x"), cx);
    assert!(dirty(&view, cx));
    cx.simulate_keystrokes("backspace");
    assert!(!dirty(&view, cx), "back to the file's text is no edit");
    // ⌘S, typing and ⌘S again before the first write lands: the second waits, so the older text
    // can't be written last.
    let editor = cx.update(|_, cx| view.read(cx).editor().unwrap());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.type_now("first\n", window, cx);
            e.save(window, cx);
            e.type_now("second\n", window, cx);
            e.save(window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "first\n");
    assert!(
        dirty(&view, cx),
        "what was typed during the write is still unsaved"
    );
    save(&view, cx);
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "second\n");
    assert!(!dirty(&view, cx));
}

#[gpui_kit::test]
fn unsaved_typing_outlives_its_tab(cx: &mut TestAppContext) {
    let vault = setup("keep", cx);
    let file = Path::new(&vault).join("Plan.md");
    let edits_file = gitviber_ui::store::config_dir().join("vault-edits.json");
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Plan.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    cx.update(|window, cx| view.update(cx, |t, cx| t.set_mode(Mode::Code, window, cx)));
    show(&host, Some(view.clone().into()), cx);
    type_all(&view, "kept\n", cx);
    // The tab goes (closed, a repo switch, a quit): its view with it.
    drop(view);
    show(&host, None, cx);
    assert_eq!(live("NoteEditor"), 0);
    cx.update(|_, cx| gitviber_ui::store::flush(cx));
    let stored = std::fs::read_to_string(&edits_file).unwrap();
    assert!(stored.contains("kept"), "{stored}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        PLAN,
        "nothing was written to the note"
    );
    // Opened again (in Code, as the last note was left): dirty, the typing back in the editor
    // and in the rendered note.
    let view = cx.update(|window, cx| tab(&vault, "Plan.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    assert!(dirty(&view, cx));
    let typed = cx.update(|_, cx| view.read(cx).editor().unwrap().read(cx).content(cx));
    assert_eq!(typed, "kept\n");
    cx.update(|window, cx| view.update(cx, |t, cx| t.set_mode(Mode::Preview, window, cx)));
    show(&host, Some(view.clone().into()), cx);
    let shown = cx.update(|_, cx| view.read(cx).note().unwrap().read(cx).source().to_string());
    assert_eq!(shown, "kept\n");
    assert!(dirty(&view, cx), "the typing stays in Preview");
    // Don't Save drops it for good.
    cx.update(|_, cx| view.update(cx, gitviber_ui::workspace::TabItem::discard));
    assert!(!dirty(&view, cx));
    drop(view);
    show(&host, None, cx);
    cx.update(|_, cx| gitviber_ui::store::flush(cx));
    let stored = std::fs::read_to_string(&edits_file).unwrap();
    assert!(!stored.contains("kept"), "{stored}");
}

#[gpui_kit::test]
fn find_in_a_rendered_note(cx: &mut TestAppContext) {
    let vault = setup("find", cx);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Home.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    let note = cx.update(|_, cx| view.read(cx).note().expect("the note"));
    let hits = |cx: &mut VisualTestContext| cx.update(|_, cx| note.read(cx).find_hits());
    // The built parts' marks: (part, (current, count)).
    let marks = |cx: &mut VisualTestContext| cx.update(|_, cx| note.read(cx).doc().marks(cx));
    let current = |cx: &mut VisualTestContext| {
        let marks = marks(cx);
        marks.iter().filter(|(_, (at, _))| at.is_some()).count()
    };
    let focus = |cx: &mut VisualTestContext, handle: FocusHandle| {
        cx.update(|window, cx| window.focus(&handle, cx));
        cx.run_until_parked();
    };
    // ⌘F where the workspace puts focus on showing the tab.
    let shown = cx.update(|_, cx| TabItem::tab_focus(view.read(cx), cx).unwrap());
    focus(cx, shown);
    cx.simulate_keystrokes("cmd-f");
    assert_eq!(hits(cx), Some((0, 0)));
    // As it reads: "[[Plan#Goals]]" shows "Plan > Goals", "## Later" shows "Later".
    cx.simulate_input("goals");
    assert_eq!(hits(cx), Some((0, 1)));
    // The box's toggles on their keys: Match Case finds no "goals" in "Goals".
    let case = if cfg!(target_os = "macos") {
        "alt-cmd-c"
    } else {
        "alt-c"
    };
    cx.simulate_keystrokes(case);
    assert_eq!(hits(cx), Some((0, 0)));
    cx.simulate_keystrokes(case);
    assert_eq!(hits(cx), Some((0, 1)));
    // Washed where it reads, as the current match.
    assert!(
        marks(cx).iter().any(|(_, m)| *m == (Some(0), 1)),
        "{:?}",
        marks(cx)
    );
    // ⌘F again selects the query, so what's typed next replaces it.
    cx.simulate_keystrokes("cmd-f");
    cx.simulate_input("e");
    let (_, n) = hits(cx).unwrap();
    assert!(n > 3, "{n}");
    cx.simulate_keystrokes("enter");
    assert_eq!(hits(cx), Some((1, n)));
    assert_eq!(current(cx), 1, "{:?}", marks(cx));
    cx.simulate_keystrokes("shift-enter shift-enter");
    assert_eq!(hits(cx), Some((n - 1, n)), "round to the last");
    cx.simulate_keystrokes("escape");
    assert_eq!(hits(cx), None);
    assert!(
        marks(cx).iter().all(|(_, m)| *m == (None, 0)),
        "{:?}",
        marks(cx)
    );
    // The tab's own focus (its header clicked) finds in the note too.
    let own = cx.update(|_, cx| view.read(cx).own_focus());
    focus(cx, own);
    cx.simulate_keystrokes("cmd-f");
    assert_eq!(
        hits(cx),
        Some((n - 1, n)),
        "the query and its match are kept"
    );
}

#[gpui_kit::test]
fn a_link_to_a_block_scrolls_to_the_block(cx: &mut TestAppContext) {
    let vault = setup("block", cx);
    // No headings: the whole note is one part, its block near the end.
    let long = format!("{}para ^b1\n", "line\n\n".repeat(300));
    std::fs::write(Path::new(&vault).join("Long.md"), long).unwrap();
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Long.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    let note = cx.update(|_, cx| view.read(cx).note().expect("the note"));
    cx.update(|_, cx| note.update(cx, |n, cx| n.reveal("^b1", cx)));
    show(&host, Some(view.clone().into()), cx);
    let top = cx.update(|_, cx| note.read(cx).list().logical_scroll_top());
    assert_eq!(top.item_ix, 0);
    assert!(top.offset_in_item > gpui_kit::px(1000.), "{top:?}");
}

#[gpui_kit::test]
fn an_svgs_preview_shows_what_was_typed(cx: &mut TestAppContext) {
    let vault = setup("svg", cx);
    let on_disk = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    std::fs::write(Path::new(&vault).join("Pic.svg"), on_disk).unwrap();
    let (view, cx) = code_view(&vault, "Pic.svg", cx);
    let typed = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><circle r="5" cx="5" cy="5"/></svg>"#;
    type_all(&view, typed, cx);
    cx.update(|window, cx| view.update(cx, |t, cx| t.set_mode(Mode::Preview, window, cx)));
    cx.run_until_parked();
    let svg = cx.update(|_, cx| view.read(cx).svg().expect("the SVG preview"));
    assert_eq!(cx.update(|_, cx| svg.read(cx).source().to_string()), typed);
    assert!(dirty(&view, cx), "the typing is still unsaved");
    assert_eq!(
        std::fs::read_to_string(Path::new(&vault).join("Pic.svg")).unwrap(),
        on_disk
    );
}

#[gpui_kit::test]
fn media_embedded_in_a_note_plays_in_place_and_goes_with_it(cx: &mut TestAppContext) {
    let vault = setup("media", cx);
    std::fs::write(Path::new(&vault).join("clip.mp4"), b"not really a video").unwrap();
    std::fs::write(Path::new(&vault).join("doc.pdf"), b"%PDF-1.1\n").unwrap();
    std::fs::write(
        Path::new(&vault).join("Media.md"),
        "# Media\n\n![[clip.mp4|320x180]]\n\n![[doc.pdf]]\n",
    )
    .unwrap();
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Media.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    assert_eq!(live("MediaView"), 2, "{:?}", memory::live_counts());
    drop(view);
    show(&host, None, cx);
    assert_eq!(live("MediaView"), 0);
}

#[gpui_kit::test]
fn an_embedded_pdf_or_video_is_made_again_only_when_its_file_is_overwritten(
    cx: &mut TestAppContext,
) {
    let vault = setup("media-reload", cx);
    std::fs::write(Path::new(&vault).join("clip.mp4"), b"not really a video").unwrap();
    std::fs::write(Path::new(&vault).join("doc.pdf"), b"%PDF-1.1\n").unwrap();
    std::fs::write(
        Path::new(&vault).join("Media.md"),
        "# Media\n\n![[clip.mp4|320x180]]\n\n![[doc.pdf]]\n",
    )
    .unwrap();
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Media.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    let shown = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let note = view.read(cx).note().expect("the note");
            note.read(cx)
                .doc()
                .embeds()
                .iter()
                .map(|e| e.read(cx).media().map(|m| m.entity_id()))
                .collect::<Vec<_>>()
        })
    };
    let first = shown(cx);
    assert!(
        first.len() == 2 && first.iter().all(Option::is_some),
        "{first:?}"
    );
    // Another file changed, then the vault read again whole with nothing new: both play on.
    change(&vault, "Home.md", "other", cx);
    cx.update(|_, cx| {
        let change = VaultChanged {
            vault: vault.clone(),
            files: true,
            paths: vec![],
        };
        CoreEvents::global(cx).update(cx, |_, cx| cx.emit(Event::VaultChanged(change)));
    });
    show(&host, Some(view.clone().into()), cx);
    assert_eq!(shown(cx), first);
    // The PDF overwritten: its viewer is made again, the video's left playing.
    change(&vault, "doc.pdf", "%PDF-1.4\n% a longer one\n", cx);
    show(&host, Some(view.clone().into()), cx);
    let now = shown(cx);
    assert_eq!(now[0], first[0]);
    assert!(now[1].is_some() && now[1] != first[1], "{now:?} {first:?}");
    assert_eq!(live("MediaView"), 2, "{:?}", memory::live_counts());
}

#[gpui_kit::test]
fn cmd_g_steps_through_a_notes_matches_and_goes_on_without_its_box(cx: &mut TestAppContext) {
    use gitviber_ui::keymap::find;
    use std::cell::Cell;
    use std::rc::Rc;
    let vault = setup("find-next", cx);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Home.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    let note = cx.update(|_, cx| view.read(cx).note().expect("the note"));
    let hits = |cx: &mut VisualTestContext| cx.update(|_, cx| note.read(cx).find_hits());
    // What a key gone on reaches: the app's own handler.
    let went_on = Rc::new(Cell::new(0));
    let (next_seen, prev_seen) = (went_on.clone(), went_on.clone());
    cx.update(|_, cx| {
        cx.on_action(move |_: &find::Next, _| next_seen.set(next_seen.get() + 1));
        cx.on_action(move |_: &find::Prev, _| prev_seen.set(prev_seen.get() + 1));
    });
    let shown = cx.update(|_, cx| TabItem::tab_focus(view.read(cx), cx).unwrap());
    cx.update(|window, cx| window.focus(&shown, cx));
    cx.simulate_keystrokes("cmd-f");
    cx.simulate_input("e");
    let (_, n) = hits(cx).unwrap();
    assert!(n > 3, "{n}");
    // From the box, and from the note itself.
    let (next, prev) = if cfg!(target_os = "macos") {
        ("cmd-g", "shift-cmd-g")
    } else {
        ("f3", "shift-f3")
    };
    cx.simulate_keystrokes(next);
    assert_eq!(hits(cx), Some((1, n)));
    let own = cx.update(|_, cx| note.read(cx).focus_handle().clone());
    cx.update(|window, cx| window.focus(&own, cx));
    cx.simulate_keystrokes(next);
    assert_eq!(hits(cx), Some((2, n)));
    cx.simulate_keystrokes(prev);
    cx.simulate_keystrokes(prev);
    cx.simulate_keystrokes(prev);
    assert_eq!(hits(cx), Some((n - 1, n)), "round to the last");
    assert_eq!(went_on.get(), 0, "the box took them");
    // Closed, the keys go on (⇧⌘G to the Git panel).
    cx.update(|window, cx| window.focus(&own, cx));
    cx.simulate_keystrokes("cmd-f escape");
    assert_eq!(hits(cx), None);
    cx.simulate_keystrokes(next);
    cx.simulate_keystrokes(prev);
    assert_eq!(went_on.get(), 2);
}

#[gpui_kit::test]
fn a_long_note_scrolls_from_the_keyboard_but_not_from_its_find_box(cx: &mut TestAppContext) {
    use gitviber_ui::keymap::scroll;
    let vault = setup("scroll-keys", cx);
    // A part a heading, the ones far down not measured until they come near.
    let long: String = (1..=12)
        .map(|i| {
            format!(
                "## Part {i}\n\n{}",
                "A paragraph of words to read.\n\n".repeat(15)
            )
        })
        .collect();
    std::fs::write(Path::new(&vault).join("Long.md"), long).unwrap();
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let view = cx.update(|window, cx| tab(&vault, "Long.md", window, cx));
    show(&host, Some(view.clone().into()), cx);
    let note = cx.update(|_, cx| view.read(cx).note().expect("the note"));
    let shown = cx.update(|_, cx| TabItem::tab_focus(view.read(cx), cx).unwrap());
    cx.update(|window, cx| window.focus(&shown, cx));
    // How far it's scrolled, how far it can be, the view's height, and the last part in view.
    let scrolled = |cx: &mut VisualTestContext| {
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        note.read_with(cx, |n, _| {
            let list = n.list();
            let view = list.viewport_bounds();
            let last = list
                .bounds_for_item(n.doc().len() - 1)
                .is_some_and(|b| b.intersects(&view));
            (
                f32::from(-list.scroll_px_offset_for_scrollbar().y),
                f32::from(list.max_offset_for_scrollbar().y),
                f32::from(view.size.height),
                last,
            )
        })
    };
    let (_, _, height, _) = scrolled(cx);
    assert!(height > 100., "{height}");
    cx.simulate_keystrokes("pagedown");
    let (top, ..) = scrolled(cx);
    assert!((top - height * scroll::PAGE).abs() < 0.5, "{top} {height}");
    cx.simulate_keystrokes("end");
    let (top, max, _, last) = scrolled(cx);
    assert!(
        last && top > height && (top - max).abs() < 0.5,
        "{top} of {max}"
    );
    // In the find box, they stay there.
    cx.simulate_keystrokes("cmd-f home pageup up");
    assert_eq!(scrolled(cx).0, top);
    cx.update(|window, cx| window.focus(&shown, cx));
    cx.simulate_keystrokes("home");
    assert_eq!(scrolled(cx).0, 0.);
}
