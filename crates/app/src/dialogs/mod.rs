//! The app's dialogs: About with the third-party licenses, the git identity a first commit needs,
//! Clone, and the screen shown while git can't run. git's own questions are gitviber-refs'.

pub mod about;
pub mod clone;
pub mod identity;
pub mod licenses;
pub mod needs_git;
pub mod unsaved;

use gpui_kit::App;

pub fn install(cx: &mut App) {
    cx.on_action(|_: &gitviber_ui::keymap::requests::AskGitIdentity, cx| {
        cx.defer(|cx| {
            if let Some(w) = cx.active_window() {
                let _ = w.update(cx, |_, window, cx| identity::ask_again(window, cx));
            }
        })
    });
    about::install(cx);
    unsaved::install(cx);
    clone::install(cx);
    needs_git::check(false, cx);
}
