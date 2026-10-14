//! Trusted input for an agent's tab: the mouse, wheel and key events a command becomes, where on
//! the view they land, and the keys an agent holds down, apart from AppKit (macos/native.rs
//! makes them NSEvents) so it can be tested. Points are the page's CSS px from its viewport's
//! top left; keys are a Mac's (kVK_* codes), as a US keyboard has them.

/// A wheel event's failure where AppKit won't make one for the view's window: the page scrolls
/// by script instead.
pub const NO_WHEEL: &str = "No native wheel event here.";

/// What one native pointer event does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pointer {
    Move,
    /// A press; `clicks` is the click count (2 for the second press of a double click).
    Down {
        right: bool,
        clicks: u8,
    },
    Up {
        right: bool,
        clicks: u8,
    },
    /// A move with the left button held.
    Drag,
    /// A wheel turn, in px: up is negative.
    Wheel {
        dy: f64,
    },
}

impl Pointer {
    /// The buttons held while WebKit reads it (`NSEvent.pressedMouseButtons`): pointer events
    /// carry them as `buttons`, and a drag handler checks them.
    pub fn buttons(self) -> usize {
        match self {
            Self::Down { right: false, .. } | Self::Drag => 1,
            Self::Down { right: true, .. } => 2,
            _ => 0,
        }
    }
}

/// A click's events at one point: a move there first, as a mouse gets there, then each press.
pub fn clicks(count: u8, right: bool) -> Vec<Pointer> {
    let mut out = vec![Pointer::Move];
    for clicks in 1..=count.max(1) {
        out.push(Pointer::Down { right, clicks });
        out.push(Pointer::Up { right, clicks });
    }
    out
}

/// How many moves a drag takes from one end to the other: enough for a sortable list's
/// threshold and a slider's steps, few enough to stay quick.
const DRAG_STEPS: usize = 10;

/// A drag from `a` to `b`: pressed at `a`, moved there with the button held, let go at `b`.
pub fn drag(a: (f64, f64), b: (f64, f64)) -> Vec<(Pointer, (f64, f64))> {
    let mut out = vec![
        (Pointer::Move, a),
        (
            Pointer::Down {
                right: false,
                clicks: 1,
            },
            a,
        ),
    ];
    for step in 1..=DRAG_STEPS {
        let t = step as f64 / DRAG_STEPS as f64;
        out.push((
            Pointer::Drag,
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t),
        ));
    }
    out.push((
        Pointer::Up {
            right: false,
            clicks: 1,
        },
        b,
    ));
    out
}

/// The most one wheel event turns, as a mouse's notches do: a page that eases or snaps its own
/// scrolling sees several.
const WHEEL_STEP: u32 = 120;

/// The wheel events that scroll `px` down (up when `up`).
pub fn wheel(px: u32, up: bool) -> Vec<Pointer> {
    let sign = if up { -1.0 } else { 1.0 };
    let mut left = px;
    let mut out = vec![];
    while left > 0 {
        let step = left.min(WHEEL_STEP);
        out.push(Pointer::Wheel {
            dy: sign * f64::from(step),
        });
        left -= step;
    }
    out
}

/// A point in the page's CSS px, in the view's points: CSS px times the page's zoom, from the
/// top left; up from the bottom in a view that isn't flipped.
pub fn to_view(at: (f64, f64), zoom: f64, height: f64, flipped: bool) -> (f64, f64) {
    let (x, y) = (at.0 * zoom, at.1 * zoom);
    (x, if flipped { y } else { height - y })
}

/// Text as `type` puts it in: a line break of any kind is one, so `\r\n` leaves no `\r`.
pub fn as_typed(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Whether `s` was meant as a point, two numbers with a comma, though maybe not a valid one
/// (`-5,10`): a selector never looks like that.
pub fn looks_like_point(s: &str) -> bool {
    let number = |s: &str| {
        let s = s.trim();
        !s.is_empty()
            && s.chars().any(|c| c.is_ascii_digit())
            && s.chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
    };
    s.split_once(',')
        .is_some_and(|(x, y)| number(x) && number(y))
}

/// `412,300` as a point; None for anything else (a ref, a selector).
pub fn point(s: &str) -> Option<(f64, f64)> {
    let (x, y) = s.split_once(',')?;
    let n = |s: &str| {
        let s = s.trim();
        let digits = !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.');
        digits.then(|| s.parse::<f64>().ok()).flatten()
    };
    Some((n(x)?, n(y)?))
}

/// NSEventModifierFlags' bits for the modifiers, and those an arrow or a function key carries.
pub const SHIFT: u64 = 1 << 17;
pub const CONTROL: u64 = 1 << 18;
pub const OPTION: u64 = 1 << 19;
pub const COMMAND: u64 = 1 << 20;
const NUMERIC_PAD: u64 = 1 << 21;
const FUNCTION: u64 = 1 << 23;

/// A key as AppKit sends it: its virtual key code, what it types unshifted and shifted (AppKit's
/// function-key characters for the named ones), the flags it carries itself, and for a modifier
/// its flag.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub code: u16,
    plain: char,
    shifted: char,
    flags: u64,
    pub modifier: Option<u64>,
}

/// One key event: a modifier's change (`flagsChanged`), or a key going down or up.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyStep {
    Flags { code: u16, flags: u64 },
    Down(Stroke),
    Up(Stroke),
}

/// A key's event as AppKit fills it in: `chars` with every modifier, `bare` with Shift only.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub code: u16,
    pub chars: String,
    pub bare: String,
    pub flags: u64,
}

impl Key {
    fn stroke(&self, held: u64) -> Stroke {
        let flags = held | self.flags;
        let bare = if flags & SHIFT != 0 {
            self.shifted
        } else {
            self.plain
        };
        // ⌃ and a letter type its control character, as AppKit has it.
        let chars = if flags & CONTROL != 0 && bare.is_ascii_alphabetic() {
            char::from(bare.to_ascii_lowercase() as u8 & 0x1f)
        } else {
            bare
        };
        Stroke {
            code: self.code,
            chars: chars.to_string(),
            bare: bare.to_string(),
            flags,
        }
    }
}

