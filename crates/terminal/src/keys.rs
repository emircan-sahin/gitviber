//! A key as the program in the pane gets it: xterm's encoding, or the kitty keyboard protocol's
//! once a program turns it on (Claude Code, Codex, neovim, fish 4), where Shift+Enter is its own
//! key. Plain text is left to the window's text input, so IMEs and dead keys compose there.

#[derive(Clone, Copy, Debug, Default)]
pub struct Key<'a> {
    /// GPUI's name for it: "a", "enter", "left", "f5"; a letter is lowercase, shift apart.
    pub key: &'a str,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// ⌘ on macOS, Super elsewhere.
    pub cmd: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Modes {
    /// DECCKM: arrows as SS3.
    pub app_cursor: bool,
    /// Kitty flag 1: modified keys and Esc as CSI u.
    pub kitty: bool,
    /// Kitty flag 8: every key as CSI u, text included.
    pub kitty_all: bool,
    /// ⌥ sends ESC and the key, rather than the character it types.
    pub alt_is_meta: bool,
    pub mac: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// These bytes to the program.
    Send(String),
    /// Text, typed through the window's text input (IME, dead keys).
    Text,
    /// Not the pane's: the app's shortcut, or nothing.
    Pass,
}

/// macOS line editing: zsh and bash bind none of xterm's ⌥← / ⌥→ / ⌥⌦ sequences, the readline
/// keys below work in both. ⌥⌫ is already ESC DEL (delete word).
fn line_edit(k: &Key) -> Option<&'static str> {
    if k.shift || k.ctrl || k.cmd == k.alt {
        return None;
    }
    Some(match (k.cmd, k.key) {
        (true, "left") => "\x01",
        (true, "right") => "\x05",
        (true, "backspace") => "\x15",
        (false, "left") => "\x1bb",
        (false, "right") => "\x1bf",
        (false, "delete") => "\x1bd",
        _ => return None,
    })
}

/// xterm's and kitty's modifier bits: shift 1, alt 2, ctrl 4.
fn bits(k: &Key) -> u8 {
    k.shift as u8 | (k.alt as u8) << 1 | (k.ctrl as u8) << 2
}

fn csi_u(code: u32, bits: u8) -> String {
    if bits == 0 {
        format!("\x1b[{code}u")
    } else {
        format!("\x1b[{code};{}u", bits + 1)
    }
}

/// The control character a key makes with ⌃, as xterm's.
fn ctrl_code(c: char) -> Option<char> {
    let byte = match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 27,
        '\\' | '4' => 28,
        ']' | '5' => 29,
        '^' | '6' | '~' => 30,
        '_' | '7' | '-' | '/' => 31,
        '8' | '?' => 127,
        _ => return None,
    };
    Some(byte as char)
}

