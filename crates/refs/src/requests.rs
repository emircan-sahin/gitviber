//! A command asking a top bar picker to open (⌘-key, palette, menu bar): the chips live in the
//! top bar's element state, out of reach of a command's handler, so the ask waits here for the
//! chip of that window to take it.

use gpui_kit::{AnyWindowHandle, App, Global};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker {
    Branches,
    Worktrees,
}

#[derive(Default)]
struct Requests(Vec<(Picker, AnyWindowHandle)>);
impl Global for Requests {}

pub fn request(picker: Picker, window: AnyWindowHandle, cx: &mut App) {
    let r = cx.default_global::<Requests>();
    r.0.retain(|(p, w)| !(*p == picker && *w == window));
    r.0.push((picker, window));
    cx.refresh_windows();
}

/// True once for each ask for `picker` in `window`.
pub fn take(picker: Picker, window: AnyWindowHandle, cx: &mut App) -> bool {
    if !cx.has_global::<Requests>() {
        return false;
    }
    let r = cx.global_mut::<Requests>();
    let before = r.0.len();
    r.0.retain(|(p, w)| !(*p == picker && *w == window));
    r.0.len() != before
}