/// What a US keyboard has on each key: unshifted, shifted, and its virtual key code.
#[rustfmt::skip]
const US: [(char, char, u16); 47] = [
    ('a', 'A', 0x00), ('s', 'S', 0x01), ('d', 'D', 0x02), ('f', 'F', 0x03), ('h', 'H', 0x04),
    ('g', 'G', 0x05), ('z', 'Z', 0x06), ('x', 'X', 0x07), ('c', 'C', 0x08), ('v', 'V', 0x09),
    ('b', 'B', 0x0B), ('q', 'Q', 0x0C), ('w', 'W', 0x0D), ('e', 'E', 0x0E), ('r', 'R', 0x0F),
    ('y', 'Y', 0x10), ('t', 'T', 0x11), ('1', '!', 0x12), ('2', '@', 0x13), ('3', '#', 0x14),
    ('4', '$', 0x15), ('6', '^', 0x16), ('5', '%', 0x17), ('=', '+', 0x18), ('9', '(', 0x19),
    ('7', '&', 0x1A), ('-', '_', 0x1B), ('8', '*', 0x1C), ('0', ')', 0x1D), (']', '}', 0x1E),
    ('o', 'O', 0x1F), ('u', 'U', 0x20), ('[', '{', 0x21), ('i', 'I', 0x22), ('p', 'P', 0x23),
    ('l', 'L', 0x25), ('j', 'J', 0x26), ('\'', '"', 0x27), ('k', 'K', 0x28), (';', ':', 0x29),
    ('\\', '|', 0x2A), (',', '<', 0x2B), ('/', '?', 0x2C), ('n', 'N', 0x2D), ('m', 'M', 0x2E),
    ('.', '>', 0x2F), ('`', '~', 0x32),
];

/// The keys a DOM name says, in any case: what they type (AppKit's characters), their code,
/// and the flags they carry.
#[rustfmt::skip]
const NAMED: [(&str, char, u16, u64); 18] = [
    ("enter", '\r', 0x24, 0), ("return", '\r', 0x24, 0), ("tab", '\t', 0x30, 0),
    ("space", ' ', 0x31, 0), ("backspace", '\u{7f}', 0x33, 0), ("escape", '\u{1b}', 0x35, 0),
    ("esc", '\u{1b}', 0x35, 0), ("delete", '\u{f728}', 0x75, FUNCTION),
    ("home", '\u{f729}', 0x73, FUNCTION), ("end", '\u{f72b}', 0x77, FUNCTION),
    ("pageup", '\u{f72c}', 0x74, FUNCTION), ("pagedown", '\u{f72d}', 0x79, FUNCTION),
    ("arrowleft", '\u{f702}', 0x7B, FUNCTION | NUMERIC_PAD),
    ("arrowright", '\u{f703}', 0x7C, FUNCTION | NUMERIC_PAD),
    ("arrowdown", '\u{f701}', 0x7D, FUNCTION | NUMERIC_PAD),
    ("arrowup", '\u{f700}', 0x7E, FUNCTION | NUMERIC_PAD),
    ("insert", '\u{f727}', 0x72, FUNCTION), ("help", '\u{f746}', 0x72, FUNCTION),
];

/// F1 to F12's codes, in order.
const F_KEYS: [u16; 12] = [
    0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F,
];

/// The modifiers by name, with their flag and their left key's code.
const MODIFIERS: [(&str, u64, u16); 10] = [
    ("shift", SHIFT, 0x38),
    ("control", CONTROL, 0x3B),
    ("ctrl", CONTROL, 0x3B),
    ("alt", OPTION, 0x3A),
    ("option", OPTION, 0x3A),
    ("meta", COMMAND, 0x37),
    ("cmd", COMMAND, 0x37),
    ("command", COMMAND, 0x37),
    // Playwright's name for the platform's shortcut key.
    ("controlormeta", COMMAND, 0x37),
    ("super", COMMAND, 0x37),
];

/// A key by its DOM name (Enter, ArrowDown, F5, Shift, Meta) or what it types (a, A, 7, ?);
/// None for one a US keyboard has no key for.
pub fn key(name: &str) -> Option<Key> {
    let lower = match name.to_ascii_lowercase().as_str() {
        " " => "space".to_string(),
        l @ ("up" | "down" | "left" | "right") => format!("arrow{l}"),
        l => l.to_string(),
    };
    if let Some(&(_, flag, code)) = MODIFIERS.iter().find(|m| m.0 == lower) {
        return Some(Key {
            code,
            plain: '\0',
            shifted: '\0',
            flags: 0,
            modifier: Some(flag),
        });
    }
    if let Some(&(_, c, code, flags)) = NAMED.iter().find(|n| n.0 == lower) {
        return Some(Key {
            code,
            plain: c,
            shifted: if c == '\t' { '\u{19}' } else { c },
            flags,
            modifier: None,
        });
    }
    if let Some(n) = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|n| (1..=12).contains(n))
    {
        let c = char::from_u32(0xf704 + n as u32 - 1)?;
        return Some(Key {
            code: F_KEYS[n - 1],
            plain: c,
            shifted: c,
            flags: FUNCTION,
            modifier: None,
        });
    }
    let mut chars = name.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let (plain, shifted, code) = *US.iter().find(|k| k.0 == c || k.1 == c)?;
    Some(Key {
        code,
        plain,
        shifted,
        // A capital or a shifted sign is typed with Shift.
        flags: if c == shifted && c != plain { SHIFT } else { 0 },
        modifier: None,
    })
}

/// Whether a chord going down on a focused <select> opens its menu, as Space and the up and
/// down arrows do on a Mac; Tab, Escape, Enter and the rest don't.
pub fn opens_menu(chord: &str) -> bool {
    let Ok((_, last)) = self::chord(chord) else {
        return false;
    };
    key(last).is_some_and(|k| matches!(k.code, 0x31 | 0x7D | 0x7E))
}

/// `Meta+Shift+a`: the modifiers' names and the key's; `+` alone, or last, is the plus key.
pub fn chord(chord: &str) -> Result<(Vec<&str>, &str), String> {
    let (mods, last) = match chord.strip_suffix("++") {
        Some(mods) => (mods, "+"),
        None => match chord.rsplit_once('+') {
            Some((mods, last)) if !last.is_empty() => (mods, last),
            _ => ("", chord),
        },
    };
    let mods: Vec<&str> = mods.split('+').filter(|m| !m.is_empty()).collect();
    for m in &mods {
        if key(m).and_then(|k| k.modifier).is_none() {
            return Err(format!(
                "Not a modifier: {m} (Meta, Control, Alt, Shift) in {chord}"
            ));
        }
    }
    if key(last).is_none() {
        return Err(format!(
            "No key {last:?} on a US keyboard: a name (Enter, Tab, ArrowDown, F5, Shift) or a \
             character it types. Text of any language goes in with `type`"
        ));
    }
    Ok((mods, last))
}

