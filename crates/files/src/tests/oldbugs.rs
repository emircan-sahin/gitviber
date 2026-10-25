//! Bugs the old app fixed, checked in the native views: ⌘S asking before it writes over a file
//! changed on disk, ⌥Z in the code, the old editor's keys, ⌘G / ⇧⌘G in find, Space paging
//! read-only code, a long name in the header, and the explorer's ⇧-range anchor.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gitviber_ui::keymap;
use gpui_kit::{Entity, InteractiveElement as _, Task, VisualTestContext, px, size};

use super::*;

/// Under the window root's key context, as the app draws everything; counts Focus Git Panel.
struct Root(Option<AnyView>, Rc<Cell<u32>>);

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let git = self.1.clone();
        div()
            .key_context(keymap::ROOT_CONTEXT)
            .on_action(move |_: &keymap::view::FocusGitPanel, _, _| git.set(git.get() + 1))
            .size_full()
            .children(self.0.clone())
    }
}

/// What a save answers, once the test runs it to the end.
fn outcome(cx: &mut VisualTestContext, task: Task<bool>) -> Rc<Cell<Option<bool>>> {
    let out = Rc::new(Cell::new(None));
    let o = out.clone();
    cx.update(|_, cx| cx.spawn(async move |_| o.set(Some(task.await))).detach());
    out
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// The macOS chord, or the one elsewhere.
fn key(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

/// A code view of `text` under the root, its editor focused; and the Focus Git Panel count.
fn code_view<'a>(
    cx: &'a mut TestAppContext,
    path: &str,
    text: &str,
    readonly: bool,
) -> (Entity<CodeView>, Rc<Cell<u32>>, &'a mut VisualTestContext) {
    let git = Rc::new(Cell::new(0));
    let (host, cx) = cx.add_window_view(|_, _| Root(None, git.clone()));
    let code = cx.update(|window, cx| {
        let source = CodeSource {
            path: path.into(),
            root: None,
        };
        let text = text.to_string();
        let code = cx.new(|cx| {
            CodeView::new(
                gpui_kit::WeakEntity::<Workspace>::new_invalid(),
                source,
                text,
                None,
                vec![],
                readonly,
                window,
                cx,
            )
        });
        host.update(cx, |h, cx| {
            h.0 = Some(code.clone().into());
            cx.notify();
        });
        code
    });
    draw(cx);
    cx.update(|window, cx| code.read(cx).editor_focus(cx).focus(window, cx));
    draw(cx);
    (code, git, cx)
}

fn text_of(cx: &mut VisualTestContext, code: &Entity<CodeView>) -> String {
    cx.update(|_, cx| code.read(cx).text(cx).to_string())
}

fn ranges(cx: &mut VisualTestContext, code: &Entity<CodeView>) -> Vec<Range<usize>> {
    cx.update(|_, cx| code.read(cx).state().read(cx).selected_ranges())
}

fn put(cx: &mut VisualTestContext, code: &Entity<CodeView>, at: Range<usize>) {
    cx.update(|_, cx| {
        let state = code.read(cx).state().clone();
        state.update(cx, |s, cx| s.set_selected_range(at, cx));
    });
}

/// The active tab's file view.
fn active_file(cx: &mut VisualTestContext, ws: &Entity<Workspace>) -> Entity<crate::FileView> {
    cx.update(|_, cx| {
        let view = ws
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle())
            .unwrap()
            .view();
        view.downcast::<crate::FileView>().unwrap()
    })
}

/// A repo with `files` open in a workspace drawn in the window.
fn open_repo<'a>(
    cx: &'a mut TestAppContext,
    repo: &std::path::Path,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    cx.update(gitviber_ui::install_core);
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.to_path_buf(), window, cx));
    });
    cx.run_until_parked();
    (ws, cx)
}

