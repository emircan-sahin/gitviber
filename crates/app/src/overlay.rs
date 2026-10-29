//! Hold ⌘ by itself for a moment and every shortcut that works where focus is shows over the
//! window, until ⌘ is released. ⌘/ shows the same until the next key or click. It takes the
//! pointer, so nothing underneath hovers through it, and a click on it only closes it.

use std::time::Duration;

use gitviber_ui::AppTheme;
use gitviber_ui::keymap::edit::winner;
use gitviber_ui::keymap::{self, Category, IS_MAC, bindings_for, chord_keys};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Part, Slot, Workspace};
use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{
    AnyElement, App, Context, Entity, InteractiveElement as _, IntoElement, KeyDownEvent,
    Modifiers, ModifiersChangedEvent, MouseButton, MouseMoveEvent, ParentElement as _, Point,
    Render, ScrollHandle, ScrollWheelEvent, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, TextAlign, TextRun, Window, canvas, deferred, div, fill, linear_color_stop,
    linear_gradient, point, prelude::FluentBuilder as _, px, rems,
};

/// Off macOS "cmd" is Ctrl (the keymap's rule), so that's the key to hold there.
pub(crate) const HOLD: Duration = Duration::from_millis(1450);
/// Between two chords: a range (⌘1 … ⌘8) rather than alternatives.
const THROUGH: &str = "…";
/// The fade at the list's foot, as tall as the padding under its last row.
const FADE: f32 = 36.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Up while ⌘ is held.
    Held,
    /// ⌘/: up until the next key or click.
    Pinned,
}

/// One line: its keys (alternatives, or THROUGH between two), and where it works when that's
/// one place.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub title: SharedString,
    pub keys: Vec<String>,
    pub place: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub title: SharedString,
    pub rows: Vec<Row>,
}

/// Keys that aren't commands: moving inside a list or view, and the system's own.
fn fixed() -> Vec<Group> {
    let row = |keys: &[&str], title: &'static str| Row {
        title: title.into(),
        keys: keys.iter().map(|k| k.to_string()).collect(),
        place: None,
    };
    let mut system = vec![
        row(&["cmd+c"], "Copy"),
        row(&["cmd+x"], "Cut"),
        row(&["cmd+v"], "Paste"),
        row(&["cmd+a"], "Select all"),
    ];
    if IS_MAC {
        system.extend([
            row(&["cmd+h"], "Hide GitViber"),
            row(&["cmd+m"], "Minimize"),
            row(&["cmd+`"], "Next window"),
            row(&["cmd+q"], "Quit"),
        ]);
    }
    vec![
        Group {
            title: "Navigation".into(),
            rows: vec![
                row(&["up", "down"], "Move through a list"),
                row(&["enter"], "Open, keep the tab"),
                row(&["right", "escape"], "To the code and back"),
                row(&["left", "right"], "Collapse / expand in a tree"),
                row(&["shift+f10"], "Row menu"),
                row(&["space", "pageup", "pagedown"], "Scroll the code view"),
                row(&["tab"], "Next control"),
            ],
        },
        Group {
            title: "System".into(),
            rows: system,
        },
    ]
}

/// The focused part's own categories, listed first while it has focus.
fn first_for(part: Option<Part>) -> &'static [Category] {
    match part {
        Some(Part::Center) => &[Category::Review, Category::Diff, Category::Editor],
        Some(Part::Slot(Slot::Left)) => &[Category::Git, Category::Review],
        Some(Part::Slot(Slot::Right)) => &[Category::Explorer],
        Some(Part::Slot(Slot::Bottom)) => &[Category::Terminal],
        None => &[],
    }
}

