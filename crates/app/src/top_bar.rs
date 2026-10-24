//! The top bar: the traffic lights' room, the wordmark, the chips and git actions features put
//! there (project, branch, worktree, pull request; undo, refresh, pull, push), and the panel
//! toggles. A button whose command nothing handles yet shows disabled.

use gitviber_ui::actions::{terminal, view, workbench};
use gitviber_ui::assets::LOGO;
use gitviber_ui::keymap::{self, shortcut_label};
use gitviber_ui::workspace::{BarSlot, PanelContext, Registry};
use gitviber_ui::{ActiveAppTheme, windows};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _, TitleBar};
use gpui_kit::{
    Action, AnyElement, App, IntoElement, ParentElement as _, Styled as _, Window, div, img,
    prelude::*, px, rems,
};

/// What the buttons need to know to draw: the commands that apply where focus is (None before
/// a frame is drawn, which GPUI tells them from), and the app.
struct Bar<'a> {
    available: Option<Vec<&'static str>>,
    cx: &'a App,
}

impl Bar<'_> {
    /// The command's title with its keys, for a tooltip.
    fn tip(&self, id: &str) -> String {
        let title = keymap::command(id).map(|c| c.title).unwrap_or_default();
        match shortcut_label(id, self.cx) {
            Some(keys) => format!("{title}  {keys}"),
            None => title.to_string(),
        }
    }

    /// Handled along focus's path or by a global listener (a feature's `cx.on_action`).
    fn enabled(&self, action: &dyn Action) -> bool {
        self.available
            .as_ref()
            .is_none_or(|names| names.contains(&action.name()))
    }

    fn icon_button(
        &self,
        id: &'static str,
        icon: IconName,
        command: &str,
        action: impl Action + Clone,
    ) -> Button {
        Button::new(id)
            .ghost()
            .small()
            .icon(icon)
            .tooltip(self.tip(command))
            .disabled(!self.enabled(&action))
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
    }
}

fn divider(cx: &App) -> AnyElement {
    div()
        .w(px(1.))
        .h(px(18.))
        .mx_1()
        .bg(cx.app_theme().border)
        .into_any_element()
}

pub fn top_bar(ctx: &PanelContext, drawn: bool, window: &mut Window, cx: &mut App) -> AnyElement {
    let chips: Vec<AnyElement> = Registry::get(cx)
        .bar_items(BarSlot::TopChips)
        .into_iter()
        .filter_map(|item| (item.render)(ctx, window, cx))
        .collect();
    let actions: Vec<AnyElement> = Registry::get(cx)
        .bar_items(BarSlot::TopActions)
        .into_iter()
        .filter_map(|item| (item.render)(ctx, window, cx))
        .collect();
    let t = cx.app_theme().clone();
    let left_pad = if window.is_fullscreen() || !cfg!(target_os = "macos") {
        px(12.)
    } else {
        px(80.)
    };
    let wordmark = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .child(img(LOGO).size(px(18.)))
        .child(
            div()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_size(rems(0.8125))
                .child("GitViber"),
        );
    let available = drawn.then(|| {
        window
            .available_actions(cx)
            .iter()
            .map(|a| a.name())
            .collect()
    });
    let bar = Bar { available, cx };
    let right = div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .pr_2()
        .children(actions)
        .child(divider(cx))
        .child(bar.icon_button(
            "toggle-terminal",
            IconName::PanelBottom,
            "terminal.toggle",
            terminal::Toggle,
        ))
        .child(bar.icon_button(
            "toggle-git",
            IconName::PanelLeft,
            "view.toggleGitPanel",
            view::ToggleGitPanel,
        ))
        .child(bar.icon_button(
            "toggle-explorer",
            IconName::PanelRight,
            "view.toggleExplorer",
            view::ToggleExplorer,
        ))
        .child(bar.icon_button(
            "settings",
            IconName::SlidersHorizontal,
            "workbench.openSettings",
            workbench::OpenSettings,
        ));
    TitleBar::new()
        .h(windows::TOP_BAR_HEIGHT)
        .pl(left_pad)
        .bg(t.chrome(t.sidebar))
        .border_color(t.border)
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap_2()
                .overflow_hidden()
                .child(wordmark)
                .when(!chips.is_empty(), |d| d.child(divider(cx)))
                .children(chips),
        )
        .child(right)
        .into_any_element()
}
