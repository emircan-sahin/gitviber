//! Notes on a repo file's own view, against a real repo and core: drawn under their lines,
//! carried by typing and kept by a save, back when the file opens again, under all of a wrapped
//! line's rows, hidden by a fold over their line, and gone with the view.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use gitviber_files::CodeView;
use gitviber_ui::workspace::{Selection, Workspace};
use gitviber_ui::{ActiveAppTheme as _, Settings};
use gpui_kit::component::input::{MoveDown, MoveUp};
use gpui_kit::{
    AppContext as _, Bounds, Context, Entity, IntoElement, Modifiers, ParentElement as _, Pixels,
    Render, Styled as _, TestAppContext, VisualTestContext, WeakEntity, Window, div, point, px,
};

use super::CodeNotes;
use crate::model::notes::{ReviewNote, anchor_at};
use crate::note_store::ReviewNotes;

/// Draws the workspace's active tab, as the app's center does.
struct WsHost(Entity<Workspace>);

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

/// A repo with `files` committed, and the config dir beside it.
fn repo(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gitviber-review-code-{name}-{}",
        std::process::id()
    ));
    _ = std::fs::remove_dir_all(&dir);
    let repo = dir.join("repo");
    for (path, text) in files {
        let at = repo.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&repo)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "first"]);
    // SAFETY: each test runs in its own process (nextest), before any thread reads it.
    unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", dir.join("config")) };
    repo.canonicalize().unwrap()
}

fn boot(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gitviber_ui::init(cx);
        let core = gitviber_ui::install_core(cx);
        gitviber_diff::init(cx);
        gitviber_diff::set_core(core, cx);
        gitviber_files::init(cx);
        crate::init(cx);
    });
}

fn window<'a>(
    cx: &'a mut TestAppContext,
    repo: &Path,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|_, cx| WsHost(cx.new(Workspace::new)));
    let ws = host.read_with(cx, |h, _| h.0.clone());
    let root = repo.to_path_buf();
    cx.update(|window, cx| {
        Workspace::register_window(&ws, window, cx);
        ws.update(cx, |ws, cx| ws.open_project(root, window, cx));
    });
    cx.run_until_parked();
    (ws, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// The file's tab, drawn twice: the first frame measures the blocks, the second has their rows.
fn open(ws: &Entity<Workspace>, path: &str, cx: &mut VisualTestContext) {
    let sel = Selection::File { path: path.into() };
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open(sel, false, window, cx)));
    cx.run_until_parked();
    draw(cx);
    draw(cx);
}

fn close(ws: &Entity<Workspace>, cx: &mut VisualTestContext) {
    ws.update(cx, |ws, cx| ws.close_active_tab(cx));
    draw(cx);
}

/// Where note `id`'s card (or "draft", the composer) was drawn in the last frame.
fn card(cx: &mut VisualTestContext, id: &str) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(Box::leak(format!("review-note-{id}").into_boxed_str()))
}

fn line_height(cx: &mut VisualTestContext) -> Pixels {
    cx.update(|_, cx| {
        let t = cx.app_theme();
        t.fonts.code_size * t.fonts.code_line_height
    })
}

type Made = Rc<RefCell<Vec<(WeakEntity<CodeView>, WeakEntity<CodeNotes>)>>>;

/// The init's attacher, also telling the test which code views and notes it made.
fn watch_attach(cx: &mut TestAppContext) -> Made {
    let made: Made = Rc::default();
    let seen = made.clone();
    cx.update(|cx| {
        gitviber_files::set_code_attacher(cx, move |code, cx| {
            let notes = super::attach(code, cx)?;
            seen.borrow_mut()
                .push((code.downgrade(), notes.downgrade()));
            Some(notes.into())
        })
    });
    made
}

fn last(made: &Made) -> (Entity<CodeView>, Entity<CodeNotes>) {
    let made = made.borrow();
    let (code, notes) = made.last().expect("a code view with notes");
    (code.upgrade().unwrap(), notes.upgrade().unwrap())
}

