//! The status bar: features' items on the left (ahead/behind, the changes' counts), theirs and
//! the app's on the right (code colors, font, diff layout, wrap, version).

use gitviber_ui::actions::{app, diff, editor};
use gitviber_ui::memory;
use gitviber_ui::settings::{LIGHT_SYNTAX_THEMES, SYNTAX_THEMES};
use gitviber_ui::workspace::{BarSlot, PanelContext, Registry};
use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, Window,
    div, prelude::*, px,
};

pub const STATUS_BAR_HEIGHT: f32 = 24.;

fn item(id: &'static str, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    let t = cx.app_theme();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .h_full()
        .px(px(6.))
        .rounded(px(3.))
        .hover(|s| s.bg(t.hover))
}

pub fn status_bar(ctx: &PanelContext, window: &mut Window, cx: &mut App) -> AnyElement {
    let left: Vec<AnyElement> = Registry::get(cx)
        .bar_items(BarSlot::StatusLeft)
        .into_iter()
        .filter_map(|i| (i.render)(ctx, window, cx))
        .collect();
    let right: Vec<AnyElement> = Registry::get(cx)
        .bar_items(BarSlot::StatusRight)
        .into_iter()
        .filter_map(|i| (i.render)(ctx, window, cx))
        .collect();
    let t = cx.app_theme().clone();
    let s = Settings::get(cx);
    let syntax = SYNTAX_THEMES
        .iter()
        .chain(LIGHT_SYNTAX_THEMES)
        .find(|(id, _)| *id == t.syntax_theme.as_ref())
        .map(|(_, label)| *label)
        .unwrap_or("");
    let font = format!("{} {}", t.fonts.code, s.code_font_size);
    let layout = if s.side_by_side { "Split" } else { "Unified" };
    let wrap = s.word_wrap;
    let os = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .h(px(STATUS_BAR_HEIGHT))
        .px_1()
        .bg(t.chrome(t.sidebar))
        .border_t_1()
        .border_color(t.border)
        .text_color(t.muted_foreground)
        .text_size(gpui_kit::rems(0.71875))
        .child(
            div()
                .flex()
                .items_center()
                .h_full()
                .gap_1()
                .min_w_0()
                .children(left),
        )
        .child(
            div()
                .flex()
                .items_center()
                .h_full()
                .gap_px()
                .children(right)
                .child(item("status-syntax", cx).child(syntax))
                .child(item("status-font", cx).child(font))
                .child(
                    item("status-layout", cx)
                        .child(layout)
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(diff::ToggleSplit), cx)
                        }),
                )
                .child(
                    item("status-wrap", cx)
                        .when(!wrap, |d| d.text_color(t.subtle))
                        .child(Icon::new(IconName::TextWrap).xsmall())
                        .child("Wrap")
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(editor::ToggleWrap), cx)
                        }),
                )
                .when(memory::readout_enabled(), |d| {
                    let footprint = memory::footprint()
                        .map(memory::format_bytes)
                        .unwrap_or_default();
                    let detail = memory::live_counts()
                        .iter()
                        .map(|(k, n)| format!("{k} {n}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    d.child(
                        item("status-memory", cx)
                            .text_color(t.subtle)
                            .font_family(t.fonts.code.clone())
                            .child(format!("{footprint} · {} live", memory::live_total()))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(detail.clone())
                                    .build(window, cx)
                            }),
                    )
                })
                .map(|d| match crate::updates::status_button(cx) {
                    // Once a newer release is out, the version turns into its button.
                    Some(button) => d.child(button),
                    None => d.child(
                        item("status-version", cx)
                            .child(format!("v{} · {os}", env!("CARGO_PKG_VERSION")))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(app::About), cx)
                            }),
                    ),
                }),
        )
        .into_any_element()
}
