//! The status bar: the branch's state (ahead / behind, a rebase in progress) and features' items
//! on the left (the changes' counts), the view's settings, features' own (the file's language),
//! the version and Open in on the right.

use gitviber_ui::actions::{app, diff, editor};
use gitviber_ui::memory;
use gitviber_ui::settings::{LIGHT_SYNTAX_THEMES, SYNTAX_THEMES};
use gitviber_ui::widgets::Pressable as _;
use gitviber_ui::workspace::{BarSlot, PanelContext, Registry};
use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    Window, div, prelude::*, px,
};

use crate::open_in_button::OpenInButton;

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

/// Ahead / behind its upstream, and a merge or rebase in progress with its conflicts: what the
/// branch is in the middle of. The branch itself is in the top bar.
fn git_state(ctx: &PanelContext, cx: &mut App) -> Vec<AnyElement> {
    let Some(project) = &ctx.project else {
        return Vec::new();
    };
    let Some(state) = gitviber_refs::RepoState::existing(project, cx) else {
        return Vec::new();
    };
    let Some(status) = state.read(cx).status.clone() else {
        return Vec::new();
    };
    let t = cx.app_theme().clone();
    let mut out = Vec::new();
    if status.upstream.is_some() && !status.upstream_gone {
        let count = |icon: IconName, n: u32, lit: gpui_kit::Hsla| {
            div()
                .flex()
                .items_center()
                .when(n > 0, |d| d.text_color(lit))
                .child(Icon::new(icon).xsmall())
                .child(n.to_string())
        };
        out.push(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(6.))
                .font_family(t.fonts.code.clone())
                .child(count(IconName::ArrowUp, status.ahead, t.primary))
                .child(count(IconName::ArrowDown, status.behind, t.modified))
                .into_any_element(),
        );
    }
    if let Some(op) = &status.operation {
        let mut text = op.kind.to_uppercase();
        if let (Some(step), Some(total)) = (op.step, op.total) {
            text.push_str(&format!(" {step}/{total}"));
        }
        let n = status.conflicted.len();
        if n > 0 {
            text.push_str(&format!(
                " · {n} {}",
                if n == 1 { "conflict" } else { "conflicts" }
            ));
        }
        out.push(
            div()
                .px(px(6.))
                .font_weight(t.semibold())
                .text_color(t.conflict)
                .child(text)
                .into_any_element(),
        );
    }
    out
}

/// `v0.1.17 · macOS 26.4.1`, the system's version once About has read it.
#[derive(Default)]
pub struct SystemName(pub Option<String>);
impl gpui_kit::Global for SystemName {}

/// Reads the OS's name and version once, off the UI thread, for the status bar.
pub fn read_system_name(cx: &mut App) {
    if !gitviber_ui::has_core(cx) {
        return;
    }
    crate::help::with_about(cx, |about, cx| {
        if let Some(a) = about {
            cx.set_global(SystemName(Some(a.os)));
            cx.refresh_windows();
        }
    });
}

pub fn status_bar(
    ctx: &PanelContext,
    open_in: &Entity<OpenInButton>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let mut left: Vec<AnyElement> = git_state(ctx, cx);
    left.extend(
        Registry::get(cx)
            .bar_items(BarSlot::StatusLeft)
            .into_iter()
            .filter_map(|i| (i.render)(ctx, window, cx)),
    );
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
    // The code and diff views' settings and Open in are about a repo: with none open, only the
    // version (and the memory readout) stay.
    let repo = ctx.project.is_some();
    let os = cx
        .try_global::<SystemName>()
        .and_then(|n| n.0.clone())
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "macOS"
            } else if cfg!(windows) {
                "Windows"
            } else {
                "Linux"
            }
            .to_string()
        });
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
                .when(repo, |d| {
                    // Both are set in Settings → Editor, which a click opens.
                    let editor_settings =
                        |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
                            crate::settings_window::open(
                                Some(crate::settings_window::Section::Editor),
                                cx,
                            )
                        };
                    d.child(
                        item("status-syntax", cx)
                            .debug_selector(|| "status-syntax".into())
                            .pressable(cx)
                            .child(syntax)
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Syntax theme")
                                    .build(window, cx)
                            })
                            .on_click(editor_settings),
                    )
                    .child(
                        item("status-font", cx)
                            .pressable(cx)
                            .child(font)
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Code font")
                                    .build(window, cx)
                            })
                            .on_click(editor_settings),
                    )
                    .child(
                        item("status-layout", cx)
                            .pressable(cx)
                            .child(layout)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(diff::ToggleSplit), cx)
                            }),
                    )
                    .child(
                        item("status-wrap", cx)
                            .pressable(cx)
                            .when(wrap, |d| d.text_color(t.primary))
                            .child(Icon::new(IconName::TextWrap).xsmall())
                            .child("Wrap")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(editor::ToggleWrap), cx)
                            }),
                    )
                })
                // Features' own (the open file's language) after the view's settings.
                .children(right)
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
                // A newer release shows in the top bar's badge; the version stays here.
                .child(
                    item("status-version", cx)
                        .pressable(cx)
                        .child(format!("v{} · {os}", env!("CARGO_PKG_VERSION")))
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(app::About), cx)),
                )
                .when(repo, |d| d.child(open_in.clone())),
        )
        .into_any_element()
}