/// Every bound command by category, as `runs` says which global ones work where focus is: those
/// that don't were left out, greyed out they crowded the ones that do. A local one works in its
/// own place, which it names instead.
pub fn groups(
    overrides: &keymap::Overrides,
    part: Option<Part>,
    runs: impl Fn(&keymap::Command) -> bool,
) -> Vec<Group> {
    let mut by: Vec<(Category, Vec<Row>)> = Vec::new();
    let mut add = |category: Category, row: Row| match by.iter_mut().find(|(c, _)| *c == category) {
        Some((_, rows)) => rows.push(row),
        None => by.push((category, vec![row])),
    };
    for c in keymap::all_commands() {
        let keys = bindings_for(c, overrides, IS_MAC);
        if keys.is_empty() {
            continue;
        }
        // ⌘1–⌘8 read as one row.
        if c.id.starts_with("tab.goto") && c.id != "tab.goto1" {
            continue;
        }
        if c.id == "tab.goto1" {
            if runs(c) {
                let last = keymap::command("tab.goto8")
                    .and_then(|l| bindings_for(l, overrides, IS_MAC).into_iter().next());
                let keys = match last {
                    Some(last) => vec![keys[0].clone(), THROUGH.into(), last],
                    None => vec![keys[0].clone()],
                };
                add(
                    c.category,
                    Row {
                        title: "Go to Tab 1–8".into(),
                        keys,
                        place: None,
                    },
                );
            }
            continue;
        }
        if let Some(place) = c.place() {
            add(
                c.category,
                Row {
                    title: c.title.into(),
                    keys,
                    place: Some(place),
                },
            );
            continue;
        }
        let wins = keys
            .iter()
            .any(|k| winner(k, overrides, IS_MAC).is_some_and(|w| w.id == c.id));
        if wins && runs(c) {
            add(
                c.category,
                Row {
                    title: c.title.into(),
                    keys,
                    place: None,
                },
            );
        }
    }
    let first = first_for(part);
    by.sort_by_key(|(c, _)| first.iter().position(|f| f == c).unwrap_or(first.len()));
    by.into_iter()
        .map(|(c, rows)| Group {
            title: c.label().into(),
            rows,
        })
        .chain(fixed())
        .collect()
}

/// `groups` laid out in `n` columns, each next one into the shortest so far, by height drawn
/// in half rows: a row naming its place takes a second, smaller line.
pub fn columns(groups: Vec<Group>, n: usize) -> Vec<Vec<Group>> {
    let n = n.max(1);
    let mut cols: Vec<(usize, Vec<Group>)> = (0..n).map(|_| (0, Vec::new())).collect();
    for g in groups {
        let places = g.rows.iter().filter(|r| r.place.is_some()).count();
        let height = 2 * g.rows.len() + places + 4;
        let shortest = cols
            .iter_mut()
            .min_by_key(|(h, _)| *h)
            .expect("at least one column");
        shortest.0 += height;
        shortest.1.push(g);
    }
    cols.into_iter()
        .map(|(_, g)| g)
        .filter(|g| !g.is_empty())
        .collect()
}

pub struct ShortcutOverlay {
    workspace: Entity<Workspace>,
    shown: Option<Shown>,
    hold: Option<Task<()>>,
    /// The list's scroll: whether more is below, and back to the top each time it shows.
    scroll: ScrollHandle,
    _live: Live,
}

impl ShortcutOverlay {
    pub fn new(workspace: Entity<Workspace>) -> Self {
        Self {
            workspace,
            shown: None,
            hold: None,
            scroll: ScrollHandle::new(),
            _live: Live::new("ShortcutOverlay"),
        }
    }

    fn set(&mut self, shown: Option<Shown>, cx: &mut Context<Self>) {
        if self.shown != shown {
            if self.shown.is_none() {
                self.scroll.set_offset(Point::default());
            }
            self.shown = shown;
            cx.notify();
        }
    }

