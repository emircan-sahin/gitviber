//! The center's tab strip: a click shows a tab, a double click keeps a preview one, the middle
//! button closes, a drag reorders, the right button has the tab's menu. A tablist for the keys:
//! one tab stop, the active tab its focused one.

use gitviber_ui::actions::{browser, file, tab};
use gitviber_ui::widgets::FileIcon;
use gitviber_ui::workspace::{Selection, TabIcon, Workspace, selection_path};
use gitviber_ui::{ActiveAppTheme, AppTheme};
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseButton, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div, prelude::*, px, rems,
};

pub const TAB_STRIP_HEIGHT: f32 = 36.;
/// A tab is at most 260 wide; past this its title ellipsizes too, so a detail can still show.
const TITLE_MAX: f32 = 170.;

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

/// The tabs the strip shows, by index: an agent's browser page while Show agents' tabs is off
/// keeps running out of it. A tab with no view yet has nothing to hide.
pub fn visible_tabs(ws: &Workspace, cx: &App) -> Vec<usize> {
    ws.tabs()
        .tabs()
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.item.handle().is_some_and(|h| h.hidden(cx)))
        .map(|(i, _)| i)
        .collect()
}

/// The shown tab `dir` steps from `active`, round the ends; None with fewer than two.
pub fn stepped(visible: &[usize], active: Option<usize>, dir: isize) -> Option<usize> {
    if visible.len() < 2 {
        return None;
    }
    let n = visible.len() as isize;
    let at = match active.and_then(|a| visible.iter().position(|i| *i == a)) {
        Some(at) => at as isize,
        // From a hidden tab: the next shown one past it, or the last before it.
        None => {
            let a = active.unwrap_or(0);
            let after = visible.iter().position(|i| *i > a).unwrap_or(visible.len());
            if dir > 0 {
                after as isize - 1
            } else {
                after as isize
            }
        }
    };
    Some(visible[(at + dir).rem_euclid(n) as usize])
}

/// The strip's focus and scroll, kept by the window's view.
pub struct StripKeys {
    pub(crate) focus: FocusHandle,
    scroll: ScrollHandle,
    /// The active tab as last drawn: a new one is scrolled into view.
    drawn: Option<SharedString>,
}

impl StripKeys {
    pub fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle().tab_stop(true),
            scroll: ScrollHandle::new(),
            drawn: None,
        }
    }
}

/// What a key on the focused strip does to the tab at `at` among `shown` (indices into the tabs).
#[derive(Debug, PartialEq)]
enum StripKey {
    Show(usize),
    Keep,
    Close,
    Menu,
}

fn strip_key(e: &KeyDownEvent, shown: &[usize], at: usize) -> Option<StripKey> {
    let k = &e.keystroke;
    if gitviber_refs::ui::is_menu_key(k) {
        return Some(StripKey::Menu);
    }
    let m = k.modifiers;
    if m.platform || m.control || m.alt || m.shift || m.function {
        return None;
    }
    let last = shown.len().checked_sub(1)?;
    Some(match k.key.as_str() {
        "left" => StripKey::Show(at.saturating_sub(1)),
        "right" => StripKey::Show((at + 1).min(last)),
        "home" => StripKey::Show(0),
        "end" => StripKey::Show(last),
        "enter" | "space" => StripKey::Keep,
        "backspace" | "delete" => StripKey::Close,
        _ => return None,
    })
}

fn on_strip_key(
    ws: &WeakEntity<Workspace>,
    scroll: &ScrollHandle,
    e: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ws) = ws.upgrade() else { return };
    let (shown, keys, active) = {
        let w = ws.read(cx);
        let shown = visible_tabs(w, cx);
        let keys: Vec<SharedString> = shown
            .iter()
            .map(|i| w.tabs().tabs()[*i].key.clone())
            .collect();
        (shown, keys, w.tabs().active_index())
    };
    let Some(at) = active.and_then(|a| shown.iter().position(|i| *i == a)) else {
        return;
    };
    let Some(key) = strip_key(e, &shown, at) else {
        return;
    };
    cx.stop_propagation();
    match key {
        StripKey::Show(to) => ws.update(cx, |ws, cx| ws.show_tab(&keys[to], window, cx)),
        StripKey::Keep => ws.update(cx, |ws, cx| ws.pin_tab(&keys[at], cx)),
        StripKey::Close => {
            let closable = ws.read(cx).tabs().tabs()[shown[at]].item.closable(cx);
            if closable {
                ws.update(cx, |ws, cx| ws.close_tab(&keys[at], cx));
            }
        }
        StripKey::Menu => {
            if let Some(mut tab) = scroll.bounds_for_item(at) {
                // Bounds as laid out; the strip draws them scrolled.
                tab.origin += scroll.offset();
                gitviber_refs::ui::open_row_menu(tab, window, cx);
            }
        }
    }
}