/// ⌘S checks the disk first: what an agent wrote since the typing began is overwritten only
/// once the user says so, and the header says the file changed meanwhile. The typing is never
/// dropped for it; a disk that comes to hold what's typed makes the file clean.
#[gpui_kit::test]
fn saving_asks_before_writing_over_a_file_changed_on_disk(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("ondisk");
    let file = repo.join("src/b.rs");
    let (ws, cx) = open_repo(cx, &repo);
    let weak = ws.downgrade();
    cx.update(|window, cx| crate::open_file(&weak, "src/b.rs", Default::default(), window, cx));
    cx.run_until_parked();
    draw(cx);
    let view = active_file(cx, &ws);
    let code = cx.update(|_, cx| view.read(cx).code().expect("the text shows"));
    let type_in = |cx: &mut VisualTestContext, text: &str| {
        cx.update(|window, cx| {
            let state = code.read(cx).state().clone();
            state.update(cx, |s, cx| {
                s.set_selected_range(0..0, cx);
                s.insert(text.to_string(), window, cx);
            });
        });
        cx.run_until_parked();
    };
    let flags = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let c = code.read(cx);
            (c.is_dirty(), c.changed_on_disk())
        })
    };
    let save = |cx: &mut VisualTestContext| {
        let task = cx.update(|window, cx| code.update(cx, |c, cx| c.save_task(window, cx)));
        let saved = outcome(cx, task.expect("a repo file saves"));
        cx.run_until_parked();
        saved
    };
    let disk = || std::fs::read_to_string(&file).unwrap();

    type_in(cx, "mine ");
    assert_eq!(flags(cx), (true, false));
    // An agent writes the file meanwhile: the typing stays, and the header says so.
    std::fs::write(&file, "agent\n").unwrap();
    cx.update(|window, cx| {
        code.update(cx, |c, cx| {
            c.file_changed("agent\n".into(), None, vec![], window, cx)
        })
    });
    assert_eq!(text_of(cx, &code), "mine b\n");
    assert_eq!(flags(cx), (true, true));

    // ⌘S asks; Cancel leaves the agent's file.
    let task = save(cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(task.get(), Some(false));
    assert_eq!(disk(), "agent\n");
    assert_eq!(flags(cx), (true, true));
    // Overwrite writes the typing.
    let task = save(cx);
    cx.simulate_prompt_answer("Overwrite");
    cx.run_until_parked();
    assert_eq!(task.get(), Some(true));
    assert_eq!(disk(), "mine b\n");
    assert_eq!(flags(cx), (false, false));

    // Nothing changed on disk since: no ask.
    type_in(cx, "more ");
    let task = save(cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(task.get(), Some(true));
    assert_eq!(disk(), "more mine b\n");

    // The disk comes to hold what's typed: nothing left to save.
    type_in(cx, "x");
    std::fs::write(&file, "xmore mine b\n").unwrap();
    cx.update(|window, cx| {
        code.update(cx, |c, cx| {
            c.file_changed("xmore mine b\n".into(), None, vec![], window, cx)
        })
    });
    assert_eq!(flags(cx), (false, false));

    // The header's Reload drops the typing, once the user says so, for the file on disk.
    type_in(cx, "y");
    std::fs::write(&file, "agent again\n").unwrap();
    cx.update(|window, cx| {
        code.update(cx, |c, cx| {
            c.file_changed("agent again\n".into(), None, vec![], window, cx)
        })
    });
    assert_eq!(flags(cx), (true, true));
    cx.update(|window, cx| view.update(cx, |v, cx| v.reload(window, cx)));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Discard Changes");
    cx.run_until_parked();
    assert_eq!(text_of(cx, &code), "agent again\n");
    assert_eq!(flags(cx), (false, false));
    _ = std::fs::remove_dir_all(&repo);
}

/// ⌥Z wraps the code where focus is in it; where ⌥Z types a letter of the layout's own
/// alphabet (Polish ż) the letter is typed instead, as in any field.
#[gpui_kit::test]
fn alt_z_wraps_the_code_and_a_letter_it_types_stays_typed(cx: &mut TestAppContext) {
    setup(cx);
    let (code, _, cx) = code_view(cx, "src/lib.rs", "fn main() {}\n", false);
    let wrap =
        |cx: &mut VisualTestContext| cx.update(|_, cx| gitviber_ui::Settings::get(cx).word_wrap);
    let before = wrap(cx);
    // A US layout's ⌥Z types Ω.
    cx.simulate_keystrokes("alt-z->Ω");
    assert_eq!(wrap(cx), !before);
    assert_eq!(text_of(cx, &code), "fn main() {}\n");
    cx.simulate_keystrokes("alt-z->ż");
    assert_eq!(wrap(cx), !before);
    assert_eq!(text_of(cx, &code), "żfn main() {}\n");
}

/// The old editor's keys in a file being edited: ⌘/, ⌥↓ (one ⌘Z back), ⇧⌘K, ⌘D, ⌘L, ⌘↵ (not Go
/// to Definition, whichever binding the keymap put first), ⇧⌘\ and the framed bracket, Space.
#[gpui_kit::test]
fn the_old_editors_keys_edit_the_code(cx: &mut TestAppContext) {
    setup(cx);
    let text = "fn a() {\n    let x = 1;\n    let y = x;\n}\n";
    let (code, _, cx) = code_view(cx, "src/lib.rs", text, false);
    let caret = |cx: &mut VisualTestContext| ranges(cx, &code)[0].start;

    put(cx, &code, 13..13);
    cx.simulate_keystrokes(key("cmd-/", "ctrl-/"));
    assert_eq!(
        text_of(cx, &code),
        "fn a() {\n    // let x = 1;\n    let y = x;\n}\n"
    );
    cx.simulate_keystrokes(key("cmd-/", "ctrl-/"));
    assert_eq!(text_of(cx, &code), text);

    cx.simulate_keystrokes("alt-down");
    assert_eq!(
        text_of(cx, &code),
        "fn a() {\n    let y = x;\n    let x = 1;\n}\n"
    );
    cx.simulate_keystrokes(key("cmd-z", "ctrl-z"));
    assert_eq!(text_of(cx, &code), text);

    put(cx, &code, 13..13);
    cx.simulate_keystrokes(key("shift-cmd-k", "ctrl-shift-k"));
    assert_eq!(text_of(cx, &code), "fn a() {\n    let y = x;\n}\n");
    cx.simulate_keystrokes(key("cmd-z", "ctrl-z"));
    assert_eq!(text_of(cx, &code), text);

    // ⌘D: the word at the caret, then the next "x" as a word (not inside another).
    put(cx, &code, 17..17);
    cx.simulate_keystrokes(key("cmd-d", "ctrl-d"));
    assert_eq!(ranges(cx, &code), vec![17..18]);
    cx.simulate_keystrokes(key("cmd-d", "ctrl-d"));
    assert_eq!(ranges(cx, &code), vec![17..18, 36..37]);

    put(cx, &code, 13..13);
    cx.simulate_keystrokes(key("cmd-l", "ctrl-l"));
    assert_eq!(ranges(cx, &code), vec![9..24]);

    // ⌘↵: a line below, indented, the caret on it.
    let below = "fn a() {\n    let x = 1;\n    \n    let y = x;\n}\n";
    put(cx, &code, 13..13);
    cx.simulate_keystrokes(key("cmd-enter", "ctrl-enter"));
    assert_eq!(text_of(cx, &code), below);
    assert_eq!(caret(cx), 28);
    cx.simulate_keystrokes(key("cmd-z", "ctrl-z"));
    // A rebinding installs the keymap again, Go to Definition's ⌘↵ then ahead of the editor's.
    cx.update(|_, cx| keymap::install(cx));
    put(cx, &code, 13..13);
    cx.simulate_keystrokes(key("cmd-enter", "ctrl-enter"));
    assert_eq!(text_of(cx, &code), below);
    cx.simulate_keystrokes(key("cmd-z", "ctrl-z"));
    assert_eq!(text_of(cx, &code), text);

    // ⇧⌘\ (⌘| from macOS) to the matching bracket and back; the pair at the caret is framed.
    let jump = key("cmd-|", "ctrl-shift-\\");
    put(cx, &code, 8..8);
    cx.simulate_keystrokes(jump);
    assert_eq!(caret(cx), 40);
    cx.simulate_keystrokes(jump);
    assert_eq!(caret(cx), 8);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    draw(cx);
    assert_eq!(
        cx.update(|_, cx| code.read(cx).bracket_frames(cx)),
        vec![7..8, 39..40]
    );

    // Space types here; it pages only read-only code.
    put(cx, &code, 0..0);
    cx.simulate_keystrokes("space");
    assert!(text_of(cx, &code).starts_with(" fn a()"));
}

/// Space and ⇧Space page read-only code, as the shortcut overlay says, without typing.
#[gpui_kit::test]
fn space_pages_read_only_code(cx: &mut TestAppContext) {
    setup(cx);
    let text: String = (0..400).map(|i| format!("line {i}\n")).collect();
    let (code, _, cx) = code_view(cx, "notes.txt", &text, true);
    let top = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| code.read(cx).state().read(cx).scroll_offset().y)
    };
    assert_eq!(top(cx), px(0.));
    cx.simulate_keystrokes("space");
    draw(cx);
    let down = top(cx);
    assert!(down < px(-100.), "{down:?}");
    cx.simulate_keystrokes("space");
    draw(cx);
    assert!(top(cx) < down);
    cx.simulate_keystrokes("shift-space shift-space");
    draw(cx);
    assert_eq!(top(cx), px(0.));
    assert_eq!(text_of(cx, &code), text);
}