/// What the Edit menu does for a ⌘ key the page let through, done in the page itself: a key
/// event alone selects or copies nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit {
    SelectAll,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
}

/// What a key command asks of the tab.
#[derive(Clone, Debug, PartialEq)]
pub enum Keys {
    /// A chord pressed and let go, or half of it for `press --hold`: to the key down, or the
    /// rest.
    Press(String, Half),
    Down(String),
    Up(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Half {
    Both,
    Down,
    Up,
}

/// The keys an agent holds down on a tab, kept from one command to the next until it lets them
/// go: modifiers go with later keys and clicks, and a held key (a game's W) stays down.
#[derive(Default, Debug)]
pub struct Held {
    flags: u64,
    keys: Vec<Key>,
}

impl Held {
    /// The modifiers held, as NSEventModifierFlags.
    pub fn flags(&self) -> u64 {
        self.flags
    }

    fn holds(&self, key: &Key) -> bool {
        self.keys.iter().any(|k| k.code == key.code)
    }

    /// Every held key let go, then every modifier: the pane that held them closed.
    pub fn release_all(&mut self) -> Vec<KeyStep> {
        let mut steps: Vec<KeyStep> = self
            .keys
            .drain(..)
            .map(|k| KeyStep::Up(k.stroke(self.flags)))
            .collect();
        for (_, flag, code) in MODIFIERS {
            if self.flags & flag != 0 {
                self.flags &= !flag;
                steps.push(KeyStep::Flags {
                    code,
                    flags: self.flags,
                });
            }
        }
        steps
    }

    /// A chord pressed and let go: its modifiers down that aren't already, the key down and up,
    /// those modifiers up again; held ones stay as they were. With it, what the Edit menu would
    /// do for it. Halved, it stops at the key down, or goes on from there.
    pub fn press(&self, chord: &str, half: Half) -> Result<(Vec<KeyStep>, Option<Edit>), String> {
        let (mods, last) = self::chord(chord)?;
        let key = self::key(last).ok_or("No such key")?;
        let mut flags = self.flags;
        let mut steps = vec![];
        let mut added = vec![];
        for m in mods.iter().filter_map(|m| self::key(m)) {
            let flag = m.modifier.unwrap_or_default();
            if flags & flag == 0 {
                flags |= flag;
                steps.push(KeyStep::Flags {
                    code: m.code,
                    flags,
                });
                added.push((m.code, flag));
            }
        }
        let mut edit = None;
        let mut up_at = steps.len();
        match key.modifier {
            Some(flag) if flags & flag == 0 => {
                steps.push(KeyStep::Flags {
                    code: key.code,
                    flags: flags | flag,
                });
                up_at = steps.len();
                steps.push(KeyStep::Flags {
                    code: key.code,
                    flags,
                });
            }
            Some(_) => {}
            // Held down already (`key down`): it stays down, as a key can't go down twice.
            None if self.holds(&key) => {}
            None => {
                let stroke = key.stroke(flags);
                edit = edit_of(stroke.flags, key.plain);
                steps.push(KeyStep::Down(stroke.clone()));
                up_at = steps.len();
                steps.push(KeyStep::Up(stroke));
            }
        }
        for (code, flag) in added.into_iter().rev() {
            flags &= !flag;
            steps.push(KeyStep::Flags { code, flags });
        }
        Ok(match half {
            Half::Both => (steps, edit),
            Half::Down => (steps[..up_at].to_vec(), edit),
            Half::Up => (steps.split_off(up_at), None),
        })
    }

    /// A key held down from now: a modifier with what comes after, any other until `up`.
    pub fn down(&mut self, name: &str) -> Result<Vec<KeyStep>, String> {
        let key = self::key(name).ok_or_else(|| unknown(name))?;
        match key.modifier {
            Some(flag) if self.flags & flag != 0 => Err(format!("{name} is held already")),
            Some(flag) => {
                self.flags |= flag;
                Ok(vec![KeyStep::Flags {
                    code: key.code,
                    flags: self.flags,
                }])
            }
            None if self.holds(&key) => Err(format!("{name} is held already")),
            None => {
                let down = KeyStep::Down(key.stroke(self.flags));
                self.keys.push(key);
                Ok(vec![down])
            }
        }
    }

    /// A held key let go.
    pub fn up(&mut self, name: &str) -> Result<Vec<KeyStep>, String> {
        let key = self::key(name).ok_or_else(|| unknown(name))?;
        let held = match key.modifier {
            Some(flag) => self.flags & flag != 0,
            None => self.holds(&key),
        };
        if !held {
            return Err(format!("{name} isn't held: `key down {name}` first"));
        }
        Ok(match key.modifier {
            Some(flag) => {
                self.flags &= !flag;
                vec![KeyStep::Flags {
                    code: key.code,
                    flags: self.flags,
                }]
            }
            None => {
                self.keys.retain(|k| k.code != key.code);
                vec![KeyStep::Up(key.stroke(self.flags))]
            }
        })
    }
}

pub fn unknown(name: &str) -> String {
    format!("No key {name:?} on a US keyboard: Enter, Tab, ArrowDown, F5, Shift, w, ...")
}

/// ⌘A, ⌘C, ⌘X, ⌘V, ⌘Z and ⇧⌘Z, with no other modifier.
fn edit_of(flags: u64, plain: char) -> Option<Edit> {
    let mods = flags & (SHIFT | CONTROL | OPTION | COMMAND);
    match (mods, plain) {
        (COMMAND, 'a') => Some(Edit::SelectAll),
        (COMMAND, 'c') => Some(Edit::Copy),
        (COMMAND, 'x') => Some(Edit::Cut),
        (COMMAND, 'v') => Some(Edit::Paste),
        (COMMAND, 'z') => Some(Edit::Undo),
        (m, 'z') if m == COMMAND | SHIFT => Some(Edit::Redo),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_moves_there_first_and_a_double_click_counts_its_presses() {
        let down = |clicks| Pointer::Down {
            right: false,
            clicks,
        };
        let up = |clicks| Pointer::Up {
            right: false,
            clicks,
        };
        assert_eq!(clicks(1, false), [Pointer::Move, down(1), up(1)]);
        assert_eq!(
            clicks(2, false),
            [Pointer::Move, down(1), up(1), down(2), up(2)]
        );
        assert_eq!(
            clicks(1, true)[1],
            Pointer::Down {
                right: true,
                clicks: 1
            }
        );
        // Pressed, the button is held as WebKit reads it; let go or moving, none is.
        assert_eq!(down(1).buttons(), 1);
        assert_eq!(clicks(1, true)[1].buttons(), 2);
        assert_eq!(up(1).buttons(), 0);
        assert_eq!(Pointer::Move.buttons(), 0);
    }

    #[test]
    fn a_drag_holds_the_button_on_every_move_and_ends_where_it_was_going() {
        let steps = drag((10.0, 20.0), (110.0, 220.0));
        assert_eq!(steps[0], (Pointer::Move, (10.0, 20.0)));
        assert_eq!(steps[1].0.buttons(), 1);
        let moves: Vec<_> = steps.iter().filter(|s| s.0 == Pointer::Drag).collect();
        assert_eq!(moves.len(), DRAG_STEPS);
        assert!(moves.iter().all(|m| m.0.buttons() == 1));
        assert_eq!(moves[0].1, (20.0, 40.0));
        assert_eq!(moves.last().unwrap().1, (110.0, 220.0));
        assert_eq!(
            steps.last(),
            Some(&(
                Pointer::Up {
                    right: false,
                    clicks: 1
                },
                (110.0, 220.0)
            ))
        );
    }

    #[test]
    fn the_wheel_turns_in_notches_that_add_up_to_the_distance() {
        let dys = |p: Vec<Pointer>| -> Vec<f64> {
            p.into_iter()
                .map(|w| match w {
                    Pointer::Wheel { dy } => dy,
                    _ => panic!("{w:?}"),
                })
                .collect()
        };
        assert_eq!(dys(wheel(300, false)), [120.0, 120.0, 60.0]);
        assert_eq!(dys(wheel(50, true)), [-50.0]);
        assert!(wheel(0, false).is_empty());
    }

    #[test]
    fn css_px_land_on_the_view_by_the_pages_zoom_either_way_up() {
        assert_eq!(to_view((100.0, 40.0), 1.0, 800.0, true), (100.0, 40.0));
        assert_eq!(to_view((100.0, 40.0), 1.0, 800.0, false), (100.0, 760.0));
        // Device mode zooms a phone's 390 px into a narrower tab.
        assert_eq!(to_view((100.0, 40.0), 0.5, 800.0, true), (50.0, 20.0));
        assert_eq!(to_view((100.0, 40.0), 1.5, 800.0, false), (150.0, 740.0));
    }

    #[test]
    fn a_point_is_two_numbers_and_anything_else_is_a_ref_or_selector() {
        assert_eq!(point("412,300"), Some((412.0, 300.0)));
        assert_eq!(point(" 12.5 , 7 "), Some((12.5, 7.0)));
        for no in ["e3", "a, b", "-1,2", "1,", ",2", "1,2,3", "1e3,2", "#x"] {
            assert_eq!(point(no), None, "{no}");
        }
    }

    #[test]
    fn keys_are_a_macs_codes_by_dom_name_or_by_what_they_type() {
        let enter = key("Enter").unwrap();
        assert_eq!((enter.code, enter.stroke(0).chars.as_str()), (0x24, "\r"));
        assert_eq!(key("return"), key("Enter"));
        assert_eq!(key("ArrowDown").unwrap().code, 0x7D);
        assert_eq!(key("down"), key("ArrowDown"));
        assert_eq!(key("ArrowUp").unwrap().stroke(0).chars, "\u{f700}");
        assert_eq!(key("F5").unwrap().code, 0x60);
        assert_eq!(key("f12").unwrap().stroke(0).chars, "\u{f70f}");
        assert_eq!(key("Shift").unwrap().modifier, Some(SHIFT));
        assert_eq!(key("Meta").unwrap().code, 0x37);
        // A capital is the letter's key with Shift.
        let a = key("A").unwrap().stroke(0);
        assert_eq!((a.code, a.chars.as_str(), a.flags), (0x00, "A", SHIFT));
        let q = key("?").unwrap().stroke(0);
        assert_eq!((q.code, q.chars.as_str(), q.flags), (0x2C, "?", SHIFT));
        assert_eq!(key("w").unwrap().stroke(0).flags, 0);
        // Not Windows' codes: those flooded a page with keydowns elsewhere.
        assert_ne!(key("Enter").unwrap().code, 13);
        for none in ["ş", "é", "F13", "Hyper", "", "ab"] {
            assert_eq!(key(none), None, "{none}");
        }
    }

    #[test]
    fn letters_and_digits_have_the_code_the_page_reads_back() {
        // keys.rs reads codes back as DOM ones: each key here lands on the same physical key.
        for (plain, _, code) in US {
            let dom = crate::browser::keys::code(code);
            let expected = match plain {
                'a'..='z' => format!("Key{}", plain.to_ascii_uppercase()),
                '0'..='9' => format!("Digit{plain}"),
                _ => continue,
            };
            assert_eq!(dom, expected, "{plain}");
        }
        assert_eq!(crate::browser::keys::code(key("Tab").unwrap().code), "Tab");
        assert_eq!(
            crate::browser::keys::code(key("Escape").unwrap().code),
            "Escape"
        );
        assert_eq!(
            crate::browser::keys::code(key("Backspace").unwrap().code),
            "Backspace"
        );
    }

    #[test]
    fn a_press_pairs_down_and_up_inside_its_modifiers() {
        let held = Held::default();
        let (steps, edit) = held.press("Meta+Shift+z", Half::Both).unwrap();
        assert_eq!(edit, Some(Edit::Redo));
        assert_eq!(
            steps[..2],
            [
                KeyStep::Flags {
                    code: 0x37,
                    flags: COMMAND
                },
                KeyStep::Flags {
                    code: 0x38,
                    flags: COMMAND | SHIFT
                },
            ]
        );
        let KeyStep::Down(down) = &steps[2] else {
            panic!("{steps:?}")
        };
        assert_eq!((down.chars.as_str(), down.flags), ("Z", COMMAND | SHIFT));
        assert_eq!(steps[3], KeyStep::Up(down.clone()));
        // Let go in reverse, back to none held.
        assert_eq!(
            steps[4..],
            [
                KeyStep::Flags {
                    code: 0x38,
                    flags: COMMAND
                },
                KeyStep::Flags {
                    code: 0x37,
                    flags: 0
                },
            ]
        );
        assert_eq!(
            held.press("Meta+a", Half::Both).unwrap().1,
            Some(Edit::SelectAll)
        );
        assert_eq!(held.press("Control+a", Half::Both).unwrap().1, None);
        assert_eq!(held.press("Enter", Half::Both).unwrap().0.len(), 2);
        // ⌃A types its control character, as AppKit's event has it.
        let (steps, _) = held.press("Control+a", Half::Both).unwrap();
        assert!(matches!(&steps[1], KeyStep::Down(s) if s.chars == "\u{1}" && s.bare == "a"));
        // The plus key, alone or last.
        assert_eq!(chord("Shift++"), Ok((vec!["Shift"], "+")));
        assert_eq!(chord("+"), Ok((vec![], "+")));
        assert!(held.press("Hyper+a", Half::Both).is_err());
        assert!(held.press("Meta+ş", Half::Both).is_err());
        // Held down for a while: the first half ends at the key down, the rest lets go.
        let (down, edit) = held.press("Meta+a", Half::Down).unwrap();
        assert!(matches!(down.last(), Some(KeyStep::Down(_))) && edit.is_some());
        let (up, edit) = held.press("Meta+a", Half::Up).unwrap();
        assert!(matches!(up.first(), Some(KeyStep::Up(_))) && edit.is_none());
        assert_eq!(
            [down, up].concat(),
            held.press("Meta+a", Half::Both).unwrap().0
        );
    }

    #[test]
    fn held_keys_stay_down_across_commands_until_let_go() {
        let mut held = Held::default();
        assert_eq!(
            held.down("Shift").unwrap(),
            [KeyStep::Flags {
                code: 0x38,
                flags: SHIFT
            }]
        );
        assert!(held.down("Shift").is_err());
        // A press now types as with Shift held, and leaves Shift held.
        let (steps, _) = held.press("a", Half::Both).unwrap();
        assert!(matches!(&steps[0], KeyStep::Down(s) if s.chars == "A" && s.flags == SHIFT));
        assert_eq!(steps.len(), 2);
        assert_eq!(held.flags(), SHIFT);
        // A press of Shift itself changes nothing held.
        assert!(held.press("Shift", Half::Both).unwrap().0.is_empty());
        let w = held.down("w").unwrap();
        assert!(matches!(&w[..], [KeyStep::Down(s)] if s.code == 0x0D));
        assert!(held.down("W").is_err(), "the same key");
        assert!(held.up("x").is_err());
        assert!(matches!(&held.up("w").unwrap()[..], [KeyStep::Up(s)] if s.code == 0x0D));
        assert_eq!(
            held.up("shift").unwrap(),
            [KeyStep::Flags {
                code: 0x38,
                flags: 0
            }]
        );
        assert_eq!(held.flags(), 0);
        assert!(held.up("Shift").is_err());
    }

    /// Every key the US table has, by either character: one code, that character typed, Shift
    /// only for the shifted one, and the DOM code keys.rs reads back names that physical key.
    #[test]
    fn every_us_key_types_its_own_character_by_either_of_its_characters() {
        let dom = |c: char| match c {
            'a'..='z' => format!("Key{}", c.to_ascii_uppercase()),
            '0'..='9' => format!("Digit{c}"),
            '-' => "Minus".into(),
            '=' => "Equal".into(),
            '[' => "BracketLeft".into(),
            ']' => "BracketRight".into(),
            '\\' => "Backslash".into(),
            ';' => "Semicolon".into(),
            '\'' => "Quote".into(),
            ',' => "Comma".into(),
            '.' => "Period".into(),
            '/' => "Slash".into(),
            '`' => "Backquote".into(),
            _ => panic!("{c}"),
        };
        let mut codes = std::collections::HashSet::new();
        for (plain, shifted, code) in US {
            assert!(codes.insert(code), "{plain}: code {code:#x} twice");
            assert_eq!(crate::browser::keys::code(code), dom(plain), "{plain}");
            let p = key(&plain.to_string()).unwrap();
            let s = key(&shifted.to_string()).unwrap();
            assert_eq!((p.code, s.code), (code, code), "{plain}{shifted}");
            assert_eq!((p.modifier, s.modifier), (None, None));
            let (ps, ss) = (p.stroke(0), s.stroke(0));
            assert_eq!((ps.chars.as_str(), ps.flags), (&*plain.to_string(), 0));
            assert_eq!(
                (ss.chars.as_str(), ss.flags),
                (&*shifted.to_string(), SHIFT)
            );
            // Shift held on the plain one types the shifted one, as the key itself does.
            assert_eq!(p.stroke(SHIFT), ss, "{plain} with Shift held");
            assert_eq!(ss.bare, ss.chars);
        }
        // Every letter and digit, and each sign a US keyboard types.
        assert_eq!(US.len(), 26 + 10 + 11);
        for c in ('a'..='z')
            .chain('A'..='Z')
            .chain('0'..='9')
            .chain("`~!@#$%^&*()-_=+[]{}\\|;:'\",<.>/?".chars())
        {
            assert!(key(&c.to_string()).is_some(), "{c}");
        }
    }

    /// The named keys in any case, with the codes and AppKit characters a Mac keyboard gives.
    #[test]
    fn named_keys_in_any_case_have_a_macs_codes_and_characters() {
        #[rustfmt::skip]
        let named: [(&str, u16, char, u64); 18] = [
            ("Enter", 0x24, '\r', 0), ("Return", 0x24, '\r', 0), ("Tab", 0x30, '\t', 0),
            ("Space", 0x31, ' ', 0), ("Backspace", 0x33, '\u{7f}', 0),
            ("Escape", 0x35, '\u{1b}', 0), ("Esc", 0x35, '\u{1b}', 0),
            ("Delete", 0x75, '\u{f728}', FUNCTION), ("Home", 0x73, '\u{f729}', FUNCTION),
            ("End", 0x77, '\u{f72b}', FUNCTION), ("PageUp", 0x74, '\u{f72c}', FUNCTION),
            ("PageDown", 0x79, '\u{f72d}', FUNCTION),
            ("ArrowLeft", 0x7B, '\u{f702}', FUNCTION | NUMERIC_PAD),
            ("ArrowRight", 0x7C, '\u{f703}', FUNCTION | NUMERIC_PAD),
            ("ArrowDown", 0x7D, '\u{f701}', FUNCTION | NUMERIC_PAD),
            ("ArrowUp", 0x7E, '\u{f700}', FUNCTION | NUMERIC_PAD),
            ("Insert", 0x72, '\u{f727}', FUNCTION), ("Help", 0x72, '\u{f746}', FUNCTION),
        ];
        for (name, code, c, flags) in named {
            for spelt in [
                name.to_string(),
                name.to_ascii_lowercase(),
                name.to_ascii_uppercase(),
            ] {
                let k = key(&spelt).unwrap_or_else(|| panic!("{spelt}"));
                let s = k.stroke(0);
                assert_eq!(
                    (k.code, s.chars.as_str(), s.flags),
                    (code, &*c.to_string(), flags)
                );
                assert_eq!(k.modifier, None);
            }
        }
        assert_eq!(key(" "), key("Space"));
        for (short, long) in [
            ("Up", "ArrowUp"),
            ("DOWN", "ArrowDown"),
            ("left", "ArrowLeft"),
        ] {
            assert_eq!(key(short), key(long), "{short}");
        }
        // Shift+Tab is AppKit's back tab; Shift leaves the others' characters as they are.
        assert_eq!(key("Tab").unwrap().stroke(SHIFT).chars, "\u{19}");
        assert_eq!(key("Enter").unwrap().stroke(SHIFT).chars, "\r");
        const F: [u16; 12] = [
            0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F,
        ];
        for (n, code) in (1..=12u32).zip(F) {
            for spelt in [format!("F{n}"), format!("f{n}")] {
                let k = key(&spelt).unwrap();
                let s = k.stroke(0);
                let c = char::from_u32(0xf703 + n).unwrap();
                assert_eq!((k.code, s.chars, s.flags), (code, c.to_string(), FUNCTION));
            }
        }
        for none in [
            "F0", "F13", "F01x", "f", "Fn", "Enterr", "Arrow", "Del", "KeyA",
        ] {
            // "f" alone is the letter; the others aren't keys.
            if none == "f" {
                assert_eq!(key(none).unwrap().code, 0x03);
            } else {
                assert_eq!(key(none), None, "{none}");
            }
        }
        #[rustfmt::skip]
        let mods = [
            ("Shift", SHIFT, 0x38), ("Control", CONTROL, 0x3B), ("Ctrl", CONTROL, 0x3B),
            ("Alt", OPTION, 0x3A), ("Option", OPTION, 0x3A), ("Meta", COMMAND, 0x37),
            ("Cmd", COMMAND, 0x37), ("Command", COMMAND, 0x37), ("ControlOrMeta", COMMAND, 0x37),
            ("Super", COMMAND, 0x37),
        ];
        for (name, flag, code) in mods {
            for spelt in [
                name.to_string(),
                name.to_ascii_uppercase(),
                name.to_ascii_lowercase(),
            ] {
                let k = key(&spelt).unwrap();
                assert_eq!((k.modifier, k.code), (Some(flag), code), "{spelt}");
            }
        }
    }

    /// Turkish letters, and any other a US keyboard has no key for, are refused by name, with
    /// the way to type them: never sent as some other key.
    #[test]
    fn letters_no_us_key_types_are_refused_and_sent_to_type() {
        let mut held = Held::default();
        for c in [
            "ğ", "ü", "ş", "ı", "ö", "ç", "Ğ", "Ü", "Ş", "İ", "Ö", "Ç", "é", "ß", "€", "£", "§",
            "±", "ä", "ñ", "日", "😀", "\t", "\n", "\u{0}",
        ] {
            assert_eq!(key(c), None, "{c:?}");
            let e = chord(c).unwrap_err();
            assert!(e.contains("`type`"), "{c:?}: {e}");
            assert!(chord(&format!("Meta+{c}")).is_err(), "Meta+{c:?}");
            assert!(held.press(c, Half::Both).is_err(), "{c:?}");
            assert!(held.down(c).is_err(), "{c:?}");
            assert!(held.up(c).is_err(), "{c:?}");
        }
        // Lowercasing İ or ı mustn't land on i's key, nor K (Kelvin sign) on k's.
        for c in ["İ", "ı", "\u{212a}", "\u{17f}"] {
            assert_eq!(key(c), None, "{c:?}");
        }
        assert_eq!(held.flags(), 0);
    }

    #[test]
    fn chords_parse_modifiers_in_any_case_and_refuse_what_isnt_one() {
        fn ok(c: &str) -> (Vec<&str>, &str) {
            chord(c).unwrap_or_else(|e| panic!("{c}: {e}"))
        }
        assert_eq!(ok("Meta+Shift+a"), (vec!["Meta", "Shift"], "a"));
        assert_eq!(ok("Control+Alt+Delete"), (vec!["Control", "Alt"], "Delete"));
        assert_eq!(ok("META+shift+A"), (vec!["META", "shift"], "A"));
        assert_eq!(ok("Meta++"), (vec!["Meta"], "+"));
        assert_eq!(ok("Meta+++"), (vec!["Meta"], "+"));
        assert_eq!(ok("++"), (vec![], "+"));
        assert_eq!(ok("Shift+Control"), (vec!["Shift"], "Control"));
        for bad in [
            "",
            "Shift+",
            "Meta+",
            "a+b",
            "Hyper+x",
            "Meta+Hyper+x",
            "Meta + a",
            " a",
            "Meta+ab",
            "Enter+a",
            "F5+a",
        ] {
            assert!(chord(bad).is_err(), "{bad:?}");
        }
        // A modifier named twice goes down once, and comes up once.
        let held = Held::default();
        let (steps, _) = held.press("Ctrl+Control+a", Half::Both).unwrap();
        let flags = steps
            .iter()
            .filter(|s| matches!(s, KeyStep::Flags { .. }))
            .count();
        assert_eq!(flags, 2, "{steps:?}");
        // ⌃⌥⌦ carries every flag it should.
        let (steps, _) = held.press("Control+Alt+Delete", Half::Both).unwrap();
        let down = steps
            .iter()
            .find_map(|s| match s {
                KeyStep::Down(s) => Some(s),
                _ => None,
            })
            .unwrap();
        assert_eq!((down.code, down.flags), (0x75, CONTROL | OPTION | FUNCTION));
        // A chord that ends on a modifier presses that modifier inside the others.
        let (steps, edit) = held.press("Shift+Control", Half::Both).unwrap();
        assert_eq!(edit, None);
        assert_eq!(
            steps,
            [
                KeyStep::Flags {
                    code: 0x38,
                    flags: SHIFT
                },
                KeyStep::Flags {
                    code: 0x3B,
                    flags: SHIFT | CONTROL
                },
                KeyStep::Flags {
                    code: 0x3B,
                    flags: SHIFT
                },
                KeyStep::Flags {
                    code: 0x38,
                    flags: 0
                },
            ]
        );
    }

    /// Every press, whatever is held: each key down has its up, the modifiers it added come up
    /// again so the flags end where they were held, the halves make the whole, and the press
    /// changes nothing held.
    #[test]
    fn every_press_ends_with_the_held_flags_and_its_halves_make_the_whole() {
        let chords = [
            "a",
            "A",
            "?",
            "Enter",
            "Shift+Tab",
            "Meta+a",
            "Meta+Shift+z",
            "Meta+Z",
            "Control+a",
            "Control+Shift+a",
            "Alt+ArrowLeft",
            "Control+Alt+Delete",
            "Shift",
            "Meta",
            "Meta+Shift",
            "Shift+Shift+x",
            "F5",
            "Meta++",
            "+",
            "Control+Alt+Shift+Meta+k",
            "ControlOrMeta+c",
        ];
        for held_mods in [
            &[][..],
            &["Shift"],
            &["Meta"],
            &["Control", "Alt"],
            &["Shift", "Meta"],
        ] {
            let mut held = Held::default();
            for m in held_mods {
                held.down(m).unwrap();
            }
            let before = held.flags();
            for c in chords {
                let (all, edit) = held.press(c, Half::Both).unwrap();
                let (down, down_edit) = held.press(c, Half::Down).unwrap();
                let (up, up_edit) = held.press(c, Half::Up).unwrap();
                assert_eq!([down, up].concat(), all, "{c} with {held_mods:?}");
                assert_eq!((down_edit, up_edit), (edit, None), "{c}");
                assert_eq!(held.flags(), before, "{c} changed what's held");
                // The last flags change goes back to what's held.
                if let Some(KeyStep::Flags { flags, .. }) = all
                    .iter()
                    .rev()
                    .find(|s| matches!(s, KeyStep::Flags { .. }))
                {
                    assert_eq!(*flags, before, "{c} with {held_mods:?}: {all:?}");
                }
                let downs: Vec<_> = all
                    .iter()
                    .filter_map(|s| match s {
                        KeyStep::Down(s) => Some(s),
                        _ => None,
                    })
                    .collect();
                let ups: Vec<_> = all
                    .iter()
                    .filter_map(|s| match s {
                        KeyStep::Up(s) => Some(s),
                        _ => None,
                    })
                    .collect();
                assert_eq!(downs, ups, "{c}");
                // Each key event carries what's held at least.
                for s in downs {
                    assert_eq!(s.flags & before, before, "{c}");
                }
            }
        }
    }

    /// A failed key command leaves nothing held that wasn't, and lets go of nothing that was.
    #[test]
    fn a_failed_key_command_changes_nothing_held() {
        let mut held = Held::default();
        held.down("Shift").unwrap();
        held.down("w").unwrap();
        for bad in ["Hyper+x", "ş", "Meta+", "", "Shift+Hyper"] {
            assert!(held.press(bad, Half::Both).is_err(), "{bad}");
            assert!(held.press(bad, Half::Down).is_err(), "{bad}");
        }
        for bad in ["Hyper", "ğ", "", "Shift", "w", "W"] {
            assert!(held.down(bad).is_err(), "down {bad}");
        }
        for bad in ["Hyper", "İ", "Meta", "x", "Control"] {
            assert!(held.up(bad).is_err(), "up {bad}");
        }
        assert_eq!(held.flags(), SHIFT);
        // Both still held, and let go once each.
        assert_eq!(held.up("W").unwrap().len(), 1);
        assert!(held.up("w").is_err());
        assert_eq!(held.up("SHIFT").unwrap().len(), 1);
        assert_eq!(held.flags(), 0);
        // A key up with no key down first is refused for every kind of key.
        for k in ["a", "Enter", "F5", "Shift", "Meta", "ArrowUp"] {
            assert!(held.up(k).is_err(), "{k}");
        }
    }

    /// A key held down and then pressed again goes down a second time with no up between, and
    /// its `key up` then sends an up of its own: two ups for a key a keyboard sent one for.
    #[test]
    fn pressing_a_key_already_held_down_doesnt_send_a_second_down() {
        let mut held = Held::default();
        held.down("w").unwrap();
        let (steps, _) = held.press("w", Half::Both).unwrap();
        assert!(
            !steps
                .iter()
                .any(|s| matches!(s, KeyStep::Down(s) if s.code == 0x0D)),
            "{steps:?}"
        );
    }

    #[test]
    fn points_take_fractions_and_refuse_signs_and_other_numbers() {
        assert_eq!(point("0,0"), Some((0.0, 0.0)));
        assert_eq!(point("0.25,1999.75"), Some((0.25, 1999.75)));
        assert_eq!(point("5.,3"), Some((5.0, 3.0)));
        assert_eq!(point(".5,3"), Some((0.5, 3.0)));
        for no in [
            "+5,3",
            "-0,3",
            "5,-3",
            ".,3",
            "1..2,3",
            "1.2.3,4",
            "0x10,5",
            "inf,1",
            "NaN,1",
            "\u{661},2",
            "1,2 ",
            "",
            ",",
            " , ",
            "1;2",
            "1 2",
        ] {
            // "1,2 " has its spaces trimmed: a point all the same.
            if no == "1,2 " {
                assert_eq!(point(no), Some((1.0, 2.0)));
                continue;
            }
            assert_eq!(point(no), None, "{no:?}");
        }
    }

    #[test]
    fn view_points_scale_by_zoom_and_flip_about_the_views_height() {
        // Fractional CSS px stay fractional; a point past the bottom lands below an unflipped
        // view's origin, one past the right past its width: no clamping here.
        assert_eq!(to_view((10.5, 0.25), 2.0, 600.0, true), (21.0, 0.5));
        assert_eq!(to_view((10.5, 0.25), 2.0, 600.0, false), (21.0, 599.5));
        assert_eq!(to_view((0.0, 700.0), 1.0, 600.0, false), (0.0, -100.0));
        assert_eq!(to_view((5000.0, 0.0), 1.0, 600.0, true), (5000.0, 0.0));
        // Flipping twice is the identity, at any zoom.
        for zoom in [0.25, 0.5, 1.0, 1.25, 3.0] {
            let (x, y) = to_view((123.0, 45.0), zoom, 800.0, false);
            assert_eq!((x, 800.0 - y), to_view((123.0, 45.0), zoom, 800.0, true));
        }
    }

    #[test]
    fn drags_step_evenly_either_way_holding_only_the_left_button() {
        for (a, b) in [
            ((0.0, 0.0), (0.0, 0.0)),
            ((300.0, 40.0), (10.0, 400.0)),
            ((0.5, 0.5), (1.5, 0.5)),
        ] {
            let steps = drag(a, b);
            assert_eq!(steps.len(), DRAG_STEPS + 3);
            assert_eq!(steps[0], (Pointer::Move, a));
            assert_eq!(
                steps[1],
                (
                    Pointer::Down {
                        right: false,
                        clicks: 1
                    },
                    a
                )
            );
            assert_eq!(
                steps[DRAG_STEPS + 2],
                (
                    Pointer::Up {
                        right: false,
                        clicks: 1
                    },
                    b
                )
            );
            assert_eq!(steps[DRAG_STEPS + 1].1, b);
            let buttons: Vec<usize> = steps.iter().map(|s| s.0.buttons()).collect();
            assert_eq!(buttons[0], 0);
            assert!(buttons[1..=DRAG_STEPS + 1].iter().all(|&b| b == 1));
            assert_eq!(buttons[DRAG_STEPS + 2], 0);
            // Each move goes the same way along the line, as far as the last.
            let (dx, dy) = (
                (b.0 - a.0) / DRAG_STEPS as f64,
                (b.1 - a.1) / DRAG_STEPS as f64,
            );
            for w in steps[1..=DRAG_STEPS + 1].windows(2) {
                assert!(((w[1].1 .0 - w[0].1 .0) - dx).abs() < 1e-9);
                assert!(((w[1].1 .1 - w[0].1 .1) - dy).abs() < 1e-9);
            }
        }
        // A right click holds the right button; a triple click counts to three.
        assert_eq!(clicks(3, false).len(), 7);
        assert_eq!(clicks(0, false).len(), 3, "no clicks is still one");
        assert_eq!(
            Pointer::Up {
                right: true,
                clicks: 1
            }
            .buttons(),
            0
        );
    }

    #[test]
    fn wheel_turns_add_up_to_the_distance_signed_by_the_way() {
        for px in [1, 119, 120, 121, 240, 1000, 30_721] {
            for up in [false, true] {
                let turns = wheel(px, up);
                assert_eq!(turns.len() as u32, px.div_ceil(WHEEL_STEP), "{px}");
                let mut sum = 0.0;
                for t in turns {
                    let Pointer::Wheel { dy } = t else {
                        panic!("{t:?}")
                    };
                    assert!(dy != 0.0 && dy.abs() <= f64::from(WHEEL_STEP));
                    assert_eq!(dy < 0.0, up, "{px} up={up}");
                    sum += dy;
                }
                assert_eq!(sum.abs(), f64::from(px));
            }
        }
    }

    /// `scroll down <px>` takes any u32: at 120 px a turn, each turn a native event a frame
    /// apart, planned all at once. 100,000,000 px is 833,334 events (hours of wheel turns the
    /// command can't stop, a 13 MB plan), u32::MAX is 35,791,395 (a 570 MB plan).
    #[test]
    fn a_huge_scroll_is_refused_or_bounded_before_it_becomes_millions_of_wheel_events() {
        use crate::browser::control::{parse, Command};
        let px = match parse("scroll", &["down".into(), "100000000".into()]) {
            Err(_) => return,
            Ok(Command::Scroll(_, Some(px))) => px,
            Ok(other) => panic!("{other:?}"),
        };
        let turns = wheel(px, false).len();
        assert!(turns <= 10_000, "{turns} wheel events for one scroll");
    }

    #[test]
    fn a_closed_pane_lets_go_of_every_held_key_and_modifier() {
        let mut held = Held::default();
        held.down("Shift").unwrap();
        held.down("Meta").unwrap();
        held.down("w").unwrap();
        let steps = held.release_all();
        assert!(
            matches!(&steps[0], KeyStep::Up(s) if s.code == 0x0D && s.flags == SHIFT | COMMAND)
        );
        assert_eq!(
            steps[1..],
            [
                KeyStep::Flags {
                    code: 0x38,
                    flags: COMMAND
                },
                KeyStep::Flags {
                    code: 0x37,
                    flags: 0
                },
            ]
        );
        assert_eq!(held.flags(), 0);
        assert!(held.release_all().is_empty() && held.down("w").is_ok());
    }

    #[test]
    fn line_breaks_of_any_kind_type_as_one_each() {
        assert_eq!(as_typed("a\n\nb"), "a\n\nb");
        assert_eq!(as_typed("a\r\nb\rc"), "a\nb\nc");
        assert_eq!(as_typed("\r\n\r\n"), "\n\n");
    }

    #[test]
    fn a_negative_or_broken_point_still_looks_like_one() {
        for yes in ["412,300", "-5,10", "5,-3", "1..2,3", " 1 , 2 "] {
            assert!(looks_like_point(yes), "{yes}");
        }
        for no in ["e3", "a, b", "#x,.y", "h1,h2", "-,-", "1,", ""] {
            assert!(!looks_like_point(no), "{no}");
        }
    }

    #[test]
    fn only_space_and_the_up_and_down_arrows_open_a_selects_menu() {
        for yes in [
            "Space",
            " ",
            "ArrowDown",
            "Up",
            "Alt+ArrowDown",
            "Shift+Space",
        ] {
            assert!(opens_menu(yes), "{yes}");
        }
        for no in [
            "Tab",
            "Shift+Tab",
            "Escape",
            "Enter",
            "Shift",
            "ArrowLeft",
            "a",
            "ş",
        ] {
            assert!(!opens_menu(no), "{no}");
        }
    }
}
