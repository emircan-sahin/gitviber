//! The top bar: the traffic lights' room, the wordmark, the chips features put there (project,
//! branch, worktree, pull request), the git actions, and the panel toggles. A button whose
//! command nothing handles yet shows disabled.

use gitviber_ui::actions::{git, repo, terminal, view, workbench};
use gitviber_ui::assets::LOGO;
use gitviber_ui::keymap::{self, shortcut_label};
use gitviber_ui::workspace::{BarSlot, PanelContext, Registry};
use gitviber_ui::{ActiveAppTheme, windows};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::{Disableable as _, Sizable as _, TitleBar};
use gpui_kit::{
    Action, AnyElement, App, FocusHandle, IntoElement, ParentElement as _, Styled as _, Window,
    div, img, prelude::*, px, rems,
};

/// What the buttons need to know to draw: whether a frame was drawn (GPUI can tell which
/// commands apply only from one), the window, the app, and where the menus' commands go.
struct Bar<'a> {
    drawn: bool,
    focus: &'a FocusHandle,
    window: &'a Window,
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

    fn enabled(&self, action: &dyn Action) -> bool {
        !self.drawn || self.window.is_action_available(action, self.cx)
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

    fn split(
        &self,
        id: &'static str,
        icon: IconName,
        label: &'static str,
        command: &str,
        action: impl Action + Clone,
        menu: Vec<(&'static str, Box<dyn Action>)>,
    ) -> AnyElement {
        let focus = self.focus.clone();
        DropdownButton::new(id)
            .button(
                Button::new((id, 0usize))
                    .small()
                    .icon(icon)
                    .label(label)
                    .tooltip(self.tip(command))
                    .disabled(!self.enabled(&action))
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx)
                    }),
            )
            .dropdown_menu(move |mut popup, _, _| {
                popup = popup.action_context(focus.clone());
                for (name, action) in &menu {
                    popup = popup.menu(*name, action.boxed_clone());
                }
                popup
            })
            .into_any_element()
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

pub fn top_bar(
    ctx: &PanelContext,
    focus: &FocusHandle,
    drawn: bool,
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
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_size(rems(0.8125))
                .child("GitViber"),
        );
    let bar = Bar {
        drawn,
        focus,
        window,
        cx,
    };
    let pull = bar.split(
        "pull",
        IconName::ArrowDownToLine,
        "Pull",
        "git.pull",
        git::Pull,
        vec![
            ("Pull", Box::new(git::Pull)),
            ("Fetch", Box::new(git::Fetch)),
            ("Sync (Pull, then Push)", Box::new(git::Sync)),
        ],
    );
    let push = bar.split(
        "push",
        IconName::ArrowUpFromLine,
        "Push",
        "git.push",
        git::Push,
        vec![
            ("Push", Box::new(git::Push)),
            ("Sync (Pull, then Push)", Box::new(git::Sync)),
        ],
    );
    let right = div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .pr_2()
        .child(bar.icon_button("undo", IconName::Undo2, "git.undo", git::Undo))
        .child(bar.icon_button("redo", IconName::Redo2, "git.redo", git::Redo))
        .child(divider(cx))
        .child(bar.icon_button(
            "refresh",
            IconName::RefreshCw,
            "repo.refresh",
            repo::Refresh,
        ))
        .child(pull)
        .child(push)
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
