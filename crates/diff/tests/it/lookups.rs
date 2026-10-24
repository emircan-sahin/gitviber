//! Go to Definition on a definition lists its uses; Peek shows the definition over the lines;
//! References lists every use.

use crate::review_fixes::{repo, setup, window_with};
use gitviber_diff::{Compare, DiffSource, DiffView, LinePos, Side};
use gitviber_ui::keymap::editor;
use gpui_kit::{AppContext as _, TestAppContext};

#[gpui::test]
fn definitions_peeks_and_references_in_a_diff(cx: &mut TestAppContext) {
    let old = "fn helper() {}\n\nfn main() {}\n";
    let new = "fn helper() {}\n\nfn main() {\n    helper();\n    helper();\n}\n";
    let dir = repo("lookups", &[("a.rs", old, new)]);
    setup(cx, &dir);
    cx.update(gitviber_diff::init);
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| DiffView::new(DiffSource::new(Compare::Unstaged, "a.rs"), cx))
    });
    cx.update(|window, cx| {
        let focus = gpui_kit::Focusable::focus_handle(view.read(cx), cx);
        window.focus(&focus, cx);
    });
    let at = |line: u32, byte: usize, cx: &mut gpui_kit::VisualTestContext| {
        view.update(cx, |v, _| {
            v.set_caret(
                LinePos {
                    side: Side::New,
                    line,
                },
                byte,
            )
        })
    };
    let state = |cx: &mut gpui_kit::VisualTestContext| view.read_with(cx, |v, _| v.places_state());

    // On `helper` where it's defined: its uses.
    at(1, 4, cx);
    cx.dispatch_action(editor::GoToDefinition);
    cx.run_until_parked();
    let (title, peek) = state(cx).expect("its uses are listed");
    assert!(title.ends_with("references"), "{title}");
    assert!(peek.is_none());

    // Peek from a use: the lines around the definition, without leaving.
    at(4, 6, cx);
    cx.dispatch_action(editor::PeekDefinition);
    cx.run_until_parked();
    let (title, peek) = state(cx).expect("a peek");
    assert_eq!(title, "Definition");
    let lines = peek.expect("its lines");
    assert_eq!(lines.first().map(|l| l.as_ref()), Some("fn helper() {}"));

    cx.dispatch_action(editor::GoToReferences);
    cx.run_until_parked();
    let (title, _) = state(cx).expect("the uses");
    assert!(title.ends_with("references"), "{title}");
    let _ = std::fs::remove_dir_all(&dir);
}