#[gpui_kit::test]
fn a_note_on_a_files_view_follows_typing_survives_a_reopen_and_goes_with_it(
    cx: &mut TestAppContext,
) {
    let text: String = (1..=12).map(|i| format!("line {i}\n")).collect();
    let repo = repo("follow", &[("notes.txt", &text)]);
    let root = repo.to_string_lossy().into_owned();
    boot(cx);
    let made = watch_attach(cx);
    let (ws, cx) = window(cx, &repo);
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    open(&ws, "notes.txt", cx);
    let (code, notes) = last(&made);
    let lh = line_height(cx);

    // Add Review Note on the caret's line 3: its composer opens under it, and Esc closes it.
    cx.update(|window, cx| code.read(cx).editor_focus(cx).focus(window, cx));
    draw(cx);
    cx.dispatch_action(MoveDown);
    cx.dispatch_action(MoveDown);
    cx.dispatch_action(gitviber_ui::keymap::review::AddNote);
    draw(cx);
    draw(cx);
    assert!(notes.read_with(cx, |n, _| n.has_draft()));
    assert!(card(cx, "draft").is_some(), "the composer is drawn");
    assert_eq!(code.read_with(cx, |c, cx| c.block_lines(cx)), [3]);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!notes.read_with(cx, |n, _| n.has_draft()));
    assert!(card(cx, "draft").is_none());
    // The keys are the code's again: the next one opens on the same line, typed into, ⌘↵ adds it.
    cx.dispatch_action(gitviber_ui::keymap::review::AddNote);
    draw(cx);
    cx.simulate_input("Why 3?");
    cx.simulate_keystrokes("cmd-enter");
    draw(cx);
    draw(cx);
    let kept = cx.update(|_, cx| ReviewNotes::get(cx, &root));
    assert_eq!(kept.len(), 1);
    let id = kept[0].id.clone();
    assert_eq!(
        (
            kept[0].path.as_str(),
            kept[0].anchor.start,
            kept[0].at.as_deref()
        ),
        ("notes.txt", 3, None)
    );
    assert_eq!(kept[0].anchor.code, ["line 3"]);
    assert_eq!(code.read_with(cx, |c, cx| c.block_lines(cx)), [3]);
    let drawn = card(cx, &id).expect("the note under line 3");
    assert!(card(cx, "draft").is_none());

    // A line typed above carries it down a row, before any save.
    cx.dispatch_action(MoveUp);
    cx.dispatch_action(MoveUp);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(code.read_with(cx, |c, cx| c.block_lines(cx)), [4]);
    let moved = card(cx, &id).unwrap();
    assert!(
        (moved.origin.y - drawn.origin.y - lh).abs() < px(0.5),
        "{drawn:?} → {moved:?}"
    );
    // A new note waits for the save: its lines on disk are the ones it goes on.
    cx.dispatch_action(gitviber_ui::keymap::review::AddNote);
    cx.run_until_parked();
    assert!(!notes.read_with(cx, |n, _| n.has_draft()));
    let toasts = ws.update(cx, |ws, _| ws.take_toasts());
    assert_eq!(
        toasts.last().and_then(|t| t.title.clone()).as_deref(),
        Some("Save the file first")
    );
    let unsaved = cx.update(|_, cx| ReviewNotes::get(cx, &root)).remove(0);
    assert_eq!(unsaved.anchor.start, 3, "the file on disk hasn't moved yet");
    // Saved, the note is where typing took it.
    code.update_in(cx, |c, window, cx| c.save(window, cx));
    cx.run_until_parked();
    let saved = cx.update(|_, cx| ReviewNotes::get(cx, &root)).remove(0);
    assert_eq!((saved.anchor.start, saved.outdated), (4, false));
    draw(cx);
    assert_eq!(code.read_with(cx, |c, cx| c.block_lines(cx)), [4]);

    // Closed, the view and its notes go; opened again, the note is back under its line.
    drop((code, notes));
    close(&ws, cx);
    assert!(
        made.borrow()
            .iter()
            .all(|(c, n)| c.upgrade().is_none() && n.upgrade().is_none())
    );
    open(&ws, "notes.txt", cx);
    let (code, notes) = last(&made);
    assert_eq!(code.read_with(cx, |c, cx| c.block_lines(cx)), [4]);
    assert!(card(cx, &id).is_some());
    // Resolved, it leaves the view.
    cx.update(|_, cx| ReviewNotes::update(cx, &root, &id, |n| n.resolved = true));
    draw(cx);
    assert!(code.read_with(cx, |c, cx| c.block_lines(cx)).is_empty());
    assert!(card(cx, &id).is_none());
    drop((code, notes));
    close(&ws, cx);
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    _ = std::fs::remove_dir_all(repo.parent().unwrap());
}

