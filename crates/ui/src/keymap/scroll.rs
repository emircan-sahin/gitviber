//! The keys that scroll a page from the keyboard (a diff's lines, a rendered document), for the
//! views that bind them in their own key context and move by each key's [`Step`].

use gpui_kit::{App, Context, InteractiveElement, KeyBinding, ListState, Pixels, px};

use super::{INPUT_CONTEXT, IS_MAC};

gpui_kit::actions!(
    scroll,
    [
        ScrollUp,
        ScrollDown,
        PageUp,
        PageDown,
        ScrollToTop,
        ScrollToBottom,
        ScrollLeft,
        ScrollRight,
    ]
);

/// How far a scroll key moves: down or right, negative up or left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Lines(f32),
    Pages(f32),
    Start,
    End,
    /// By the view's own sideways step.
    Side(f32),
}

/// ↑/↓, PageUp/PageDown, Space/⇧Space, Home/End and ⌘↑/⌘↓ (Ctrl+Home/End off macOS) in
/// `context`, ←/→ too when it scrolls `sideways`. Never while a field in it is typed into.
pub fn bind(cx: &mut App, context: &str, sideways: bool) {
    let (top, bottom) = if IS_MAC {
        ("cmd-up", "cmd-down")
    } else {
        ("ctrl-home", "ctrl-end")
    };
    let context = format!("{context} && !{INPUT_CONTEXT}");
    let c = Some(context.as_str());
    cx.bind_keys([
        KeyBinding::new("up", ScrollUp, c),
        KeyBinding::new("down", ScrollDown, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("shift-space", PageUp, c),
        KeyBinding::new("space", PageDown, c),
        KeyBinding::new("home", ScrollToTop, c),
        KeyBinding::new("end", ScrollToBottom, c),
        KeyBinding::new(top, ScrollToTop, c),
        KeyBinding::new(bottom, ScrollToBottom, c),
    ]);
    if sideways {
        cx.bind_keys([
            KeyBinding::new("left", ScrollLeft, c),
            KeyBinding::new("right", ScrollRight, c),
        ]);
    }
}

/// Hands each scroll key's step to `f`.
pub fn on_keys<T: 'static, E: InteractiveElement>(
    el: E,
    f: fn(&mut T, Step, &mut Context<T>),
    cx: &Context<T>,
) -> E {
    el.on_action(cx.listener(move |t, _: &ScrollUp, _, cx| f(t, Step::Lines(-1.), cx)))
        .on_action(cx.listener(move |t, _: &ScrollDown, _, cx| f(t, Step::Lines(1.), cx)))
        .on_action(cx.listener(move |t, _: &PageUp, _, cx| f(t, Step::Pages(-1.), cx)))
        .on_action(cx.listener(move |t, _: &PageDown, _, cx| f(t, Step::Pages(1.), cx)))
        .on_action(cx.listener(move |t, _: &ScrollToTop, _, cx| f(t, Step::Start, cx)))
        .on_action(cx.listener(move |t, _: &ScrollToBottom, _, cx| f(t, Step::End, cx)))
        .on_action(cx.listener(move |t, _: &ScrollLeft, _, cx| f(t, Step::Side(-1.), cx)))
        .on_action(cx.listener(move |t, _: &ScrollRight, _, cx| f(t, Step::Side(1.), cx)))
}

/// A rendered page's line step: WebKit's, which the old app's pages scrolled by.
pub const LINE: Pixels = px(40.);
/// The share of the view a rendered page's page step moves, WebKit's too.
pub const PAGE: f32 = 0.875;

/// Moves a `gpui::list` of rendered content by `step` (not sideways), within its content;
/// false when it didn't move.
pub fn step_list(list: &ListState, step: Step) -> bool {
    let top = -list.scroll_px_offset_for_scrollbar().y;
    let max = list.max_offset_for_scrollbar().y;
    let page = (list.viewport_bounds().size.height * PAGE).max(LINE);
    let to = match step {
        Step::Lines(n) => top + LINE * n,
        Step::Pages(n) => top + page * n,
        Step::Start => px(0.),
        Step::End => max,
        Step::Side(_) => top,
    }
    .clamp(px(0.), max);
    if to == top {
        return false;
    }
    list.scroll_by(to - top);
    true
}