/// ⌘G / ⇧⌘G step through the code's find while its box is open; with none open ⇧⌘G is still
/// Focus Git Panel's.
#[gpui_kit::test]
fn find_keys_step_in_the_code_and_leave_the_git_panel_alone(cx: &mut TestAppContext) {
    setup(cx);
    let text: String = (0..30).map(|i| format!("let v{i} = {i};\n")).collect();
    let (code, git, cx) = code_view(cx, "src/lib.rs", &text, true);
    let (next, prev) = (key("cmd-g", "f3"), key("shift-cmd-g", "shift-f3"));
    cx.simulate_keystrokes(prev);
    assert_eq!(git.get(), 1);

    cx.dispatch_action(gitviber_ui::keymap::editor::Find);
    draw(cx);
    cx.simulate_input("let");
    draw(cx);
    let current = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let s = code.read(cx).state().read(cx);
            assert!(s.search_session().open);
            s.search_session().matcher.current_match_index()
        })
    };
    let at = current(cx);
    cx.simulate_keystrokes(next);
    assert_eq!(current(cx), at + 1);
    cx.simulate_keystrokes(prev);
    assert_eq!(current(cx), at);
    // Back in the code with the box still open.
    cx.update(|window, cx| code.read(cx).editor_focus(cx).focus(window, cx));
    cx.simulate_keystrokes(next);
    assert_eq!(current(cx), at + 1);
    assert_eq!(git.get(), 1);
}