#[gpui_kit::test]
fn notes_sit_under_all_of_a_wrapped_line_and_a_fold_over_their_line_hides_them(
    cx: &mut TestAppContext,
) {
    let long = format!("// {}", "wrap me ".repeat(80));
    let text = format!("fn a() {{\n    let x = 1;\n}}\n{long}\nfn b() {{}}\n");
    let repo = repo("wrap", &[("src/lib.rs", &text)]);
    let root = repo.to_string_lossy().into_owned();
    boot(cx);
    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    // Under line 2 (inside fn a), the long line 4, and line 5.
    let ids: Vec<String> = cx.update(|cx| {
        [2, 4, 5]
            .into_iter()
            .map(|l| {
                let note = ReviewNote {
                    path: "src/lib.rs".into(),
                    body: format!("On {l}"),
                    anchor: anchor_at(&lines, l, l),
                    ..Default::default()
                };
                ReviewNotes::add(cx, &root, note)
            })
            .collect()
    });
    let (ws, cx) = window(cx, &repo);
    open(&ws, "src/lib.rs", cx);
    let cards = |cx: &mut VisualTestContext| -> Vec<Option<Bounds<Pixels>>> {
        ids.iter().map(|id| card(cx, id)).collect()
    };
    let lh = line_height(cx);
    let plain = cards(cx);
    assert!(
        plain.iter().all(Option::is_some),
        "init's attacher drew all three"
    );

    // Soft wrap: the long line takes more rows, and its note sits under the last of them.
    cx.update(|_, cx| Settings::update(cx, |s| s.word_wrap = true));
    draw(cx);
    draw(cx);
    let wrapped = cards(cx);
    let [a, b, c] = [0, 1, 2].map(|i| wrapped[i].expect("drawn while wrapped"));
    let down = b.origin.y - plain[1].unwrap().origin.y;
    assert_eq!(
        a.origin.y,
        plain[0].unwrap().origin.y,
        "above the long line, nothing moves"
    );
    let rows = down / lh;
    assert!(
        rows >= 0.99 && (rows - rows.round()).abs() < 0.05,
        "down by whole rows: {down:?}"
    );
    assert!((c.origin.y - plain[2].unwrap().origin.y - down).abs() < px(0.5));

    // A fold over fn a, from its chevron in the gutter (shown while the pointer is there).
    let rows_of = |h: Pixels| (h / lh).ceil();
    let block_top = a.origin.y - (lh * rows_of(a.size.height) - a.size.height) / 2.;
    let chevron = point(a.origin.x - px(15.), block_top - lh * 2. + lh / 2.);
    cx.simulate_mouse_move(chevron, None, Modifiers::none());
    draw(cx);
    cx.simulate_click(chevron, Modifiers::none());
    draw(cx);
    draw(cx);
    let folded = cards(cx);
    assert!(folded[0].is_none(), "inside the fold, its note is hidden");
    let up = b.origin.y - folded[1].expect("below the fold, drawn").origin.y;
    assert!(up > lh * 2., "what's below moves up: {up:?}");
    // Unfolded, it's back where it was.
    cx.simulate_click(chevron, Modifiers::none());
    draw(cx);
    draw(cx);
    let open_again = cards(cx);
    assert_eq!(open_again[0].map(|r| r.origin.y), Some(a.origin.y));
    assert_eq!(open_again[1].map(|r| r.origin.y), Some(b.origin.y));
    close(&ws, cx);
    _ = std::fs::remove_dir_all(repo.parent().unwrap());
}