    /// ⌘/: shows it until the next key, or hides it.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.hold = None;
        let next = match self.shown {
            Some(Shown::Pinned) => None,
            _ => Some(Shown::Pinned),
        };
        self.set(next, cx);
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.hold = None;
        self.set(None, cx);
    }

    /// ⌘ alone going down starts the hold; anything else with it is a chord on its way.
    pub fn modifiers_changed(
        &mut self,
        e: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let held = held(&e.modifiers, IS_MAC);
        if alone(&e.modifiers, IS_MAC) {
            if self.shown.is_none() && self.hold.is_none() && Settings::get(cx).shortcut_overlay {
                self.hold = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(HOLD).await;
                    // Not over a dialog (⌘P, ⌘K): it would show behind it.
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.hold = None;
                        if !window.has_active_dialog(cx) {
                            this.set(Some(Shown::Held), cx);
                        }
                    });
                }));
            }
            return;
        }
        self.hold = None;
        if self.shown == Some(Shown::Held) && !held {
            self.set(None, cx);
        }
    }

    /// A real key: it ends the wait, and the overlay unless it's the one that toggles it. Escape
    /// is taken; any other key still does its job.
    pub fn key_down(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        self.hold = None;
        if self.shown.is_none() {
            return false;
        }
        let own = keymap::command("workbench.shortcutOverlay")
            .map(|c| bindings_for(c, &Settings::get(cx).keybindings, IS_MAC))
            .unwrap_or_default();
        let chord = keymap::edit::chord_of(&e.keystroke, IS_MAC);
        if chord.as_ref().is_some_and(|c| own.contains(c)) {
            return false;
        }
        self.set(None, cx);
        e.keystroke.key == "escape"
    }

    /// Any keystroke resolved in the window, a dialog's too: it ends the wait, and a held overlay
    /// once ⌘ is up. A modifier let go alone is reported too, late, even after the next ⌘ went
    /// down: that one starts no new wait and ends none.
    pub fn keystroke(&mut self, key: &str, m: &Modifiers, cx: &mut Context<Self>) {
        if !matches!(key, "platform" | "control" | "alt" | "shift" | "function") {
            self.hold = None;
        }
        self.still_held(m, cx);
    }

    /// A click or a move of the pointer: a ⌘-click on its way, which the overlay would swallow.
    pub fn pointer(&mut self) {
        self.hold = None;
    }

    /// An event over the overlay that says what's held. Mission Control (the swipe, F3, ⌃↑)
    /// takes ⌘'s release and the window stays key, so the first move, scroll or click after it
    /// is what says ⌘ is up.
    fn still_held(&mut self, m: &Modifiers, cx: &mut Context<Self>) {
        if self.shown == Some(Shown::Held) && !held(m, IS_MAC) {
            self.hide(cx);
        }
    }
}

/// The key the overlay is held up by: ⌘, or off macOS Ctrl (the keymap's "cmd").
fn held(m: &Modifiers, mac: bool) -> bool {
    if mac { m.platform } else { m.control }
}

/// That key by itself: with another modifier it's a chord on its way, which starts no hold.
fn alone(m: &Modifiers, mac: bool) -> bool {
    let other = if mac { m.control } else { m.platform };
    held(m, mac) && !m.alt && !m.shift && !other
}

fn keycap(key: &str, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    div()
        .min_w(px(16.))
        .px(px(3.))
        .rounded(px(3.))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.background)
        .text_color(t.foreground)
        .text_size(rems(0.625))
        .line_height(rems(0.875))
        .text_center()
        .child(SharedString::from(key.to_string()))
        .into_any_element()
}

fn keys(row: &Row, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    let mut out: Vec<AnyElement> = Vec::new();
    for (i, k) in row.keys.iter().enumerate() {
        if k == THROUGH {
            out.push(div().text_color(t.subtle).child(THROUGH).into_any_element());
            continue;
        }
        // Alternatives, told apart from the keys of one chord.
        if i > 0 && row.keys[i - 1] != THROUGH {
            out.push(
                div()
                    .text_size(rems(0.625))
                    .text_color(t.subtle)
                    .child("/")
                    .into_any_element(),
            );
        }
        out.push(
            div()
                .flex()
                .gap(px(2.))
                .children(chord_keys(k, IS_MAC).iter().map(|key| keycap(key, cx)))
                .into_any_element(),
        );
    }
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .children(out)
        .into_any_element()
}

