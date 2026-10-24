//! Pieces the pickers and dialogs share: a form in a dialog, the line under a name field, the
//! branch / tag menu a base is picked from, and a chip.

use std::cell::Cell;
use std::rc::Rc;

use gitviber_ui::ActiveAppTheme;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{
    AnyElement, App, Bounds, Entity, FocusHandle, IntoElement as _, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, ParentElement as _, Pixels, PlatformInput, Render, SharedString,
    Styled as _, Window, div, px, rems,
};

use crate::names::Branch;

/// A dialog showing `view`; it closes itself with `window.close_dialog`. `focus` takes the
/// keyboard as it opens.
pub fn open_form<V: Render>(
    title: impl Into<SharedString>,
    width: f32,
    view: Entity<V>,
    focus: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) {
    let title = title.into();
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false)
            && let Some(f) = &focus
        {
            f.focus(window, cx);
        }
        dialog.title(title.clone()).w(px(width)).child(view.clone())
    });
}

/// ⇧F10: a row's right-click menu, asked for from the keyboard. The menus open on a right
/// click, so it is one, on the row's middle, once this key's dispatch is over (a dispatch can't
/// run inside another). `row`: the row's bounds, from the list's scroll handle.
pub fn open_row_menu(row: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let at = row.center();
    window.defer(cx, move |window, cx| {
        let down = MouseDownEvent { button: MouseButton::Right, position: at, modifiers: Modifiers::default(), click_count: 1, first_mouse: false };
        window.dispatch_event(PlatformInput::MouseDown(down), cx);
        let up = MouseUpEvent { button: MouseButton::Right, position: at, modifiers: Modifiers::default(), click_count: 1 };
        window.dispatch_event(PlatformInput::MouseUp(up), cx);
    });
}

/// ⇧F10, as a list's key handler sees it.
pub fn is_menu_key(k: &gpui_kit::Keystroke) -> bool {
    k.key == "f10" && k.modifiers.shift && !k.modifiers.platform && !k.modifiers.control && !k.modifiers.alt
}

/// The line under a typed name: what it became, or that it's taken.
pub fn hint(text: Option<&str>, bad: bool, cx: &App) -> Option<AnyElement> {
    let t = cx.app_theme();
    text.map(|text| {
        div()
            .mt_1()
            .text_size(rems(0.71875))
            .text_color(if bad { t.removed } else { t.muted_foreground })
            .child(text.to_string())
            .into_any_element()
    })
}

/// Small dim text.
pub fn note(text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let t = cx.app_theme();
    div()
        .text_size(rems(0.71875))
        .text_color(t.muted_foreground)
        .child(text.into())
}

/// A small label beside a row's name ("main folder", "working").
pub fn chip(text: impl Into<SharedString>, live: bool, cx: &App) -> AnyElement {
    chip_on(text, live, false, cx)
}

/// A chip on a highlighted row: white on its color.
pub fn chip_on(text: impl Into<SharedString>, live: bool, hot: bool, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    if hot {
        return div()
            .flex_none()
            .px_1()
            .rounded(px(3.))
            .text_size(rems(0.625))
            .line_height(rems(1.))
            .bg(t.primary_foreground.opacity(0.2))
            .text_color(t.primary_foreground)
            .child(text.into())
            .into_any_element();
    }
    div()
        .flex_none()
        .px_1()
        .rounded(px(3.))
        .text_size(rems(0.625))
        .line_height(rems(1.))
        .bg(if live {
            t.primary.opacity(0.15)
        } else {
            t.active
        })
        .text_color(if live { t.primary } else { t.muted_foreground })
        .child(text.into())
        .into_any_element()
}

pub type Pick = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// What a new branch starts at: a local or remote branch, a tag, or with `head` HEAD. Values are
/// full refs. A repo can have thousands of tags: the newest few hundred are listed.
pub fn ref_menu(
    mut menu: PopupMenu,
    branches: &[Branch],
    tags: &[String],
    head: bool,
    current: &str,
    pick: Pick,
) -> PopupMenu {
    let item = |label: String, value: String, pick: &Pick| {
        let pick = pick.clone();
        let checked = value == current;
        PopupMenuItem::new(label)
            .checked(checked)
            .on_click(move |_, window, cx| pick(value.clone(), window, cx))
    };
    menu = menu.scrollable(true).max_h(px(360.)).min_w(px(260.));
    if head {
        menu = menu.item(item("HEAD".into(), "HEAD".into(), &pick));
    }
    let locals: Vec<&Branch> = branches.iter().filter(|b| !b.remote).collect();
    if !locals.is_empty() {
        menu = menu.label("Local");
        for b in locals {
            menu = menu.item(item(
                b.name.clone(),
                format!("refs/heads/{}", b.name),
                &pick,
            ));
        }
    }
    let remotes: Vec<&Branch> = branches
        .iter()
        .filter(|b| b.remote && !b.name.ends_with("/HEAD"))
        .collect();
    if !remotes.is_empty() {
        menu = menu.separator().label("Remote");
        for b in remotes {
            menu = menu.item(item(
                b.name.clone(),
                format!("refs/remotes/{}", b.name),
                &pick,
            ));
        }
    }
    if !tags.is_empty() {
        menu = menu.separator().label("Tags");
        for t in tags.iter().take(300) {
            menu = menu.item(item(t.clone(), format!("refs/tags/{t}"), &pick));
        }
    }
    menu
}
