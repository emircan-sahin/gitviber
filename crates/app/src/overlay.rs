//! Hold ⌘ by itself for a moment and every shortcut that works where focus is shows over the
//! window, until ⌘ is released. ⌘/ shows the same until the next key or click. It takes the
//! pointer, so nothing underneath hovers through it, and a click on it only closes it.

use std::time::Duration;

use gitviber_ui::keymap::edit::winner;
use gitviber_ui::keymap::{self, Category, IS_MAC, bindings_for, chord_keys};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Part, Slot, Workspace};
use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::{
    AnyElement, App, Context, Entity, InteractiveElement as _, IntoElement, KeyDownEvent,
    ModifiersChangedEvent, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, deferred, div,
    prelude::FluentBuilder as _, px, rems,
};

/// Off macOS "cmd" is Ctrl (the keymap's rule), so that's the key to hold there.
const HOLD: Duration = Duration::from_millis(1450);
/// Between two chords: a range (⌘1 … ⌘8) rather than alternatives.
const THROUGH: &str = "…";

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
        if let Some(local) = c.local {
            add(
                c.category,
                Row {
                    title: c.title.into(),
                    keys,
                    place: Some(local),
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

/// `groups` laid out in `n` columns, each next one into the shortest so far.
pub fn columns(groups: Vec<Group>, n: usize) -> Vec<Vec<Group>> {
    let n = n.max(1);
    let mut cols: Vec<(usize, Vec<Group>)> = (0..n).map(|_| (0, Vec::new())).collect();
    for g in groups {
        let height = g.rows.len() + 2;
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
    _live: Live,
}

impl ShortcutOverlay {
    pub fn new(workspace: Entity<Workspace>) -> Self {
        Self {
            workspace,
            shown: None,
            hold: None,
            _live: Live::new("ShortcutOverlay"),
        }
    }

    fn set(&mut self, shown: Option<Shown>, cx: &mut Context<Self>) {
        if self.shown != shown {
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
        let m = &e.modifiers;
        let held = if IS_MAC { m.platform } else { m.control };
        let alone = held && !m.alt && !m.shift && !(if IS_MAC { m.control } else { m.platform });
        if alone {
            if self.shown.is_none() && self.hold.is_none() && Settings::get(cx).shortcut_overlay {
                self.hold = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(HOLD).await;
                    let _ = this.update(cx, |this, cx| {
                        this.hold = None;
                        this.set(Some(Shown::Held), cx);
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

    /// A click or a move of the pointer: a ⌘-click on its way, which the overlay would swallow.
    pub fn pointer(&mut self) {
        self.hold = None;
    }
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
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
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
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .bg(gpui_kit::black().opacity(0.5))
                .occlude()
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
                                .child(
                                    div()
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .child("Keyboard Shortcuts"),
                                )
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
                        .child(
                            div()
                                .id("shortcut-overlay-list")
                                .flex()
                                .gap_8()
                                .min_h_0()
                                .overflow_y_scroll()
                                .px_5()
                                .pt_3()
                                .pb_4()
                                .children(cols),
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
                .any(|r| r.place == Some("Terminal"))
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
