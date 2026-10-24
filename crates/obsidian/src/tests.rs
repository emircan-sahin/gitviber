//! The views against a vault in a temporary folder, listed in an obsidian.json under a temporary
//! $HOME: closing a note or a canvas tab gives everything it built back, a change core reports
//! is shown, ⌘S keeps the note's line endings, and the panel lists the vault as Obsidian would.

use std::path::{Path, PathBuf};

use gitviber_core::Event;
use gitviber_core::obsidian::VaultChanged;
use gitviber_ui::workspace::{PanelContext, Workspace};
use gitviber_ui::{CoreEvents, memory};
use gpui_kit::{
    AnyView, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, TestAppContext, VisualTestContext, WeakEntity, Window, div,
};

use crate::panel::VaultPanel;
use crate::vaults::Vaults;
use crate::view::{Mode, VaultTab};

struct Host(Option<AnyView>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.0.clone())
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
    let root =
        std::env::temp_dir().join(format!("gitviber-obsidian-{name}-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&root);
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