/// A file name too long for the header gives way (its folder first) before the buttons.
#[gpui_kit::test]
fn a_long_file_name_gives_way_in_the_header(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("longname");
    let long = format!("{}.rs", "a_very_long_file_name_".repeat(6));
    std::fs::write(repo.join("src").join(&long), "fn x() {}\n").unwrap();
    let (ws, cx) = open_repo(cx, &repo);
    cx.simulate_resize(size(px(520.), px(400.)));
    let weak = ws.downgrade();
    let path = format!("src/{long}");
    cx.update(|window, cx| crate::open_file(&weak, &path, Default::default(), window, cx));
    cx.run_until_parked();
    draw(cx);
    let name = cx.debug_bounds("file-name").expect("drawn");
    let header = cx.debug_bounds("file-header").expect("drawn");
    // Changes and Blame still fit after it.
    assert!(
        name.right() < header.right() - px(100.),
        "{name:?} {header:?}"
    );
    assert!(name.size.width > px(100.), "{name:?}");
    _ = std::fs::remove_dir_all(&repo);
}

/// ⇧↓ after the range's anchor was hidden (its folder closed) starts over from the selected
/// row, instead of picking one row at a time.
#[gpui_kit::test]
fn a_shift_range_starts_over_once_its_anchor_is_hidden(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("anchor");
    std::fs::write(repo.join("src/c.rs"), "c\n").unwrap();
    std::fs::write(repo.join("d.txt"), "d\n").unwrap();
    cx.update(gitviber_ui::install_core);
    let (host, cx) = cx.add_window_view(|_, cx| PanelHost(cx.new(Workspace::new), None));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx));
    });
    cx.run_until_parked();
    let panel = cx
        .update(|window, cx| Workspace::ensure_panel(&ws, crate::explorer::PANEL_ID, window, cx))
        .unwrap()
        .view();
    host.update(cx, |h, cx| {
        h.1 = Some(panel.clone());
        cx.notify();
    });
    let panel = panel.downcast::<crate::ExplorerPanel>().unwrap();
    draw(cx);
    panel.update(cx, |p, cx| p.set_open_for_test("src", true, cx));
    draw(cx);
    assert_eq!(
        cx.update(|_, cx| panel.read(cx).row_paths()),
        ["src", "src/b.rs", "src/c.rs", "a.txt", "d.txt"]
    );
    cx.update(|window, cx| {
        use gitviber_ui::workspace::Panel as _;
        panel.update(cx, |p, _| p.select_for_test("src/c.rs"));
        let focus = panel.read(cx).panel_focus(cx).unwrap();
        focus.focus(window, cx);
    });
    draw(cx);
    let picked = |cx: &mut VisualTestContext| cx.update(|_, cx| panel.read(cx).picked_paths());
    cx.simulate_keystrokes("shift-down");
    assert_eq!(picked(cx), ["a.txt", "src/c.rs"]);
    panel.update(cx, |p, cx| p.set_open_for_test("src", false, cx));
    draw(cx);
    cx.simulate_keystrokes("shift-down");
    assert_eq!(picked(cx), ["a.txt", "d.txt"]);
    _ = std::fs::remove_dir_all(&repo);
}
