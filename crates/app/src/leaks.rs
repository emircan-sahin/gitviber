//! Opening and closing the app's own windows and dialogs, again and again, gives every entity
//! back: the settings window (on each section), About with the licenses, Clone, the identity ask.

use gitviber_ui::Workspace;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div,
};

use crate::dialogs;
use crate::overlay::ShortcutOverlay;
use crate::settings_window::{self, Section};

struct Host;
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Lays the window out and paints it, as a frame would: what only rendering builds gets built.
fn draw(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.run_until_parked();
    let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn dialogs_windows_and_the_overlay_give_their_entities_back(cx: &mut TestAppContext) {
    gitviber_ui::test_support::config_dir("app-leaks");
    cx.update(gitviber_ui::init);
    // Updated through the untyped handle: the typed one would hold Root, which a dialog updates.
    let window: AnyWindowHandle = cx
        .add_window(|window, cx| {
            let host = cx.new(|_| Host);
            gpui_kit::base::Root::new(host, window, cx)
        })
        .into();
    cx.run_until_parked();
    let snapshot = cx.update(|cx| cx.leak_detector_snapshot());
    for round in 0..3 {
        let _ = window.update(cx, |_, window, cx| {
            dialogs::about::open(window, cx);
            dialogs::licenses::open(window, cx);
            dialogs::clone::open(window, cx);
        });
        draw(window, cx);
        let _ = window.update(cx, |_, window, cx| window.close_all_dialogs(cx));
        cx.run_until_parked();
        // Each section drawn, then the window closed.
        let section = Section::ALL[round % Section::ALL.len()];
        let mut view = None;
        let settings = cx.add_window(|window, cx| {
            let v = settings_window::view(section, window, cx);
            view = Some(v.clone());
            gpui_kit::base::Root::new(v, window, cx)
        });
        let view = view.unwrap();
        let any: AnyWindowHandle = settings.into();
        for s in Section::ALL {
            view.update(cx, |w, cx| w.show(s, cx));
            draw(any, cx);
        }
        drop(view);
        let _ = any.update(cx, |_, window, _| window.remove_window());
        cx.run_until_parked();
        // The overlay over a workspace, shown and hidden.
        let _ = window.update(cx, |_, _, cx| {
            let ws = cx.new(Workspace::new);
            let overlay = cx.new(|_| ShortcutOverlay::new(ws.clone()));
            overlay.update(cx, |o, cx| {
                o.toggle(cx);
                o.hide(cx);
            });
            drop((overlay, ws));
        });
        cx.run_until_parked();
    }
    cx.update(|cx| cx.assert_no_new_leaks(&snapshot));
}
