//! The center's tab strip: a click shows a tab, a double click keeps a preview one, the middle
//! button closes, a drag reorders, the right button has the close commands.

use gitviber_ui::actions::tab;
use gitviber_ui::widgets::FileIcon;
use gitviber_ui::workspace::{TabIcon, Workspace};
use gitviber_ui::{ActiveAppTheme, AppTheme};
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::*, px, rems,
};

pub const TAB_STRIP_HEIGHT: f32 = 36.;

/// A tab being dragged, drawn under the pointer as its title.
#[derive(Clone)]
struct DraggedTab {
    index: usize,
    title: SharedString,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        div()
            .px_3()
            .py_1()
            .rounded(px(4.))
            .bg(t.elevated)
            .border_1()
            .border_color(t.border_strong)
            .text_color(t.foreground)
            .text_size(t.ui_text())
            .child(self.title.clone())
    }
}

fn icon(icon: TabIcon, t: &AppTheme) -> Option<AnyElement> {
    match icon {
        TabIcon::None => None,
        TabIcon::File(path) => Some(FileIcon::file(&path).into_any_element()),
        TabIcon::Icon(name) => Some(
            Icon::new(name)
                .small()
                .text_color(t.muted_foreground)
                .into_any_element(),
        ),
    }
}

pub fn tab_strip(workspace: &Entity<Workspace>, _window: &mut Window, cx: &mut App) -> AnyElement {
    let t = cx.app_theme().clone();
    let ws = workspace.read(cx);
    let active = ws.tabs().active_index();
    let entries: Vec<_> = ws
        .tabs()
        .tabs()
        .iter()
        .map(|e| {
            (
                e.key.clone(),
                e.preview,
                e.item.title(cx),
                e.item.detail(cx),
                e.item.icon(cx),
                e.item.tooltip(cx),
                e.item.closable(cx),
                e.item.is_dirty(cx),
            )
        })
        .collect();
    let tabs = entries.into_iter().enumerate().map(
        |(i, (key, preview, title, detail, tab_icon, tooltip, closable, dirty))| {
            let is_active = active == Some(i);
            let ws_click = workspace.clone();
            let ws_close = workspace.clone();
            let ws_middle = workspace.clone();
            let ws_drop = workspace.clone();
            let ws_menu = workspace.clone();
            let key_click = key.clone();
            let key_close = key.clone();
            let key_middle = key.clone();
            let key_menu = key.clone();
            let close_color = if is_active {
                t.muted_foreground
            } else {
                t.subtle
            };
            div()
                .id(("tab", i))
                .group("tab")
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .h_full()
                .pl_3()
                .pr(px(6.))
                .max_w(px(260.))
                .border_r_1()
                .border_color(t.border)
                .cursor_pointer()
                .text_color(if is_active {
                    t.foreground
                } else {
                    t.muted_foreground
                })
                .when(is_active, |d| d.bg(t.code(t.background)))
                .when(!is_active, |d| d.hover(|s| s.bg(t.hover)))
                .when(is_active, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(2.))
                            .bg(t.primary),
                    )
                })
                .children(icon(tab_icon, &t))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(rems(0.8125))
                        .when(preview, |d| d.italic())
                        .child(title.clone()),
                )
                .when_some(detail, |d, detail| {
                    d.child(
                        div()
                            .flex_none()
                            .font_family(t.fonts.code.clone())
                            .text_size(rems(0.6875))
                            .text_color(t.subtle)
                            .child(detail),
                    )
                })
                .when(closable, |d| {
                    d.child(
                        div()
                            .id(("tab-close", i))
                            .flex_none()
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .hover(|s| s.bg(t.active))
                            .text_color(close_color)
                            .child(if dirty {
                                div()
                                    .size(px(7.))
                                    .rounded_full()
                                    .bg(t.muted_foreground)
                                    .into_any_element()
                            } else {
                                Icon::new(IconName::X).xsmall().into_any_element()
                            })
                            .when(!is_active && !dirty, |d| {
                                d.invisible().group_hover("tab", |s| s.visible())
                            })
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                ws_close.update(cx, |ws, cx| ws.close_tab(&key_close, cx));
                            }),
                    )
                })
                .when_some(tooltip, |d, tip| {
                    d.tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                    })
                })
                .on_click(move |event, window, cx| {
                    ws_click.update(cx, |ws, cx| {
                        ws.activate_tab(&key_click, window, cx);
                        if event.click_count() == 2 {
                            ws.pin_tab(&key_click, cx);
                        }
                    });
                })
                .on_mouse_down(MouseButton::Middle, move |_, _, cx| {
                    ws_middle.update(cx, |ws, cx| ws.close_tab(&key_middle, cx));
                })
                .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                    ws_menu.update(cx, |ws, cx| ws.activate_tab(&key_menu, window, cx));
                })
                .on_drag(DraggedTab { index: i, title }, |tab, _, _, cx| {
                    cx.new(|_| tab.clone())
                })
                .drag_over::<DraggedTab>(move |s, _, _, cx| s.bg(cx.app_theme().active))
                .on_drop(move |dragged: &DraggedTab, _, cx| {
                    ws_drop.update(cx, |ws, cx| ws.move_tab(dragged.index, i, cx));
                })
                .context_menu(|menu, _, _| {
                    menu.menu("Close", Box::new(tab::Close))
                        .menu("Close Others", Box::new(tab::CloseOthers))
                        .menu("Close to the Left", Box::new(tab::CloseLeft))
                        .menu("Close to the Right", Box::new(tab::CloseRight))
                        .separator()
                        .menu("Close All", Box::new(tab::CloseAll))
                })
        },
    );
    div()
        .id("tab-strip")
        .key_context("TabStrip")
        .flex()
        .flex_none()
        .h(px(TAB_STRIP_HEIGHT))
        .w_full()
        .bg(t.chrome(t.panel))
        .border_b_1()
        .border_color(t.border)
        .overflow_x_scroll()
        .children(tabs)
        .into_any_element()
}
