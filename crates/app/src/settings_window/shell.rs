//! The window's layout: the sections in a sidebar under their group's heading, one search over
//! every row, and the open section's rows in bordered groups. Our own, as gpui-component's
//! Settings view has no headings in its sidebar, puts a submenu chevron on some sections only,
//! and builds every page on every frame.

use std::rc::Rc;

use gitviber_ui::ActiveAppTheme;
use gitviber_ui::keymap::shortcut_label;
use gitviber_ui::widgets::KeyHint;
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, App, Div, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    RenderOnce, ScrollHandle, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, px, relative, rems,
};

/// A titled row's control; `true` while the row is disabled.
pub type Control = Rc<dyn Fn(bool, &mut Window, &mut App) -> AnyElement>;
/// A row drawn whole, as a note or a list is.
pub type Custom = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
type Dirty = Rc<dyn Fn(&App) -> bool>;
type Reset = Rc<dyn Fn(&mut App)>;

pub const TITLE_TEXT: f32 = 0.78125;
pub const HINT_TEXT: f32 = 0.71875;

/// A row's text under its title, and the commands whose keys show beside the title: as keycaps
/// of their own, so a chord never breaks across lines.
#[derive(Clone, Default)]
pub struct Hint {
    pub text: SharedString,
    pub keys: &'static [&'static str],
}

impl From<&str> for Hint {
    fn from(text: &str) -> Self {
        Hint {
            text: SharedString::from(text.to_string()),
            keys: &[],
        }
    }
}

impl From<String> for Hint {
    fn from(text: String) -> Self {
        Hint {
            text: text.into(),
            keys: &[],
        }
    }
}

enum Body {
    Control(Control),
    Custom(Custom),
}

pub struct Row {
    title: SharedString,
    hint: Hint,
    keywords: Vec<SharedString>,
    /// Why it can't be changed now, shown under the hint.
    disabled: Option<SharedString>,
    body: Body,
    reset: Option<(Dirty, Reset)>,
    /// A custom row drawn without the padding, as a list of its own is.
    tight: bool,
}

