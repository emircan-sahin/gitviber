//! Typing kept from before a restart: it comes back in the editor over the file as it was,
//! a save asks first when the file changed since, and dropping typing asks too.

use crate::review_fixes::{repo, setup, window_with};
use gitviber_diff::{Compare, DiffSource, DiffView};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext};

#[gpui::test]
fn kept_typing_comes_back_and_asks_before_overwriting(cx: &mut TestAppContext) {
    let dir = repo("restore", &[("a.txt", "one\n", "two\n")]);
    setup(cx, &dir);
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| {
            let mut v = DiffView::new(DiffSource::new(Compare::Unstaged, "a.txt"), cx);
            // Typed over "two\n" before the restart; an agent wrote "agent\n" since.
            v.restore_edit("mine\n".into(), "two\n".into(), cx);
            v
        })
    });
    std::fs::write(dir.join("a.txt"), "agent\n").unwrap();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert!(v.is_editing(), "the editor opens with the kept typing");
        assert_eq!(
            v.unsaved_edit(cx),
            Some(("mine\n".to_string(), "two\n".to_string()))
        );
    });
    cx.update(|window, cx| view.update(cx, |v, cx| v.save(window, cx)));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "agent\n",
        "the agent's write stays until the user says otherwise"
    );
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn leaving_the_editor_with_typing_asks_first(cx: &mut TestAppContext) {
    let dir = repo("discard", &[("a.txt", "one\n", "two\n")]);
    setup(cx, &dir);
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| DiffView::new(DiffSource::new(Compare::Unstaged, "a.txt"), cx))
    });
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.toggle_edit(window, cx);
            v.set_edit_text("mine\n", window, cx);
            v.toggle_edit(window, cx);
        })
    });
    cx.run_until_parked();
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    assert!(view.read_with(cx, |v, cx| v.has_unsaved_edits(cx)));
    // Nothing typed: it just closes, and says there's nothing left to keep.
    cx.update(|window, cx| {
        window.close_dialog(cx);
        view.update(cx, |v, cx| {
            v.set_edit_text("two\n", window, cx);
            v.toggle_edit(window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert!(!v.is_editing());
        assert_eq!(v.unsaved_edit(cx), None);
    });
    let _ = std::fs::remove_dir_all(&dir);
}