pub fn encode(k: &Key, m: &Modes) -> Action {
    if m.mac
        && let Some(seq) = line_edit(k)
    {
        return Action::Send(seq.into());
    }
    if k.cmd {
        return Action::Pass;
    }
    let bits = bits(k);
    let kitty = m.kitty || m.kitty_all;
    let esc = if k.alt { "\x1b" } else { "" };
    let send = |s: String| Action::Send(s);
    // A cursor key: CSI 1;mods X once modified, else SS3 in application mode.
    let cursor = |c: char| {
        if bits != 0 {
            format!("\x1b[1;{}{c}", bits + 1)
        } else if m.app_cursor {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[{c}")
        }
    };
    let tilde = |n: u8| {
        if bits != 0 {
            format!("\x1b[{n};{}~", bits + 1)
        } else {
            format!("\x1b[{n}~")
        }
    };
    let function = |c: char| {
        if bits != 0 {
            format!("\x1b[1;{}{c}", bits + 1)
        } else {
            format!("\x1bO{c}")
        }
    };
    match k.key {
        "enter" if kitty && (bits != 0 || m.kitty_all) => send(csi_u(13, bits)),
        "enter" => send(format!("{esc}\r")),
        "tab" if kitty && (bits != 0 || m.kitty_all) => send(csi_u(9, bits)),
        "tab" if k.shift => send("\x1b[Z".into()),
        "tab" => send(format!("{esc}\t")),
        "backspace" if kitty && (bits != 0 || m.kitty_all) => send(csi_u(127, bits)),
        "backspace" if k.ctrl => send(format!("{esc}\x08")),
        "backspace" => send(format!("{esc}\x7f")),
        "escape" if kitty => send(csi_u(27, bits)),
        "escape" => send(format!("{esc}\x1b")),
        "up" => send(cursor('A')),
        "down" => send(cursor('B')),
        "right" => send(cursor('C')),
        "left" => send(cursor('D')),
        "home" => send(cursor('H')),
        "end" => send(cursor('F')),
        "insert" => send(tilde(2)),
        "delete" => send(tilde(3)),
        "pageup" => send(tilde(5)),
        "pagedown" => send(tilde(6)),
        "f1" => send(function('P')),
        "f2" => send(function('Q')),
        "f3" => send(function('R')),
        "f4" => send(function('S')),
        f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => {
            let n = match f[1..].parse::<u8>().unwrap_or(0) {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                13 => 25,
                14 => 26,
                15 => 28,
                16 => 29,
                17 => 31,
                18 => 32,
                19 => 33,
                20 => 34,
                _ => return Action::Pass,
            };
            send(tilde(n))
        }
        key => {
            let c = match key {
                "space" => ' ',
                key => {
                    let mut chars = key.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) => c,
                        _ => return Action::Pass,
                    }
                }
            };
            let shifted = |c: char| {
                if k.shift {
                    c.to_uppercase().next().unwrap_or(c)
                } else {
                    c
                }
            };
            if k.ctrl {
                if kitty {
                    return send(csi_u(c.to_lowercase().next().unwrap_or(c) as u32, bits));
                }
                return send(match ctrl_code(c) {
                    Some(code) => format!("{esc}{code}"),
                    None => format!("{esc}{}", shifted(c)),
                });
            }
            if k.alt && m.alt_is_meta {
                if kitty {
                    return send(csi_u(c.to_lowercase().next().unwrap_or(c) as u32, bits));
                }
                return send(format!("\x1b{}", shifted(c)));
            }
            if m.kitty_all && !k.alt {
                return send(csi_u(c.to_lowercase().next().unwrap_or(c) as u32, bits));
            }
            Action::Text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &str) -> Key<'_> {
        Key {
            key,
            ..Key::default()
        }
    }
    fn sent(a: Action) -> String {
        match a {
            Action::Send(s) => s,
            other => panic!("{other:?}"),
        }
    }
    const MAC: Modes = Modes {
        app_cursor: false,
        kitty: false,
        kitty_all: false,
        alt_is_meta: false,
        mac: true,
    };

    #[test]
    fn macos_line_editing_keys_are_readlines() {
        let cmd = |k| Key {
            cmd: true,
            ..key(k)
        };
        let alt = |k| Key {
            alt: true,
            ..key(k)
        };
        assert_eq!(sent(encode(&cmd("left"), &MAC)), "\x01");
        assert_eq!(sent(encode(&cmd("right"), &MAC)), "\x05");
        assert_eq!(sent(encode(&cmd("backspace"), &MAC)), "\x15");
        assert_eq!(sent(encode(&alt("left"), &MAC)), "\x1bb");
        assert_eq!(sent(encode(&alt("right"), &MAC)), "\x1bf");
        assert_eq!(sent(encode(&alt("delete"), &MAC)), "\x1bd");
        assert_eq!(sent(encode(&alt("backspace"), &MAC)), "\x1b\x7f");
        // Not off macOS, and not with Shift (selecting in a program that reads it).
        let linux = Modes { mac: false, ..MAC };
        assert_eq!(sent(encode(&alt("left"), &linux)), "\x1b[1;3D");
        assert_eq!(
            sent(encode(
                &Key {
                    shift: true,
                    ..alt("left")
                },
                &MAC
            )),
            "\x1b[1;4D"
        );
        // Other ⌘ keys are the app's.
        assert_eq!(encode(&cmd("t"), &MAC), Action::Pass);
        assert_eq!(encode(&cmd("up"), &MAC), Action::Pass);
    }

    #[test]
    fn shift_enter_is_its_own_key_once_a_program_turns_kitty_on() {
        let shift_enter = Key {
            shift: true,
            ..key("enter")
        };
        assert_eq!(sent(encode(&shift_enter, &MAC)), "\r");
        let kitty = Modes { kitty: true, ..MAC };
        assert_eq!(sent(encode(&shift_enter, &kitty)), "\x1b[13;2u");
        assert_eq!(
            sent(encode(&key("enter"), &kitty)),
            "\r",
            "unmodified stays legacy"
        );
        assert_eq!(sent(encode(&key("escape"), &kitty)), "\x1b[27u");
        assert_eq!(
            sent(encode(
                &Key {
                    ctrl: true,
                    ..key("c")
                },
                &kitty
            )),
            "\x1b[99;5u"
        );
        assert_eq!(
            sent(encode(
                &Key {
                    shift: true,
                    ..key("tab")
                },
                &kitty
            )),
            "\x1b[9;2u"
        );
        assert_eq!(encode(&key("a"), &kitty), Action::Text);
        let all = Modes {
            kitty_all: true,
            ..MAC
        };
        assert_eq!(sent(encode(&key("a"), &all)), "\x1b[97u");
        assert_eq!(sent(encode(&key("enter"), &all)), "\x1b[13u");
    }

    #[test]
    fn xterm_keys() {
        assert_eq!(sent(encode(&key("enter"), &MAC)), "\r");
        assert_eq!(sent(encode(&key("tab"), &MAC)), "\t");
        assert_eq!(
            sent(encode(
                &Key {
                    shift: true,
                    ..key("tab")
                },
                &MAC
            )),
            "\x1b[Z"
        );
        assert_eq!(sent(encode(&key("backspace"), &MAC)), "\x7f");
        assert_eq!(
            sent(encode(
                &Key {
                    ctrl: true,
                    ..key("backspace")
                },
                &MAC
            )),
            "\x08"
        );
        assert_eq!(sent(encode(&key("escape"), &MAC)), "\x1b");
        assert_eq!(sent(encode(&key("up"), &MAC)), "\x1b[A");
        assert_eq!(
            sent(encode(
                &key("up"),
                &Modes {
                    app_cursor: true,
                    ..MAC
                }
            )),
            "\x1bOA"
        );
        assert_eq!(
            sent(encode(
                &Key {
                    ctrl: true,
                    ..key("right")
                },
                &MAC
            )),
            "\x1b[1;5C"
        );
        assert_eq!(sent(encode(&key("home"), &MAC)), "\x1b[H");
        assert_eq!(sent(encode(&key("delete"), &MAC)), "\x1b[3~");
        assert_eq!(
            sent(encode(
                &Key {
                    shift: true,
                    ..key("pageup")
                },
                &MAC
            )),
            "\x1b[5;2~"
        );
        assert_eq!(sent(encode(&key("f1"), &MAC)), "\x1bOP");
        assert_eq!(sent(encode(&key("f5"), &MAC)), "\x1b[15~");
        assert_eq!(
            sent(encode(
                &Key {
                    shift: true,
                    ..key("f12")
                },
                &MAC
            )),
            "\x1b[24;2~"
        );
        assert_eq!(
            sent(encode(
                &Key {
                    ctrl: true,
                    ..key("c")
                },
                &MAC
            )),
            "\x03"
        );
        assert_eq!(
            sent(encode(
                &Key {
                    ctrl: true,
                    ..key("space")
                },
                &MAC
            )),
            "\0"
        );
        assert_eq!(
            sent(encode(
                &Key {
                    ctrl: true,
                    ..key("[")
                },
                &MAC
            )),
            "\x1b"
        );
        assert_eq!(
            sent(encode(
                &Key {
                    ctrl: true,
                    alt: true,
                    ..key("x")
                },
                &MAC
            )),
            "\x1b\x18"
        );
        // Text goes through the text input, for IMEs and dead keys; ⌥ types a character.
        assert_eq!(encode(&key("a"), &MAC), Action::Text);
        assert_eq!(encode(&key("ş"), &MAC), Action::Text);
        assert_eq!(
            encode(
                &Key {
                    shift: true,
                    ..key("a")
                },
                &MAC
            ),
            Action::Text
        );
        assert_eq!(
            encode(
                &Key {
                    alt: true,
                    ..key("s")
                },
                &MAC
            ),
            Action::Text
        );
        let meta = Modes {
            alt_is_meta: true,
            ..MAC
        };
        assert_eq!(
            sent(encode(
                &Key {
                    alt: true,
                    ..key("s")
                },
                &meta
            )),
            "\x1bs"
        );
        assert_eq!(
            sent(encode(
                &Key {
                    alt: true,
                    shift: true,
                    ..key("s")
                },
                &meta
            )),
            "\x1bS"
        );
        assert_eq!(
            sent(encode(
                &Key {
                    alt: true,
                    ..key("enter")
                },
                &MAC
            )),
            "\x1b\r"
        );
    }
}