impl Row {
    pub fn new(
        title: impl Into<SharedString>,
        control: impl Fn(bool, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Row {
            title: title.into(),
            hint: Hint::default(),
            keywords: Vec::new(),
            disabled: None,
            body: Body::Control(Rc::new(control)),
            reset: None,
            tight: false,
        }
    }

    /// A row drawn whole. The search finds it by its keywords only.
    pub fn custom(render: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        Row {
            title: SharedString::default(),
            hint: Hint::default(),
            keywords: Vec::new(),
            disabled: None,
            body: Body::Custom(Rc::new(render)),
            reset: None,
            tight: false,
        }
    }

    pub fn tight(mut self) -> Self {
        self.tight = true;
        self
    }

    pub fn description(mut self, hint: impl Into<Hint>) -> Self {
        self.hint = hint.into();
        self
    }

    pub fn keywords<S: Into<SharedString>>(mut self, words: impl IntoIterator<Item = S>) -> Self {
        self.keywords = words.into_iter().map(Into::into).collect();
        self
    }

    /// Dims the row and turns its control off while `disabled`, saying `why`.
    pub fn disabled_because(mut self, disabled: bool, why: impl Into<SharedString>) -> Self {
        self.disabled = disabled.then(|| why.into());
        self
    }

    /// Lets the page's reset button put it back while `dirty`.
    pub fn on_reset(
        mut self,
        dirty: impl Fn(&App) -> bool + 'static,
        reset: impl Fn(&mut App) + 'static,
    ) -> Self {
        self.reset = Some((Rc::new(dirty), Rc::new(reset)));
        self
    }

    fn matches(&self, q: &str, cx: &App) -> bool {
        let hit = |s: &str| s.to_lowercase().contains(q);
        q.is_empty()
            || hit(&self.title)
            || hit(&self.hint.text)
            || self.keywords.iter().any(|k| hit(k))
            || self
                .hint
                .keys
                .iter()
                .filter_map(|id| shortcut_label(id, cx))
                .any(|k| hit(&k))
    }

    fn render(&self, ix: usize, last: bool, window: &mut Window, cx: &mut App) -> AnyElement {
        let t = cx.app_theme().clone();
        let row = div()
            .id(("row", ix))
            .w_full()
            .py(px(if self.tight { 0. } else { 14. }))
            .when(!last, |d| d.border_b_1().border_color(t.border));
        let control = match &self.body {
            Body::Custom(render) => return row.child(render(window, cx)).into_any_element(),
            Body::Control(control) => control.clone(),
        };
        let off = self.disabled.is_some();
        let text =
            |size: f32, height: f32| div().text_size(rems(size)).line_height(relative(height));
        let keys: Vec<AnyElement> = self
            .hint
            .keys
            .iter()
            .map(|id| KeyHint::command(id, cx).into_any_element())
            .collect();
        row.flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(24.))
            .gap_y_2()
            .child(
                div()
                    .flex_1()
                    .min_w(px(192.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_x_2()
                            .gap_y_1()
                            .child(
                                text(TITLE_TEXT, 1.5)
                                    .font_weight(t.medium())
                                    .text_color(if off {
                                        t.muted_foreground
                                    } else {
                                        t.foreground
                                    })
                                    .child(self.title.clone()),
                            )
                            .children(keys),
                    )
                    .when(!self.hint.text.is_empty(), |d| {
                        d.child(
                            text(HINT_TEXT, 1.6)
                                .text_color(if off { t.subtle } else { t.muted_foreground })
                                .child(self.hint.text.clone()),
                        )
                    })
                    .when_some(self.disabled.clone(), |d, why| {
                        d.child(text(HINT_TEXT, 1.6).text_color(t.subtle).child(why))
                    }),
            )
            .child(div().flex_none().child(control(off, window, cx)))
            .into_any_element()
    }
}

#[derive(Default)]
pub struct Group {
    title: Option<SharedString>,
    rows: Vec<Row>,
    /// Drawn without the border, as a preview is.
    plain: bool,
}

impl Group {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }

    pub fn item(mut self, row: Row) -> Self {
        self.rows.push(row);
        self
    }

    pub fn items(mut self, rows: impl IntoIterator<Item = Row>) -> Self {
        self.rows.extend(rows);
        self
    }

    fn render(&self, ix: usize, window: &mut Window, cx: &mut App) -> AnyElement {
        let t = cx.app_theme().clone();
        let count = self.rows.len();
        let rows: Vec<AnyElement> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| r.render(i, self.plain || i + 1 == count, window, cx))
            .collect();
        div()
            .id(("group", ix))
            .pt(px(if ix == 0 { 16. } else { 20. }))
            .when_some(self.title.clone(), |d, title| {
                d.child(heading(title, &t).pb(px(6.)))
            })
            .child(if self.plain {
                div().children(rows)
            } else {
                div()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(t.border)
                    .px(px(14.))
                    .children(rows)
            })
            .into_any_element()
    }
}

/// A part of the window's top that moves the window when dragged and zooms it on a double
/// click, as a title bar does: the window draws its own top and owns the drag, so nothing else
/// would move it.
pub fn drags_window(id: &'static str, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    // Started on the first move, not the press, so a click on a button inside still lands.
    let pressed = window.use_keyed_state(id, cx, |_, _| false);
    let (down, moved) = (pressed.clone(), pressed);
    div()
        .id(id)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            down.update(cx, |p, _| *p = true)
        })
        .on_mouse_move(move |e, window, cx| {
            if *moved.read(cx) {
                moved.update(cx, |p, _| *p = false);
                // A button let go elsewhere left it set: only a drag moves the window.
                if e.dragging() {
                    window.start_window_move();
                }
            }
        })
        .on_click(|e, window, _| {
            if e.click_count() == 2 {
                window.titlebar_double_click();
            }
        })
}

