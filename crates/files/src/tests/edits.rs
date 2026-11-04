//! No edit is lost to the disk: a file deleted, renamed or trashed under unsaved typing, and
//! the explorer's empty, failed and discarded states.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{Entity, SharedString, VisualTestContext};

use super::oldbugs::{active_file, draw, open_repo, outcome};
use super::*;
use crate::code::OnDisk;

fn type_in(cx: &mut VisualTestContext, code: &Entity<CodeView>, text: &str) {
    cx.update(|window, cx| {
        let state = code.read(cx).state().clone();
        state.update(cx, |s, cx| {
            s.set_selected_range(0..0, cx);
            s.insert(text.to_string(), window, cx);
        });
    });
    cx.run_until_parked();
}

fn code_of(cx: &mut VisualTestContext, view: &Entity<crate::FileView>) -> Option<Entity<CodeView>> {
    cx.update(|_, cx| view.read(cx).code())
}

fn flags(cx: &mut VisualTestContext, code: &Entity<CodeView>) -> (bool, OnDisk) {
    cx.update(|_, cx| {
        let c = code.read(cx);
        (c.is_dirty(), c.on_disk())
    })
}

/// The file read again, as the watcher does once something changed in the repo.
fn reread(cx: &mut VisualTestContext, view: &Entity<crate::FileView>) {
    cx.update(|window, cx| view.update(cx, |v, cx| v.load(window, cx)));
    cx.run_until_parked();
}

fn open(cx: &mut VisualTestContext, ws: &Entity<Workspace>, path: &str) -> Entity<crate::FileView> {
    let weak = ws.downgrade();
    cx.update(|window, cx| crate::open_file(&weak, path, Default::default(), window, cx));
    cx.run_until_parked();
    draw(cx);
    active_file(cx, ws)
}

fn explorer(cx: &mut VisualTestContext, ws: &Entity<Workspace>) -> Entity<crate::ExplorerPanel> {
    let panel = cx
        .update(|window, cx| Workspace::ensure_panel(ws, crate::explorer::PANEL_ID, window, cx))
        .unwrap();
    cx.run_until_parked();
    panel.view().downcast::<crate::ExplorerPanel>().unwrap()
}

fn tab_keys(cx: &mut VisualTestContext, ws: &Entity<Workspace>) -> Vec<String> {
    cx.update(|_, cx| {
        ws.read(cx)
            .tabs()
            .tabs()
            .iter()
            .map(|t| t.key.to_string())
            .collect()
    })
}

/// Counts the tabs the close guard is asked about (the app's "Save changes?" dialog).
fn count_asks(cx: &mut VisualTestContext) -> Rc<RefCell<Vec<SharedString>>> {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let a = asked.clone();
    cx.update(|_, cx| {
        gitviber_ui::workspace::set_close_guard(
            cx,
            Rc::new(move |keys, _, _| a.borrow_mut().extend(keys)),
        )
    });
    asked
}

