//! No edit is lost to the disk: a file deleted, renamed or trashed under unsaved typing, and
//! the explorer's empty, failed and discarded states.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{Entity, SharedString, VisualTestContext};

use super::oldbugs::{active_file, draw, open_repo, outcome};
use super::*;
use crate::code::OnDisk;
use crate::file_view::Mode;

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

/// A file deleted under typing and brought back (a git checkout): the same text clears the
/// bar, other text says it changed.
#[gpui_kit::test]
fn typing_stays_when_its_deleted_file_comes_back(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("restored");
    let file = repo.join("src/b.rs");
    let (ws, cx) = open_repo(cx, &repo);
    let view = open(cx, &ws, "src/b.rs");
    let code = code_of(cx, &view).expect("the text shows");
    type_in(cx, &code, "mine ");
    std::fs::remove_file(&file).unwrap();
    reread(cx, &view);
    assert_eq!(flags(cx, &code), (true, OnDisk::Deleted));
    std::process::Command::new("git")
        .args(["checkout", "--", "src/b.rs"])
        .current_dir(&repo)
        .output()
        .unwrap();
    reread(cx, &view);
    assert_eq!(flags(cx, &code), (true, OnDisk::Same));
    std::fs::remove_file(&file).unwrap();
    reread(cx, &view);
    std::fs::write(&file, "theirs\n").unwrap();
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

/// A file renamed under typing takes its new name's kind: its language follows, a preview the
/// name can't have goes back to the code, and a media name keeps the text without saying the
/// disk changed. A folder rename builds the preview again, for its links' new folder.
#[gpui_kit::test]
fn a_renamed_file_under_typing_takes_its_new_kind(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("rename-kind");
    std::fs::create_dir_all(repo.join("docs")).unwrap();
    std::fs::write(repo.join("docs/notes.md"), "# Notes\n").unwrap();
    let (ws, cx) = open_repo(cx, &repo);
    let view = open(cx, &ws, "docs/notes.md");
    let code = code_of(cx, &view).expect("the text shows");
    type_in(cx, &code, "mine ");
    cx.update(|window, cx| view.update(cx, |v, cx| v.set_mode(Mode::Preview, window, cx)));
    draw(cx);
    let preview = |cx: &mut VisualTestContext| cx.update(|_, cx| view.read(cx).shown_preview());
    let old = preview(cx).expect("the preview shows");
    let panel = explorer(cx, &ws);
    let rename = |cx: &mut VisualTestContext, path: &str, name: &str| {
        cx.update(|window, cx| panel.update(cx, |p, cx| p.reveal(path, false, window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| panel.update(cx, |p, cx| p.rename_for_test(path, name, window, cx)));
        cx.run_until_parked();
        draw(cx);
    };

    rename(cx, "docs", "guide");
    let again = preview(cx).expect("still the preview");
    assert_ne!(again.entity_id(), old.entity_id(), "built again");
    let base = cx.update(|_, cx| again.read(cx).base().file.clone());
    assert_eq!(base.as_deref(), Some("guide/notes.md"));
    let query = gitviber_markdown::FindQuery {
        text: "mine".into(),
        match_case: false,
        whole_word: false,
        regex: false,
    };
    let found = cx.update(|_, cx| again.update(cx, |p, cx| p.find(&query, cx)));
    assert_eq!(found, Ok(1), "the typing, not the file on disk");

    rename(cx, "guide/notes.md", "notes.txt");
    assert!(preview(cx).is_none(), "back to the code");
    assert_eq!(code_of(cx, &view), Some(code.clone()));
    let lang = cx.update(|_, cx| code.read(cx).language());
    assert_eq!(lang, crate::language::language_for("notes.txt", None));

    rename(cx, "guide/notes.txt", "notes.png");
    assert_eq!(
        code_of(cx, &view),
        Some(code.clone()),
        "the typing stays text"
    );
    assert_eq!(flags(cx, &code), (true, OnDisk::Same));
    _ = std::fs::remove_dir_all(&repo);
}

/// Renamed onto a name whose tab is still open (its file was deleted): a clean tab has nothing
/// to keep and closes; one with typing stays, saying its file is gone.
#[gpui_kit::test]
fn a_rename_onto_an_open_tab_keeps_only_typing(cx: &mut TestAppContext) {
    setup(cx);
    let repo = small_repo("rename-onto");
    std::fs::write(repo.join("c.txt"), "c\n").unwrap();
    std::fs::write(repo.join("src/d.rs"), "d\n").unwrap();
    let (ws, cx) = open_repo(cx, &repo);
    let panel = explorer(cx, &ws);
    for path in ["c.txt", "src/d.rs"] {
        let view = open(cx, &ws, path);
        std::fs::remove_file(repo.join(path)).unwrap();
        reread(cx, &view);
    }
    open(cx, &ws, "a.txt");
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| p.rename_for_test("a.txt", "c.txt", window, cx))
    });
    cx.run_until_parked();
    assert_eq!(tab_keys(cx, &ws), ["file::c.txt", "file::src/d.rs"]);

    let view = open(cx, &ws, "src/b.rs");
    let code = code_of(cx, &view).expect("the text shows");
    type_in(cx, &code, "mine ");
    cx.update(|window, cx| panel.update(cx, |p, cx| p.reveal("src/b.rs", false, window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.rename_for_test("src/b.rs", "d.rs", window, cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(
        tab_keys(cx, &ws),
        ["file::c.txt", "file::src/d.rs", "file::src/b.rs"]
    );
    assert_eq!(flags(cx, &code), (true, OnDisk::Deleted));
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
    assert_eq!(detail, "Unsaved edits in src/b.rs are lost.");
    let weak = ws.downgrade();
    let unsaved = cx.update(|_, cx| crate::unsaved_in(&weak, &["src", "src/b.rs"], cx));
    assert_eq!(unsaved, ["src/b.rs"], "once, by its path in the repo");
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

/// The Trash's confirm names each unsaved file once, and only the first few.
#[test]
fn the_trash_lists_a_few_unsaved_files_then_counts_the_rest() {
    use crate::explorer::unsaved_lost;
    let paths = |n: usize| {
        (0..n)
            .map(|i| format!("src/m{i}/mod.rs"))
            .collect::<Vec<_>>()
    };
    assert_eq!(unsaved_lost(&[]), None);
    assert_eq!(
        unsaved_lost(&paths(3)).unwrap(),
        "Unsaved edits in src/m0/mod.rs, src/m1/mod.rs, src/m2/mod.rs are lost."
    );
    assert_eq!(
        unsaved_lost(&paths(5)).unwrap(),
        "Unsaved edits in src/m0/mod.rs, src/m1/mod.rs, src/m2/mod.rs and 2 more are lost."
    );
}
