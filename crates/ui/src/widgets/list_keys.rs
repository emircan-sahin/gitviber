//! The keys that move through a list of rows: one rule for every picker, menu and list.

use std::rc::Rc;

use gpui_kit::{App, InteractiveElement, KeyBinding, KeyDownEvent, Keystroke, Window};

gpui_kit::actions!(list_keys, [ListHome, ListEnd, ListPageUp, ListPageDown]);

/// "ListFilter" in a picker's key context (`"ProjectPicker ListFilter"`): its filter field hands
/// Home/End/PgUp/PgDn to the list. The field's own bindings for them run before any key
/// handler, so a capture_key_down never sees these keys.
pub(crate) fn bind(cx: &mut App) {
    let c = Some("ListFilter > Input");
    cx.bind_keys([
        KeyBinding::new("home", ListHome, c),
        KeyBinding::new("end", ListEnd, c),
        KeyBinding::new("pageup", ListPageUp, c),
        KeyBinding::new("pagedown", ListPageDown, c),
    ]);
}

/// Gives the list keys bound in a "ListFilter" back to `on_key` as key presses. A key it leaves
/// alone (Home/End with text typed) goes on to the field, for its caret.
pub fn on_list_keys<E: InteractiveElement>(
    el: E,
    on_key: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
) -> E {
    let on_key = Rc::new(on_key);
    let key = move |key: &'static str| {
        let on_key = on_key.clone();
        move |window: &mut Window, cx: &mut App| {
            let e = KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap(),
                is_held: false,
                prefer_character_input: false,
            };
            cx.propagate();
            on_key(&e, window, cx);
        }
    };
    let (home, end, up, down) = (key("home"), key("end"), key("pageup"), key("pagedown"));
    el.on_action(move |_: &ListHome, window, cx| home(window, cx))
        .on_action(move |_: &ListEnd, window, cx| end(window, cx))
        .on_action(move |_: &ListPageUp, window, cx| up(window, cx))
        .on_action(move |_: &ListPageDown, window, cx| down(window, cx))
}

/// ↑/↓ past an end: a popover's short list wraps around, a panel's long one stops there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    Wrap,
    Stop,
}

/// Rows a PgUp or PgDn moves.
pub const PAGE: usize = 10;

/// The row `k` moves to from `at` (None: none highlighted yet) in a list of `n`: ↑/↓, Home/End,
/// PgUp/PgDn, without modifiers. `typed`: a filter field holding text has the keys, and Home/End
/// move its cursor instead.
pub fn list_step(
    k: &Keystroke,
    at: Option<usize>,
    n: usize,
    ends: Ends,
    typed: bool,
) -> Option<usize> {
    let m = k.modifiers;
    if n == 0 || m.platform || m.control || m.alt || m.shift {
        return None;
    }
    let last = n - 1;
    let at = at.map(|i| i.min(last));
    let wrap = ends == Ends::Wrap;
    Some(match k.key.as_str() {
        "down" => match at {
            None => 0,
            Some(i) if i == last => {
                if wrap {
                    0
                } else {
                    last
                }
            }
            Some(i) => i + 1,
        },
        "up" => match at {
            None => last,
            Some(0) => {
                if wrap {
                    last
                } else {
                    0
                }
            }
            Some(i) => i - 1,
        },
        "home" if !typed => 0,
        "end" if !typed => last,
        "pageup" => at.unwrap_or(0).saturating_sub(PAGE),
        "pagedown" => at.map_or(0, |i| i + PAGE).min(last),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(key: &str, at: Option<usize>, n: usize, ends: Ends) -> Option<usize> {
        list_step(&Keystroke::parse(key).unwrap(), at, n, ends, false)
    }

    #[test]
    fn arrows_wrap_in_a_popover_and_stop_in_a_panel() {
        assert_eq!(step("down", None, 3, Ends::Wrap), Some(0));
        assert_eq!(step("up", None, 3, Ends::Wrap), Some(2));
        assert_eq!(step("down", Some(2), 3, Ends::Wrap), Some(0));
        assert_eq!(step("up", Some(0), 3, Ends::Wrap), Some(2));
        assert_eq!(step("down", Some(2), 3, Ends::Stop), Some(2));
        assert_eq!(step("up", Some(0), 3, Ends::Stop), Some(0));
        assert_eq!(step("down", Some(0), 3, Ends::Stop), Some(1));
    }

    #[test]
    fn home_end_and_pages_go_to_the_ends_and_stop_there() {
        assert_eq!(step("home", Some(5), 30, Ends::Wrap), Some(0));
        assert_eq!(step("end", Some(5), 30, Ends::Wrap), Some(29));
        assert_eq!(step("pagedown", Some(5), 30, Ends::Wrap), Some(15));
        assert_eq!(step("pagedown", Some(25), 30, Ends::Wrap), Some(29));
        assert_eq!(step("pageup", Some(5), 30, Ends::Wrap), Some(0));
    }

    #[test]
    fn a_shrunk_list_an_empty_one_modifiers_and_typed_text_leave_the_keys_alone() {
        // The highlighted row past the end after a filter: from the last row.
        assert_eq!(step("up", Some(9), 3, Ends::Stop), Some(1));
        assert_eq!(step("down", None, 0, Ends::Wrap), None);
        assert_eq!(step("cmd-down", Some(0), 3, Ends::Wrap), None);
        assert_eq!(step("shift-down", Some(0), 3, Ends::Wrap), None);
        assert_eq!(step("a", Some(0), 3, Ends::Wrap), None);
        let home = Keystroke::parse("home").unwrap();
        assert_eq!(list_step(&home, Some(2), 3, Ends::Wrap, true), None);
        let down = Keystroke::parse("down").unwrap();
        assert_eq!(list_step(&down, Some(0), 3, Ends::Wrap, true), Some(1));
    }
}