/// The tab's menu past the close commands: Keep Open for a preview, a page's Reload and Open in
/// System Browser, a file's Show History and Reveal in Explorer View.
fn tab_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    preview: bool,
    selection: Option<&Selection>,
    workspace: &WeakEntity<Workspace>,
    key: &SharedString,
) -> gpui_kit::component::menu::PopupMenu {
    let mut menu = menu;
    if preview {
        let (ws, key) = (workspace.clone(), key.clone());
        menu = menu
            .separator()
            .item(PopupMenuItem::new("Keep Open").on_click(move |_, _, cx| {
                let _ = ws.update(cx, |ws, cx| ws.pin_tab(&key, cx));
            }));
    }
    let Some(selection) = selection else {
        return menu;
    };
    if let Selection::Browser { url, .. } = selection {
        let ws = workspace.clone();
        let url = url.clone();
        let has_host = url.starts_with("http://") || url.starts_with("https://");
        return menu
            .separator()
            .item(
                PopupMenuItem::new("Reload")
                    .icon(IconName::RotateCw)
                    .on_click(move |_, window, cx| {
                        let tab = ws.upgrade().and_then(|ws| {
                            ws.read(cx)
                                .tabs()
                                .active()
                                .and_then(|t| t.item.focus_handle(cx))
                        });
                        if let Some(tab) = tab {
                            tab.dispatch_action(&browser::Reload, window, cx);
                        }
                    }),
            )
            .item(
                PopupMenuItem::new("Open in System Browser")
                    .icon(IconName::ExternalLink)
                    .disabled(!has_host)
                    .on_click(move |_, _, cx| crate::help::open_url(url.clone(), cx)),
            );
    }
    if selection.file().is_none() && !matches!(selection, Selection::File { .. }) {
        return menu;
    }
    let path = selection_path(selection);
    menu.separator()
        .item(
            PopupMenuItem::new("Show History")
                .icon(IconName::RotateCcwClock)
                .on_click(move |_, window, cx| crate::show_history(&path, true, window, cx)),
        )
        .item(
            PopupMenuItem::new("Reveal in Explorer View")
                .icon(IconName::ListTree)
                .disabled(!selection.on_disk())
                .action(Box::new(file::RevealInExplorer)),
        )
}