/// A file deleted on disk under unsaved typing keeps it: the header says so, ⌘S writes it
/// back (its folder too), and Discard drops it once asked. A clean tab just says it's gone.
#[gpui_kit::test]
fn typing_stays_when_its_file_is_deleted_on_disk(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("deleted");
    let file = repo.join("src/b.rs");
    let (ws, cx) = open_repo(cx, &repo);
    let view = open(cx, &ws, "src/b.rs");
    let code = code_of(cx, &view).expect("the text shows");
    type_in(cx, &code, "mine ");

    // An agent removes the whole folder.
    std::fs::remove_dir_all(repo.join("src")).unwrap();
    reread(cx, &view);
    let kept = code_of(cx, &view).expect("the typing stays");
    assert_eq!(kept, code);
    assert_eq!(flags(cx, &code), (true, OnDisk::Deleted));
    assert!(
        cx.update(|_, cx| gitviber_ui::workspace::TabItem::is_dirty(view.read(cx), cx)),
        "quit still asks"
    );

    // ⌘S writes it back, without asking.
    let task = cx.update(|window, cx| code.update(cx, |c, cx| c.save_task(window, cx)));
    let saved = outcome(cx, task.expect("a repo file saves"));
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(saved.get(), Some(true));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine b\n");
    assert_eq!(flags(cx, &code), (false, OnDisk::Same));

    // Gone again under new typing: Discard asks, then shows the file is gone.
    type_in(cx, &code, "more ");
    std::fs::remove_file(&file).unwrap();
    reread(cx, &view);
    assert_eq!(flags(cx, &code), (true, OnDisk::Deleted));
    cx.update(|window, cx| view.update(cx, |v, cx| v.reload(window, cx)));
    cx.run_until_parked();
    let (_, detail) = cx.pending_prompt().expect("Discard asks");
    assert!(detail.contains("deleted"), "{detail}");
    cx.simulate_prompt_answer("Discard Changes");
    cx.run_until_parked();
    assert!(code_of(cx, &view).is_none());

    // A clean tab whose file goes just says so.
    let other = open(cx, &ws, "a.txt");
    std::fs::remove_file(repo.join("a.txt")).unwrap();
    reread(cx, &other);
    assert!(code_of(cx, &other).is_none());
    _ = std::fs::remove_dir_all(&repo);
}

/// A file turned binary under unsaved typing keeps the typing, as changed on disk.
#[gpui_kit::test]
fn typing_stays_when_its_file_turns_binary(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("binary");
    let (ws, cx) = open_repo(cx, &repo);
    let view = open(cx, &ws, "src/b.rs");
    let code = code_of(cx, &view).expect("the text shows");
    type_in(cx, &code, "mine ");
    std::fs::write(repo.join("src/b.rs"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    reread(cx, &view);
    assert_eq!(code_of(cx, &view), Some(code.clone()));
    assert_eq!(flags(cx, &code), (true, OnDisk::Changed));
    _ = std::fs::remove_dir_all(&repo);
}

/// Renaming a file with unsaved typing in the explorer moves its tab, typing and all, with no
/// ask; ⌘S then writes the new name and the old one stays gone. A clean tab follows too.
#[gpui_kit::test]
fn a_renamed_file_keeps_its_unsaved_typing(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("rename-dirty");
    let (ws, cx) = open_repo(cx, &repo);
    let asked = count_asks(cx);
    let clean = open(cx, &ws, "a.txt");
    let view = open(cx, &ws, "src/b.rs");
    let code = code_of(cx, &view).expect("the text shows");
    type_in(cx, &code, "mine ");
    let panel = explorer(cx, &ws);
    cx.update(|window, cx| panel.update(cx, |p, cx| p.reveal("src/b.rs", false, window, cx)));
    cx.run_until_parked();

    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.rename_for_test("src/b.rs", "c.rs", window, cx)
        })
    });
    cx.run_until_parked();
    assert!(asked.borrow().is_empty(), "nothing asked");
    assert!(!cx.has_pending_prompt());
    assert_eq!(tab_keys(cx, &ws), ["file::a.txt", "file::src/c.rs"]);
    assert_eq!(active_file(cx, &ws), view, "the same view, in its place");
    assert_eq!(code_of(cx, &view), Some(code.clone()));
    assert_eq!(flags(cx, &code), (true, OnDisk::Same));
    assert_eq!(
        cx.update(|_, cx| view.read(cx).source().clone()),
        crate::FileSource::Repo("src/c.rs".into())
    );
    let task = cx.update(|window, cx| code.update(cx, |c, cx| c.save_task(window, cx)));
    let saved = outcome(cx, task.unwrap());
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(saved.get(), Some(true));
    assert_eq!(
        std::fs::read_to_string(repo.join("src/c.rs")).unwrap(),
        "mine b\n"
    );
    assert!(!repo.join("src/b.rs").exists());

    // The clean one is read again under its new name.
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| p.rename_for_test("a.txt", "z.md", window, cx))
    });
    cx.run_until_parked();
    assert_eq!(tab_keys(cx, &ws), ["file::z.md", "file::src/c.rs"]);
    assert_eq!(
        cx.update(|_, cx| clean.read(cx).source().clone()),
        crate::FileSource::Repo("z.md".into())
    );
    _ = std::fs::remove_dir_all(&repo);
}

