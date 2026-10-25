//! A clickable div that Tab reaches, as the old app's buttons were.

use gpui_kit::{App, Div, InteractiveElement as _, MouseButton, Stateful, Styled as _};

use crate::ActiveAppTheme;

pub trait Pressable {
    /// A tab stop: ↵ and Space click it (GPUI's keyboard click), under the pointing hand. A
    /// pointer press leaves focus where it was, as WebKit's buttons did; keys that put focus here
    /// light it as a hover.
    fn pressable(self, cx: &App) -> Self;
}

impl Pressable for Stateful<Div> {
    fn pressable(self, cx: &App) -> Self {
        let t = cx.app_theme();
        let (bg, fg) = (t.hover, t.foreground);
        self.tab_index(0)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .focus_visible(move |s| s.bg(bg).text_color(fg))
    }
}
