//! OS notifications for news the user would miss: only while no window of ours is in front.

use gpui_kit::{App, AppContext as _};

use crate::Settings;

/// Tells the OS, when the user asked (Settings → Notifications on, and `wanted`: the switch for
/// this kind of news) and isn't looking at the app; `target` is what a click on it shows. True
/// when it was sent. A build outside an app bundle can't post one: quietly nothing.
pub fn notify_if_away(
    wanted: fn(&Settings) -> bool,
    title: String,
    body: String,
    target: Option<String>,
    cx: &mut App,
) -> bool {
    let s = Settings::get(cx);
    if !s.notify || !wanted(s) || cx.active_window().is_some() || !crate::has_core(cx) {
        return false;
    }
    let core = crate::core(cx);
    cx.background_spawn(async move {
        let _ = gitviber_core::api::app::notification_send(&core, title, body, target);
    })
    .detach();
    true
}
