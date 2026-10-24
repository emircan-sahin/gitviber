//! Two working-tree files compared: the diff view with `Compare::Files`, in a tab.

use gitviber_diff::{Compare, DiffSource, DiffView};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{TabIcon, TabItem};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div,
};

pub struct CompareTab {
    left: String,
    right: String,
    diff: Entity<DiffView>,
    _live: Live,
}

fn name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl CompareTab {
    pub fn new(left: String, right: String, cx: &mut Context<Self>) -> Self {
        if gitviber_ui::has_core(cx) {
            gitviber_diff::set_core(gitviber_ui::core(cx), cx);
        }
        let source = DiffSource::new(Compare::Files, right.clone()).old_path(Some(left.clone()));
        let diff = cx.new(|cx| DiffView::new(source, cx));
        CompareTab {
            left,
            right,
            diff,
            _live: Live::new("CompareTab"),
        }
    }
}

impl TabItem for CompareTab {
    fn tab_title(&self, _: &App) -> SharedString {
        format!("{} ↔ {}", name(&self.left), name(&self.right)).into()
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::File(self.right.clone().into())
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        Some(format!("{} ↔ {}", self.left, self.right).into())
    }
}

impl Render for CompareTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.diff.clone())
    }
}
