//! The status bar's language of the diff the active tab shows, as a code view's tab has it.

use crate::language::language_label;
use crate::{DiffView, StackedDiff};
use gitviber_ui::workspace::{BarItem, BarSlot, PanelContext, register_bar_item};
use gpui_kit::{
    AnyElement, AnyWeakEntity, App, FocusHandle, Global, IntoElement as _, ParentElement as _,
    Styled as _, WeakFocusHandle, Window, div, px,
};

/// Each open diff by its focus: a tab hands out its diff's focus, not the diff. Weak; a closed
/// diff's entry goes when the next one opens.
#[derive(Default)]
struct Diffs(Vec<(WeakFocusHandle, AnyWeakEntity)>);

impl Global for Diffs {}

pub(crate) fn register(cx: &mut App) {
    register_bar_item(
        cx,
        BarItem::new("diff-language", BarSlot::StatusRight, render),
    );
}

pub(crate) fn track(focus: &FocusHandle, view: AnyWeakEntity, cx: &mut App) {
    let diffs = &mut cx.default_global::<Diffs>().0;
    diffs.retain(|(_, v)| v.upgrade().is_some());
    diffs.push((focus.downgrade(), view));
}

fn render(ctx: &PanelContext, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let ws = ctx.workspace.upgrade()?;
    let focus = ws
        .read(cx)
        .tabs()
        .active()?
        .item
        .handle()?
        .focus_handle(cx)?;
    let label = shown_language(&focus, cx)?;
    Some(div().px(px(6.)).child(label).into_any_element())
}

/// The name of the language of the lines the diff with `focus` shows (the file on top, in a
/// list): "Rust". None while it shows none (an image, a preview, a file not read yet) or isn't
/// a diff.
pub fn shown_language(focus: &FocusHandle, cx: &App) -> Option<String> {
    let view = cx
        .try_global::<Diffs>()?
        .0
        .iter()
        .find(|(f, _)| f == focus)?
        .1
        .upgrade()?;
    let lang = match view.downcast::<DiffView>() {
        Ok(diff) => diff.read(cx).shown_language(),
        Err(view) => view
            .downcast::<StackedDiff>()
            .ok()?
            .read(cx)
            .shown_language(),
    };
    Some(language_label(lang?).to_string())
}
