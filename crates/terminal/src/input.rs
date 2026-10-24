//! What goes into a pane besides typed keys: wheel rows, pastes, dropped paths, a program's OSC 52
//! copy, and the size a pane is given. Pure, so it's tested here.

/// Rows a wheel event scrolls, from the pixels still short of a row (`pending`) and its own: a
/// row's height of pixels is a row, the rest waits for the next event. Past `max_rows` the rows
/// are dropped, not owed to the next event. Returns (rows, pending).
pub fn wheel_rows(pending: f32, pixels: f32, row_height: f32, max_rows: i32) -> (i32, f32) {
    let total = pending + pixels;
    // A pane not laid out yet has no row height: counting by it would give infinite rows.
    if row_height.is_nan() || row_height <= 0. || !total.is_finite() {
        return (0, if pending.is_finite() { pending } else { 0. });
    }
    let whole = (total / row_height).trunc();
    let rows = whole.clamp(-(max_rows as f32), max_rows as f32) as i32;
    (rows, total - whole * row_height)
}

/// Escaped, a path is one word to a shell, and an AI CLI still finds the file. None when it holds
/// a control character: a name with a newline or ESC in it could run a command or end the
/// bracketed paste early.
pub fn shell_path(path: &str) -> Option<String> {
    if path
        .chars()
        .any(|c| matches!(c, '\x00'..='\x08' | '\x0a'..='\x1f' | '\x7f'))
    {
        return None;
    }
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if "\\ ()[]{}<>\"'`!#$&;|*?\t".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    Some(out)
}

/// The pastes for `paths`, one each: Claude Code attaches only the last path of a paste that holds
/// several. From the second on they start with a space, to stay separate words.
pub fn path_pastes(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|p| shell_path(p))
        .enumerate()
        .map(|(i, p)| if i > 0 { format!(" {p}") } else { p })
        .collect()
}

/// The lines a paste would run one by one where the program hasn't turned on bracketed paste; a
/// trailing line break doesn't start another.
pub fn pasted_lines(text: &str) -> usize {
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .or_else(|| text.strip_suffix('\r'))
        .unwrap_or(text);
    text.replace("\r\n", "\n").split(['\n', '\r']).count()
}

/// A paste as the program gets it: line breaks as Enter, and inside the brackets when it asked for
/// them, with nothing in the text able to close them early.
pub fn paste_bytes(text: &str, bracketed: bool) -> String {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if !bracketed {
        return text;
    }
    let inner = text.replace("\x1b[201~", "").replace("\x1b[200~", "");
    format!("\x1b[200~{inner}\x1b[201~")
}

/// About 1 MB decoded: past that a `cat` of a hostile file would put megabytes on the clipboard.
pub const MAX_OSC52_TEXT: usize = 1_050_000;

/// Whether a program's OSC 52 copy is taken. alacritty decodes it (and never answers a read);
/// a copy this big or empty isn't one a program meant.
pub fn osc52_accepts(text: &str) -> bool {
    !text.is_empty() && text.len() <= MAX_OSC52_TEXT
}

/// History lines past which a column change waits for the resize to settle.
pub const REWRAP_LINES: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
}

