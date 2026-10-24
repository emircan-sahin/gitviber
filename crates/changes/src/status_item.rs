//! The status bar's "N changed · +a −d · r/N reviewed": the uncommitted files, their lines, and
//! how many are reviewed. Read from the repo state the top bar keeps, so it shows with Changes
//! closed too.

use crate::model::list::totals;
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::workspace::{BarItem, BarSlot, PanelContext, register_bar_item};
use gpui_kit::{AnyElement, App, IntoElement, ParentElement as _, Styled as _, Window, div, px};

pub fn register(cx: &mut App) {
    register_bar_item(
        cx,
        BarItem::new("changes", BarSlot::StatusLeft, render).order(0),
    );
}

fn render(ctx: &PanelContext, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let state = gitviber_refs::RepoState::existing(ctx.project.as_ref()?, cx)?;
    let status = state.read(cx).status.clone()?;
    let ws = ctx.workspace.upgrade()?;
    let ws = ws.read(cx);
    let n = totals(
        &status.conflicted,
        &status.staged,
        &status.unstaged,
        |key, sig| ws.viewed(key) == Some(sig),
    );
    if n.files == 0 {
        return None;
    }
    let t = cx.app_theme();
    let code = |text: String, color| {
        div()
            .font_family(t.fonts.code.clone())
            .text_color(color)
            .child(text)
    };
    Some(
        div()
            .flex()
            .items_center()
            .gap_1()
            .px(px(6.))
            .child(format!("{} changed ·", n.files))
            .child(code(format!("+{}", n.add), t.added))
            .child(code(format!("−{}", n.del), t.removed))
            .child(format!("· {}/{} reviewed", n.reviewed, n.files))
            .into_any_element(),
    )
}
