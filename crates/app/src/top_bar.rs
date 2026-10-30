//! The top bar: the traffic lights' room, the wordmark, the chips and git actions features put
//! there (project, branch, worktree, pull request; undo, refresh, pull, push), and the panel
//! toggles. A button whose command nothing handles yet shows disabled.

use gitviber_ui::actions::{terminal, view, workbench};
use gitviber_ui::assets::LOGO;
use gitviber_ui::keymap;
use gitviber_ui::widgets::{Look, StatusDot, command_kbd};
use gitviber_ui::workspace::{BarSlot, PanelContext, Registry, Slot};
use gitviber_ui::{ActiveAppTheme, windows};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _, TitleBar};
use gpui_kit::{
    Action, AnyElement, App, IntoElement, ParentElement as _, SharedString, Styled as _, Window,
    div, img, prelude::*, px, rems,
};

/// What the buttons need to know to draw: the commands that apply where focus is (None before
/// a frame is drawn, which GPUI tells them from), and the app.
struct Bar<'a> {
    available: Option<Vec<&'static str>>,
    cx: &'a App,
}

impl Bar<'_> {
    /// The command's title, for a tooltip that shows its keys beside it.
    fn title(&self, id: &str) -> &'static str {
        keymap::command(id).map(|c| c.title).unwrap_or_default()
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
            .tooltip(self.title(command))
            .tooltip_key_binding(command_kbd(command, self.cx))
            .disabled(!self.enabled(&action))
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
    }

    /// A panel's button: a screen reader hears the panel's name and whether it's shown, where
    /// the tooltip says the command and its keys.
    fn panel_button(
        &self,
        id: &'static str,
        icon: IconName,
        name: impl Into<SharedString>,
        command: &str,
        action: impl Action + Clone,
        shown: bool,
    ) -> Button {
        self.icon_button(id, icon, command, action)
            .accessibility_label(name)
            .toggled(shown)
    }
}

fn divider(cx: &App) -> AnyElement {
    div()
        .w(px(1.))
        .h(px(18.))
        .mx_1()
        .bg(cx.app_theme().border_strong)
        .into_any_element()
}

/// The terminal's button; while it's hidden, a dot for an agent that asked or news not looked at
/// (`news`), which its tooltip and name say too.
fn terminal_button(bar: &Bar, shown: bool, news: Option<Look>) -> AnyElement {
    let what = news.map(|look| match look {
        Look::Needs => "needs you",
        _ => "not viewed",
    });
    let mut button = bar.panel_button(
        "toggle-terminal",
        IconName::PanelBottom,
        what.map_or_else(
            || "Terminal".into(),
            |what| format!("Terminal, a terminal {what}"),
        ),
        "terminal.toggle",
        terminal::Toggle,
        shown,
    );
    if let Some(what) = what {
        button = button.tooltip(format!(
            "{} · a terminal: {what}",
            bar.title("terminal.toggle")
        ));
    }
    div()
        .relative()
        .child(button)
        .when_some(news, |d, look| {
            d.child(
                div()
                    .absolute()
                    .top(px(3.))
                    .right(px(3.))
                    .child(StatusDot(look)),
            )
        })
        .into_any_element()
}

pub fn top_bar(
    ctx: &PanelContext,
    drawn: bool,
    terminal_news: Option<Look>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
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
                .font_weight(t.semibold())
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
    let shown = |slot| {
        ctx.workspace
            .upgrade()
            .is_some_and(|ws| ws.read(cx).is_open(slot))
    };
    let right = div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .pr_2()
        .children(actions)
        .child(divider(cx))
        .child(terminal_button(&bar, shown(Slot::Bottom), terminal_news))
        .child(bar.panel_button(
            "toggle-git",
            IconName::PanelLeft,
            "Git Panel",
            "view.toggleGitPanel",
            view::ToggleGitPanel,
            shown(Slot::Left),
        ))
        .child(bar.panel_button(
            "toggle-explorer",
            IconName::PanelRight,
            "Explorer",
            "view.toggleExplorer",
            view::ToggleExplorer,
            shown(Slot::Right),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen reader names a panel's button by its panel, not by the tooltip's command and keys.
    #[gpui_kit::test]
    fn a_panels_button_is_named_by_its_panel(cx: &mut gpui_kit::TestAppContext) {
        gitviber_ui::test_support::config_dir("top-bar");
        cx.update(|cx| {
            gitviber_ui::init(cx);
            let bar = Bar {
                available: None,
                cx,
            };
            let button = bar.panel_button(
                "toggle-git",
                IconName::PanelLeft,
                "Git Panel",
                "view.toggleGitPanel",
                view::ToggleGitPanel,
                true,
            );
            assert_eq!(button.accessibility_name().as_deref(), Some("Git Panel"));
        });
    }
}