/// Trashing a file with unsaved typing says in the confirm that it's lost, then closes the tab
/// without the "Save changes?" ask, which would write the file back.
#[gpui_kit::test]
fn trashing_a_file_with_unsaved_typing_says_so_and_closes_its_tab(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("trash-dirty");
    let (ws, cx) = open_repo(cx, &repo);
    let asked = count_asks(cx);
    let view = open(cx, &ws, "src/b.rs");
    let code = code_of(cx, &view).expect("the text shows");
    type_in(cx, &code, "mine ");
    let panel = explorer(cx, &ws);
    cx.update(|window, cx| panel.update(cx, |p, cx| p.reveal("src/b.rs", false, window, cx)));
    cx.run_until_parked();

    cx.update(|window, cx| panel.update(cx, |p, cx| p.trash_for_test("src/b.rs", window, cx)));
    let (_, detail) = cx.pending_prompt().expect("the Trash asks");
    assert!(detail.contains("Unsaved edits in b.rs"), "{detail}");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(tab_keys(cx, &ws), ["file::src/b.rs"]);

    // A clean file's confirm says nothing of edits.
    open(cx, &ws, "a.txt");
    cx.update(|window, cx| panel.update(cx, |p, cx| p.trash_for_test("a.txt", window, cx)));
    let (_, detail) = cx.pending_prompt().expect("the Trash asks");
    assert!(!detail.contains("Unsaved"), "{detail}");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();

    // Once trashed (not run here: it would fill the Trash), the tab closes without the ask.
    let weak = ws.downgrade();
    cx.update(|window, cx| crate::path_moved(&weak, "src", None, window, cx));
    cx.run_until_parked();
    assert_eq!(tab_keys(cx, &ws), ["file::a.txt"]);
    assert!(asked.borrow().is_empty(), "the close guard was never asked");
    _ = std::fs::remove_dir_all(&repo);
}

/// An empty repo says so in the explorer.
#[gpui_kit::test]
fn an_empty_folder_says_so_in_the_explorer(cx: &mut TestAppContext) {
    setup(cx);
    let repo = std::env::temp_dir().join(format!("gitviber-files-empty-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let (ws, cx) = open_repo(cx, &repo);
    let panel = explorer(cx, &ws);
    assert!(cx.update(|_, cx| panel.read(cx).shows_empty_for_test()));
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    cx.update(|_, cx| panel.update(cx, |p, cx| p.refresh(cx)));
    cx.run_until_parked();
    assert!(!cx.update(|_, cx| panel.read(cx).shows_empty_for_test()));
    _ = std::fs::remove_dir_all(&repo);
}

/// Discard's toast names what went in its title; Undo says it was restored, or why not. (Run
/// for real, a discard would fill the Trash: core keeps its copies out of it only in its own tests.)
#[test]
fn discard_names_the_file_and_undo_reports_back() {
    use crate::explorer::{discarded, restored};
    let one = discarded("a.txt", false);
    assert_eq!(one.title.as_deref(), Some("Discarded a.txt"));
    assert_eq!(one.message.as_ref(), "The old version is in the Trash.");
    let many = discarded("2 files", true);
    assert_eq!(many.title.as_deref(), Some("Discarded 2 files"));
    assert_eq!(many.message.as_ref(), "The old versions are in the Trash.");
    let back = restored("a.txt", Ok(()));
    assert_eq!(back.message.as_ref(), "Restored a.txt");
    let failed = restored("a.txt", Err("Nothing to undo.".into()));
    assert_eq!(failed.title.as_deref(), Some("Could not restore a.txt"));
    assert_eq!(failed.message.as_ref(), "Nothing to undo.");
}