pub fn tab_strip(
    workspace: &Entity<Workspace>,
    keys: &mut StripKeys,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = cx.app_theme().clone();
    let ws = workspace.read(cx);
    let active = ws.tabs().active_index();
    let shown = visible_tabs(ws, cx);
    let active_key = ws.tabs().active_key().cloned();
    if active_key != keys.drawn {
        if let Some(at) = active.and_then(|a| shown.iter().position(|i| *i == a)) {
            keys.scroll.scroll_to_item(at);
        }
        keys.drawn = active_key;
    }
    let ring = keys.focus.is_focused(window) && window.last_input_was_keyboard();
    let count = ws.tabs().len();
    let entries: Vec<_> = ws
        .tabs()
        .tabs()
        .iter()
        .enumerate()
        .filter(|(i, _)| shown.contains(i))
        .map(|(i, e)| {
            (
                i,
                e.key.clone(),
                e.preview,
                e.item.title(cx),
                e.item.detail(cx),
                e.item.icon(cx),
                e.item.tooltip(cx),
                e.item.closable(cx),
                e.item.is_dirty(cx),
                e.item.selection().cloned(),
            )
        })
        .collect();
    // Two files of one name (a/index.ts, b/index.ts) show their folders.
    let folders = Selection::folders_apart(entries.iter().filter_map(|e| e.9.as_ref()));
    let tabs = entries.into_iter().map(
        |(i, key, preview, title, detail, tab_icon, tooltip, closable, dirty, selection)| {
            let is_active = active == Some(i);
            let folder = selection
                .as_ref()
                .filter(|s| s.is_file_like())
                .and_then(|s| folders.get(&selection_path(s)))
                .filter(|f| !f.is_empty())
                .cloned();
            let ws_click = workspace.clone();
            let ws_close = workspace.clone();
            let ws_middle = workspace.clone();
            let ws_drop = workspace.clone();
            let ws_menu = workspace.clone();
            let ws_items = workspace.downgrade();
            let key_click = key.clone();
            let key_close = key.clone();
            let key_middle = key.clone();
            let key_menu = key.clone();
            // Close Others / to the Left / to the Right / All: off when they'd close nothing.
            let (left, right) = (i > 0, i + 1 < count);
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
                .when(is_active && ring, |d| {
                    d.child(div().absolute().inset_0().border_1().border_color(t.ring))
                })
                .children(icon(tab_icon, &t))
                // The detail gives way first ("Compare main...feature/search"), the title only
                // past its cap.
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(px(TITLE_MAX))
                        .truncate()
                        .text_size(rems(0.8125))
                        .when(preview, |d| d.italic())
                        .child(title.clone()),
                )
                .when_some(folder, |d, folder| {
                    d.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.6875))
                            .text_color(t.subtle)
                            .child(folder),
                    )
                })
                .when_some(detail, |d, detail| {
                    d.child(
                        div()
                            .min_w_0()
                            .truncate()
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
                .context_menu(move |menu, _, _| {
                    let menu = menu
                        .menu_with_disabled("Close", Box::new(tab::Close), !closable)
                        .menu_with_disabled("Close Others", Box::new(tab::CloseOthers), count < 2)
                        .menu_with_disabled("Close to the Left", Box::new(tab::CloseLeft), !left)
                        .menu_with_disabled("Close to the Right", Box::new(tab::CloseRight), !right)
                        .menu("Close All", Box::new(tab::CloseAll));
                    tab_menu(menu, preview, selection.as_ref(), &ws_items, &key)
                })
        },
    );
    let (ws_keys, scroll) = (workspace.downgrade(), keys.scroll.clone());
    div()
        .id("tab-strip")
        .key_context("TabStrip")
        .track_focus(&keys.focus)
        .track_scroll(&keys.scroll)
        .on_key_down(move |e: &KeyDownEvent, window, cx| {
            on_strip_key(&ws_keys, &scroll, e, window, cx)
        })
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

#[cfg(test)]
mod tests {
    use super::{StripKey, stepped, strip_key};
    use gpui_kit::{KeyDownEvent, Keystroke};

    fn key(s: &str) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke::parse(s).unwrap(),
            is_held: false,
            prefer_character_input: false,
        }
    }

    #[test]
    fn strip_keys_step_without_wrapping() {
        let shown = [0, 2, 3];
        let at = |s: &str, at: usize| strip_key(&key(s), &shown, at);
        assert_eq!(at("right", 1), Some(StripKey::Show(2)));
        assert_eq!(at("right", 2), Some(StripKey::Show(2)));
        assert_eq!(at("left", 0), Some(StripKey::Show(0)));
        assert_eq!(at("end", 0), Some(StripKey::Show(2)));
        assert_eq!(at("home", 2), Some(StripKey::Show(0)));
        assert_eq!(at("space", 1), Some(StripKey::Keep));
        assert_eq!(at("backspace", 1), Some(StripKey::Close));
        assert_eq!(at("shift-f10", 1), Some(StripKey::Menu));
        // ⌥←/⌥→ move the tab (the keymap's), ⌘←/⌘→ step tabs everywhere.
        assert_eq!(at("alt-left", 1), None);
        assert_eq!(at("cmd-right", 1), None);
        assert_eq!(at("a", 1), None);
    }

    #[test]
    fn stepping_skips_hidden_tabs() {
        let shown = [0, 2, 3];
        assert_eq!(stepped(&shown, Some(0), 1), Some(2));
        assert_eq!(stepped(&shown, Some(3), 1), Some(0));
        assert_eq!(stepped(&shown, Some(0), -1), Some(3));
        // From the hidden tab 1: on to 2, or back to 0.
        assert_eq!(stepped(&shown, Some(1), 1), Some(2));
        assert_eq!(stepped(&shown, Some(1), -1), Some(0));
        assert_eq!(stepped(&[4], Some(4), 1), None);
    }
}