/// A fade over the list's foot and a cue in it, while more of the list is below. Painted, not
/// laid out: the list's scroll is only known once it's laid out, in this same frame.
fn more_below(scroll: ScrollHandle, t: &AppTheme) -> impl IntoElement {
    const CUE: &str = "↓ Scroll for more";
    let (bg, ink) = (t.elevated, t.muted_foreground);
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            if scroll.max_offset().y + scroll.offset().y <= px(1.) {
                return;
            }
            window.paint_quad(fill(
                bounds,
                linear_gradient(
                    0.,
                    linear_color_stop(bg, 0.4),
                    linear_color_stop(bg.opacity(0.), 1.),
                ),
            ));
            let size = rems(0.6875).to_pixels(window.rem_size());
            let run = TextRun {
                len: CUE.len(),
                font: window.text_style().font(),
                color: ink,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window
                .text_system()
                .shape_line(CUE.into(), size, &[run], None);
            let height = size * 1.4;
            let origin = point(
                bounds.center().x - line.width / 2.,
                bounds.bottom() - height - px(4.),
            );
            let _ = line.paint(origin, height, TextAlign::Left, None, window, cx);
        },
    )
    .absolute()
    .left_0()
    .right_0()
    .bottom_0()
    .h(px(FADE))
}

impl Render for ShortcutOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shown) = self.shown else {
            return div().into_any_element();
        };
        let part = self.workspace.read(cx).focused_part(window, cx);
        let overrides = Settings::get(cx).keybindings.clone();
        let available = window.available_actions(cx);
        let list = groups(&overrides, part, |c| {
            let action = c.action();
            available.iter().any(|a| a.name() == action.name())
                && !window.bindings_for_action(action.as_ref()).is_empty()
        });
        let width = f32::from(window.viewport_size().width);
        let n = ((width.min(1320.) - 64.) / 290.).floor().max(1.) as usize;
        let t = cx.app_theme().clone();
        let hint = match shown {
            Shown::Held if IS_MAC => "Release ⌘ to close",
            Shown::Held => "Release Ctrl to close",
            Shown::Pinned => "Any key closes",
        };
        let column = |groups: Vec<Group>, cx: &App| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap_3()
                .children(groups.into_iter().map(|g| {
                    let t = cx.app_theme();
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .mb_1()
                                .text_size(rems(0.65625))
                                .font_weight(t.semibold())
                                .text_color(t.subtle)
                                .child(g.title.to_uppercase()),
                        )
                        .children(g.rows.into_iter().map(|r| {
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .min_h(px(22.))
                                .text_size(rems(0.71875))
                                .child(div().flex_1().min_w_0().child(r.title.clone()).when_some(
                                    r.place,
                                    |d, place| {
                                        d.child(
                                            div()
                                                .text_size(rems(0.65625))
                                                .text_color(t.subtle)
                                                .child(format!("in {place}")),
                                        )
                                    },
                                ))
                                .child(keys(&r, cx))
                        }))
                }))
        };
        let cols: Vec<_> = columns(list, n)
            .into_iter()
            .map(|g| column(g, cx))
            .collect();
        deferred(
            div()
                .id("shortcut-overlay")
                .debug_selector(|| "shortcut-overlay".into())
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .bg(gpui_kit::black().opacity(0.5))
                .occlude()
                .on_mouse_move(
                    cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                        this.still_held(&e.modifiers, cx)
                    }),
                )
                .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                    this.still_held(&e.modifiers, cx)
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.hide(cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _, _, cx| this.hide(cx)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .max_h_full()
                        .w_full()
                        .max_w(px(1320.))
                        .overflow_hidden()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border_strong)
                        .bg(t.elevated)
                        .shadow_xl()
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_baseline()
                                .gap_3()
                                .px_5()
                                .py_3()
                                .border_b_1()
                                .border_color(t.border)
                                .child(div().font_weight(t.semibold()).child("Keyboard Shortcuts"))
                                .child(
                                    div().text_size(rems(0.71875)).text_color(t.subtle).child(
                                        "Change any of them in Settings → Keyboard Shortcuts",
                                    ),
                                )
                                .child(
                                    div()
                                        .ml_auto()
                                        .text_size(rems(0.71875))
                                        .text_color(t.subtle)
                                        .child(hint),
                                ),
                        )
                        // No scrollbar, as a click anywhere closes it: while more is below, a
                        // fade and a cue over the list's foot say so.
                        .child(
                            div()
                                .relative()
                                .flex()
                                .flex_col()
                                .min_h_0()
                                .child(
                                    div()
                                        .id("shortcut-overlay-list")
                                        .flex()
                                        // Columns at their own heights: stretched to the
                                        // list's, they left nothing to scroll and cut the foot.
                                        .items_start()
                                        .gap_8()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.scroll)
                                        .px_5()
                                        .pt_3()
                                        .pb(px(FADE))
                                        .children(cols),
                                )
                                .child(more_below(self.scroll.clone(), &t)),
                        ),
                ),
        )
        .with_priority(1)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_cmd_holds_it_and_off_macos_ctrl_does() {
        let m = |platform, control, alt, shift| Modifiers {
            platform,
            control,
            alt,
            shift,
            ..Modifiers::default()
        };
        // macOS: ⌘ alone starts it; with ⇧ or ⌥ it's still held but starts nothing.
        assert!(alone(&m(true, false, false, false), true));
        for chord in [
            m(true, false, false, true),
            m(true, false, true, false),
            m(true, true, false, false),
        ] {
            assert!(held(&chord, true) && !alone(&chord, true), "{chord:?}");
        }
        assert!(!held(&m(false, true, false, false), true), "Ctrl isn't ⌘");
        // Elsewhere Ctrl is the key, and the platform (Super) key is the other one.
        assert!(alone(&m(false, true, false, false), false));
        assert!(!held(&m(true, false, false, false), false));
        assert!(
            held(&m(true, true, false, false), false)
                && !alone(&m(true, true, false, false), false)
        );
        assert!(!alone(&Modifiers::default(), true) && !alone(&Modifiers::default(), false));
    }

    #[test]
    fn tabs_read_as_one_row_and_fixed_groups_close_the_list() {
        let list = groups(&keymap::Overrides::new(), None, |_| true);
        let tabs = list.iter().find(|g| g.title.as_ref() == "Tabs").unwrap();
        let goto = tabs
            .rows
            .iter()
            .find(|r| r.title.as_ref() == "Go to Tab 1–8")
            .unwrap();
        assert_eq!(goto.keys.len(), 3);
        assert_eq!(goto.keys[1], THROUGH);
        assert!(
            !tabs
                .rows
                .iter()
                .any(|r| r.title.as_ref().starts_with("Go to Tab 2"))
        );
        let n = list.len();
        assert_eq!(list[n - 2].title.as_ref(), "Navigation");
        assert_eq!(list[n - 1].title.as_ref(), "System");
    }

    #[test]
    fn what_doesnt_run_here_stays_out_but_local_commands_name_their_place() {
        let list = groups(&keymap::Overrides::new(), None, |_| false);
        assert!(list.iter().all(|g| g.rows.iter().all(|r| r.place.is_some()
            || g.title.as_ref() == "Navigation"
            || g.title.as_ref() == "System")));
        assert!(
            list.iter()
                .flat_map(|g| &g.rows)
                .any(|r| r.place == Some("the terminal"))
        );
    }

    #[test]
    fn the_focused_parts_categories_come_first() {
        let list = groups(
            &keymap::Overrides::new(),
            Some(Part::Slot(Slot::Bottom)),
            |_| true,
        );
        assert_eq!(list[0].title.as_ref(), "Terminal");
    }

    #[test]
    fn columns_fill_the_shortest_first() {
        let g = |n: usize| Group {
            title: "x".into(),
            rows: vec![
                Row {
                    title: "r".into(),
                    keys: vec![],
                    place: None,
                };
                n
            ],
        };
        let cols = columns(vec![g(10), g(2), g(2), g(2)], 2);
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].len(), 1);
        assert_eq!(cols[1].len(), 3);
        assert_eq!(columns(vec![g(1)], 3).len(), 1);
    }
}
