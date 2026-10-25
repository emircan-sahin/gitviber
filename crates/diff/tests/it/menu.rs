//! The diff's right-click menu: a right-click picks the change under it, which the menu's
//! actions (here Copy Change as Patch) then take.

use crate::review_fixes::{repo, setup, window_with};
use gitviber_diff::actions::CopyPatch;
use gitviber_diff::{Compare, DiffSource, DiffStyle, DiffView};
use gpui_kit::{AppContext as _, Modifiers, MouseButton, TestAppContext, point, px};

#[gpui::test]
fn a_right_click_picks_the_change_under_it_for_the_menu(cx: &mut TestAppContext) {
    let dir = repo("menu", &[("a.txt", "one\n", "two\n")]);
    setup(cx, &dir);
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| DiffView::new(DiffSource::new(Compare::Unstaged, "a.txt"), cx))
    });
    let line = cx.update(|_, cx| DiffStyle::get(cx).line_px());
    // Below the 34px header, on the change's second line.
    cx.simulate_mouse_down(
        point(px(200.), px(34.) + line * 1.5),
        MouseButton::Right,
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.selection()).is_some());
    cx.dispatch_action(CopyPatch);
    cx.run_until_parked();
    let patch = cx
        .update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()))
        .unwrap_or_default();
    assert!(patch.contains("-one") && patch.contains("+two"), "{patch}");
    let _ = std::fs::remove_dir_all(&dir);
}