/// The size to give a pane now, None to leave it: an empty box is a pane hidden or not laid out,
/// whose one row would make the program redraw for nothing. Past REWRAP_LINES of `history`, new
/// columns wait (true) for a drag to settle, since a reflow rewraps all of it; rows follow at once.
pub fn plan_fit(
    width: f32,
    height: f32,
    proposed: Option<GridSize>,
    current: GridSize,
    history: usize,
) -> Option<(GridSize, bool)> {
    let proposed = proposed?;
    let empty = |v: f32| v.is_nan() || v <= 0.;
    if empty(width) || empty(height) || proposed.cols == 0 || proposed.rows == 0 {
        return None;
    }
    if history > REWRAP_LINES && proposed.cols != current.cols {
        return Some((
            GridSize {
                cols: current.cols,
                rows: proposed.rows,
            },
            true,
        ));
    }
    Some((proposed, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swipe_reports_a_row_for_each_rows_height_of_pixels_and_keeps_the_rest() {
        let m = i32::MAX;
        assert_eq!(wheel_rows(0., 50., 20., m), (2, 10.));
        assert_eq!(wheel_rows(10., 12., 20., m), (1, 2.));
        assert_eq!(wheel_rows(0., 7., 20., m), (0, 7.));
        assert_eq!(wheel_rows(0., -45., 20., m), (-2, -5.));
        // Turning back takes up what the other way left first.
        assert_eq!(wheel_rows(15., -20., 20., m), (0, -5.));
        let (mut pending, mut rows) = (0., 0);
        for _ in 0..100 {
            let (r, p) = wheel_rows(pending, 3., 17., m);
            rows += r;
            pending = p;
        }
        assert_eq!(rows, 300 / 17);
    }

    #[test]
    fn fractional_pixels_over_many_events_lose_no_rows() {
        let (mut pending, mut rows) = (0f32, 0);
        for _ in 0..1000 {
            let (r, p) = wheel_rows(pending, -0.7, 17.3, i32::MAX);
            rows += r;
            pending = p;
        }
        assert_eq!(rows, -(700. / 17.3f32).floor() as i32);
        assert!(pending.abs() < 17.3);
    }

    #[test]
    fn a_pane_with_no_row_height_reports_no_rows_and_pending_stays_a_number() {
        assert_eq!(wheel_rows(5., 40., 0., 40), (0, 5.));
        assert_eq!(wheel_rows(5., 40., f32::NAN, 40), (0, 5.));
        assert_eq!(wheel_rows(5., 40., -20., 40), (0, 5.));
        assert_eq!(wheel_rows(f32::NAN, 40., 20., 40), (0, 0.));
    }

    #[test]
    fn one_event_reports_a_screenful_at_most_and_drops_the_rows_past_it() {
        assert_eq!(wheel_rows(0., 100_000., 20., 40), (40, 0.));
        assert_eq!(wheel_rows(0., -100_010., 20., 40), (-40, -10.));
        assert_eq!(wheel_rows(0., 50., 20., 40), (2, 10.));
    }

    #[test]
    fn paths_are_escaped_so_a_shell_reads_one_word() {
        let s = |p: &str| shell_path(p).unwrap();
        assert_eq!(
            s("/Users/me/Ekran Resmi (1).png"),
            "/Users/me/Ekran\\ Resmi\\ \\(1\\).png"
        );
        assert_eq!(
            s("/tmp/a'b\"c$d&e;f|g*h?i!j#k`l"),
            "/tmp/a\\'b\\\"c\\$d\\&e\\;f\\|g\\*h\\?i\\!j\\#k\\`l"
        );
        assert_eq!(
            s("/tmp/[x]{y}<z>\\w\tv"),
            "/tmp/\\[x\\]\\{y\\}\\<z\\>\\\\w\\\tv"
        );
        assert_eq!(s("/tmp/plain-name_1.txt"), "/tmp/plain-name_1.txt");
        assert_eq!(
            s("/Users/me/Ekran Görüntüsü 😀/şçğ.png"),
            "/Users/me/Ekran\\ Görüntüsü\\ 😀/şçğ.png"
        );
    }

    #[test]
    fn a_name_with_a_control_character_is_left_out() {
        assert_eq!(shell_path("/tmp/evil\nrm -rf ~"), None);
        assert_eq!(shell_path("/tmp/esc\x1b[201~"), None);
        let paths = ["/a b", "/tmp/x\ry", "/c"].map(String::from);
        assert_eq!(path_pastes(&paths), ["/a\\ b", " /c"]);
    }

    #[test]
    fn each_path_is_its_own_paste_space_led_after_the_first() {
        let paths = ["/a.png", "/b.png", "/c.png"].map(String::from);
        assert_eq!(path_pastes(&paths), ["/a.png", " /b.png", " /c.png"]);
        assert!(path_pastes(&[]).is_empty());
    }

    #[test]
    fn a_pastes_lines_a_trailing_line_break_not_counted() {
        assert_eq!(pasted_lines("ls"), 1);
        assert_eq!(pasted_lines("ls\n"), 1);
        assert_eq!(pasted_lines("ls\r\n"), 1);
        assert_eq!(pasted_lines("ls\npwd"), 2);
        assert_eq!(pasted_lines("a\r\nb\rc\n\n"), 4);
    }

    #[test]
    fn a_bracketed_paste_cant_be_closed_from_inside() {
        assert_eq!(paste_bytes("a\nb", false), "a\rb");
        assert_eq!(paste_bytes("a\r\nb", true), "\x1b[200~a\rb\x1b[201~");
        assert_eq!(
            paste_bytes("x\x1b[201~rm -rf ~\n", true),
            "\x1b[200~xrm -rf ~\r\x1b[201~"
        );
    }

    #[test]
    fn osc_52_copies_text_up_to_a_megabyte() {
        assert!(osc52_accepts("héllo ✅\n"));
        assert!(!osc52_accepts(""));
        assert!(!osc52_accepts(&"A".repeat(MAX_OSC52_TEXT + 1)));
    }

    const AT: GridSize = GridSize {
        cols: 100,
        rows: 30,
    };
    fn g(cols: u16, rows: u16) -> Option<GridSize> {
        Some(GridSize { cols, rows })
    }

    #[test]
    fn a_pane_with_no_room_keeps_its_size_rather_than_shrinking_to_one_row() {
        assert_eq!(plan_fit(800., 0., g(100, 1), AT, 0), None);
        assert_eq!(plan_fit(0., 0., g(2, 1), AT, 0), None, "detached");
        assert_eq!(
            plan_fit(800., 400., None, AT, 0),
            None,
            "no cell size measured yet"
        );
        assert_eq!(
            plan_fit(0., 267., g(2, 15), AT, 10),
            None,
            "squeezed to no width"
        );
        assert_eq!(
            plan_fit(563., 14., g(68, 1), AT, 10),
            Some((GridSize { cols: 68, rows: 1 }, false))
        );
    }

    #[test]
    fn rows_follow_at_once_columns_wait_past_a_long_history() {
        assert_eq!(
            plan_fit(800., 400., g(80, 20), AT, 10),
            Some((GridSize { cols: 80, rows: 20 }, false))
        );
        assert_eq!(
            plan_fit(800., 400., g(80, 20), AT, REWRAP_LINES + 1),
            Some((
                GridSize {
                    cols: 100,
                    rows: 20
                },
                true
            ))
        );
        assert_eq!(
            plan_fit(800., 400., g(100, 20), AT, REWRAP_LINES + 1),
            Some((
                GridSize {
                    cols: 100,
                    rows: 20
                },
                false
            ))
        );
        assert_eq!(
            plan_fit(800., 400., g(80, 30), AT, REWRAP_LINES).map(|p| p.1),
            Some(false),
            "at the threshold"
        );
        assert_eq!(
            plan_fit(800., 400., g(80, 30), AT, 10_000),
            Some((AT, true))
        );
        // The held columns come due: no history to hold for.
        assert_eq!(
            plan_fit(
                800.,
                400.,
                g(89, 19),
                GridSize {
                    cols: 100,
                    rows: 19
                },
                0
            ),
            Some((GridSize { cols: 89, rows: 19 }, false))
        );
    }
}
