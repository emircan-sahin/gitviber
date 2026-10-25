//! One file's diff as a tab of its own: a comparison's file, or the working tree against a commit.

use gitviber_diff::{DiffSource, DiffView};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{TabIcon, TabItem};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window, div,
};

pub struct DiffTab {
    diff: Entity<DiffView>,
    path: String,
    /// What it compares: "main...feature", "abc1234 → working tree".
    label: SharedString,
    _live: Live,
}

impl DiffTab {
    pub fn new(source: DiffSource, label: SharedString, cx: &mut Context<Self>) -> Self {
        let path = source.path.clone();
        DiffTab {
            diff: cx.new(|cx| DiffView::new(source, cx)),
            path,
            label,
            _live: Live::new("DiffTab"),
        }
    }
}

impl TabItem for DiffTab {
    fn tab_title(&self, _: &App) -> SharedString {
        self.path
            .rsplit('/')
            .next()
            .unwrap_or(&self.path)
            .to_string()
            .into()
    }

    fn tab_detail(&self, _: &App) -> Option<SharedString> {
        Some(self.label.clone())
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::File(self.path.clone().into())
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        Some(format!("{} · {}", self.path, self.label).into())
    }

    fn tab_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.diff.focus_handle(cx))
    }
}

impl Render for DiffTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.diff.clone())
    }
}