/// The small capitals over a group of rows, or of sections in the sidebar.
pub fn heading(text: SharedString, t: &gitviber_ui::theme::AppTheme) -> gpui_kit::Div {
    div()
        .text_size(rems(0.65625))
        .font_weight(t.semibold())
        .text_color(t.subtle)
        .child(SharedString::from(text.to_uppercase()))
}

#[derive(Default)]
pub struct Page {
    groups: Vec<Group>,
    /// Shortcuts has a reset of its own.
    fixed: bool,
    /// Above the rows, staying while they scroll.
    pinned: Option<Custom>,
}

impl Page {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn group(mut self, group: Group) -> Self {
        self.groups.push(group);
        self
    }

    pub fn groups(mut self, groups: impl IntoIterator<Item = Group>) -> Self {
        self.groups.extend(groups);
        self
    }

    pub fn resettable(mut self, on: bool) -> Self {
        self.fixed = !on;
        self
    }

    pub fn pinned(
        mut self,
        render: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.pinned = Some(Rc::new(render));
        self
    }

    /// Keeps the rows that match `q` (lowercase); false when none is left.
    pub fn filter(&mut self, q: &str, cx: &App) -> bool {
        for g in &mut self.groups {
            g.rows.retain(|r| r.matches(q, cx));
        }
        self.groups.retain(|g| !g.rows.is_empty());
        !self.groups.is_empty()
    }

    /// The resets of the rows that differ from their defaults now.
    fn dirty(&self, cx: &App) -> Vec<Reset> {
        if self.fixed {
            return Vec::new();
        }
        self.groups
            .iter()
            .flat_map(|g| &g.rows)
            .filter_map(|r| r.reset.as_ref())
            .filter(|(dirty, _)| dirty(cx))
            .map(|(_, reset)| reset.clone())
            .collect()
    }

    /// The page as an element of its own: its rows are drawn once the window's render is done,
    /// so they can read the window's view.
    pub fn view(self, title: &'static str, scroll: &ScrollHandle) -> PageView {
        PageView {
            page: self,
            title,
            scroll: scroll.clone(),
        }
    }

    fn render(
        &self,
        title: &'static str,
        scroll: &ScrollHandle,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let t = cx.app_theme().clone();
        let resets = self.dirty(cx);
        let groups: Vec<AnyElement> = self
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| g.render(i, window, cx))
            .collect();
        div()
            .id(title)
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                drags_window("page-header", window, cx)
                    .h(px(44.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(px(20.))
                    .pr_2()
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .text_size(rems(0.84375))
                            .font_weight(t.semibold())
                            .child(title),
                    )
                    .when(!resets.is_empty(), |d| {
                        d.child(
                            Button::new("reset-page")
                                .ghost()
                                .small()
                                .icon(IconName::Undo2)
                                .tooltip("Reset this page")
                                .on_click(move |_, _, cx| {
                                    for reset in &resets {
                                        reset(cx);
                                    }
                                }),
                        )
                    }),
            )
            .when_some(self.pinned.clone(), |d, pinned| {
                d.child(
                    div()
                        .flex_none()
                        .px(px(20.))
                        .pt(px(12.))
                        .pb(px(10.))
                        .border_b_1()
                        .border_color(t.border)
                        .child(pinned(window, cx)),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("settings-rows")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(scroll)
                            .px(px(20.))
                            .pb(px(20.))
                            .children(groups),
                    )
                    .vertical_scrollbar(scroll),
            )
            .into_any_element()
    }
}

#[derive(IntoElement)]
pub struct PageView {
    page: Page,
    title: &'static str,
    scroll: ScrollHandle,
}

impl RenderOnce for PageView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.page.render(self.title, &self.scroll, window, cx)
    }
}
