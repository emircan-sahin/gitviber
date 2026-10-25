//! Mouse reports for a program that asked for them (1000/1002/1003), in the encoding it chose:
//! SGR (1006), UTF-8 (1005), or the default, a byte per value.

use alacritty_terminal::term::TermMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    /// Motion with no button held (1003 only).
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Press,
    Release,
    Motion,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// A report, and whether it's binary (a char per byte, which isn't UTF-8 to encode). None where
/// the encoding can't say it: the default one past column or row 223.
pub fn report(
    button: Button,
    kind: Kind,
    col: usize,
    row: usize,
    mods: Mods,
    mode: TermMode,
) -> Option<(String, bool)> {
    let base = match button {
        Button::Left => 0,
        Button::Middle => 1,
        Button::Right => 2,
        Button::None => 3,
        Button::WheelUp => 64,
        Button::WheelDown => 65,
    };
    let mut code = base;
    if kind == Kind::Motion {
        code += 32;
    }
    code += (mods.shift as u32) * 4 + (mods.alt as u32) * 8 + (mods.ctrl as u32) * 16;
    let (x, y) = (col + 1, row + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if kind == Kind::Release { 'm' } else { 'M' };
        return Some((format!("\x1b[<{code};{x};{y}{end}"), false));
    }
    // Past SGR, a release doesn't say which button.
    if kind == Kind::Release {
        code = 3 + (code & !3 & !64);
    }
    let values = [code as usize + 32, x + 32, y + 32];
    if mode.contains(TermMode::UTF8_MOUSE) {
        let mut out = String::from("\x1b[M");
        for v in values {
            out.push(char::from_u32(v.min(2047) as u32)?);
        }
        return Some((out, false));
    }
    if values.iter().any(|&v| v > 255) {
        return None;
    }
    let mut out = String::from("\x1b[M");
    out.extend(values.iter().map(|&v| v as u8 as char));
    Some((out, true))
}

/// Whether the program hears this event in the mode it's in.
pub fn wants(kind: Kind, button: Button, mode: TermMode) -> bool {
    match kind {
        Kind::Press | Kind::Release => mode.intersects(TermMode::MOUSE_MODE),
        Kind::Motion if button == Button::None => mode.contains(TermMode::MOUSE_MOTION),
        Kind::Motion => mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_in_each_encoding() {
        let none = Mods::default();
        let sgr = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            report(Button::Left, Kind::Press, 0, 0, none, sgr)
                .unwrap()
                .0,
            "\x1b[<0;1;1M"
        );
        assert_eq!(
            report(Button::Left, Kind::Release, 4, 2, none, sgr)
                .unwrap()
                .0,
            "\x1b[<0;5;3m"
        );
        assert_eq!(
            report(
                Button::WheelDown,
                Kind::Press,
                9,
                9,
                Mods { ctrl: true, ..none },
                sgr
            )
            .unwrap()
            .0,
            "\x1b[<81;10;10M"
        );
        assert_eq!(
            report(Button::Left, Kind::Motion, 1, 1, none, sgr)
                .unwrap()
                .0,
            "\x1b[<32;2;2M"
        );
        let plain = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(
            report(Button::Left, Kind::Press, 0, 0, none, plain),
            Some(("\x1b[M !!".into(), true))
        );
        assert_eq!(
            report(Button::Right, Kind::Release, 0, 0, none, plain)
                .unwrap()
                .0,
            "\x1b[M#!!"
        );
        // Column 200: a byte past 127, sent as one.
        assert_eq!(
            report(Button::Left, Kind::Press, 199, 0, none, plain)
                .unwrap()
                .0,
            "\x1b[M \u{e8}!"
        );
        assert_eq!(report(Button::Left, Kind::Press, 300, 0, none, plain), None);
        let utf8 = TermMode::MOUSE_REPORT_CLICK | TermMode::UTF8_MOUSE;
        assert_eq!(
            report(Button::Left, Kind::Press, 300, 0, none, utf8),
            Some(("\x1b[M \u{14d}!".into(), false))
        );
    }

    #[test]
    fn what_each_mode_reports() {
        assert!(wants(
            Kind::Press,
            Button::Left,
            TermMode::MOUSE_REPORT_CLICK
        ));
        assert!(!wants(
            Kind::Motion,
            Button::Left,
            TermMode::MOUSE_REPORT_CLICK
        ));
        assert!(wants(Kind::Motion, Button::Left, TermMode::MOUSE_DRAG));
        assert!(!wants(Kind::Motion, Button::None, TermMode::MOUSE_DRAG));
        assert!(wants(Kind::Motion, Button::None, TermMode::MOUSE_MOTION));
        assert!(!wants(Kind::Press, Button::Left, TermMode::empty()));
    }
}
