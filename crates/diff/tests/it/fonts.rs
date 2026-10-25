//! A diff's code draws ligatures only when Settings → Editor turns them on, as a code view's does.

use gitviber_diff::DiffStyle;
use gpui_kit::{FontFeatures, TestAppContext};

#[gpui::test]
fn ligatures_follow_the_setting(cx: &mut TestAppContext) {
    let features =
        |cx: &mut TestAppContext| cx.update(|cx| DiffStyle::dark().code_font(cx).features);
    // No settings (a diff shown without the app): their default, off.
    assert_eq!(features(cx), FontFeatures::disable_ligatures());
    cx.update(|cx| {
        cx.set_global(gitviber_ui::Settings {
            ligatures: true,
            ..Default::default()
        })
    });
    assert_eq!(features(cx), FontFeatures::default());
    cx.update(|cx| cx.set_global(gitviber_ui::Settings::default()));
    assert_eq!(features(cx), FontFeatures::disable_ligatures());
}
