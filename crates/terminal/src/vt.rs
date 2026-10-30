//! The parser in front of alacritty's `Term`, keeping what a program can pile up bounded.
//!
//! alacritty 0.26 caps its kitty keyboard stack by removing from the title stack: the 4097th
//! `CSI > flags u` with no title pushed panicked (any program, or `yes $'\e[>1u'`). Its title stack
//! keeps 4096 whole titles, each as long as an OSC, and a cell keeps every zero-width mark sent to
//! it, which the renderer shapes each frame. vte doesn't parse XTPUSHSGR, so there's no SGR stack.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Attr, CharsetIndex, ClearMode, CursorShape, CursorStyle, Handler, Hyperlink, KeyboardModes,
    KeyboardModesApplyBehavior, LineClearMode, Mode, ModifyOtherKeys, PrivateMode, Processor, Rgb,
    ScpCharPath, ScpUpdateMode, StandardCharset, StdSyncHandler, TabulationClearMode,
    cursor_icon::CursorIcon,
};
use unicode_width::UnicodeWidthChar as _;

/// Pushes of keyboard modes or titles kept: deeper than programs nest.
const MAX_DEPTH: usize = 64;
/// Longer than a tab or a window shows.
const MAX_TITLE: usize = 1024;
/// Zero-width marks on one cell: UAX #15's stream-safe limit of non-starters in a row.
const MAX_ZEROWIDTH: usize = 30;

/// vte's processor, handing alacritty's `Term` each call through `Bounded`.
#[derive(Default)]
pub struct Parser {
    processor: Processor<StdSyncHandler>,
    tracked: Tracked,
    /// Where replies alacritty can't give go: written in order with its own.
    reply: Option<Box<dyn Fn(String) + Send>>,
}

/// What alacritty keeps out of reach, followed call by call: its stack depths (the keyboard
/// modes' per screen), and mode 2031, which it doesn't keep at all: the program asked to hear
/// of color scheme changes.
#[derive(Default)]
struct Tracked {
    keys: [usize; 2],
    titles: usize,
    scheme_notify: bool,
}

const SCHEME_NOTIFY: PrivateMode = PrivateMode::Unknown(2031);

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// A parser that also answers what alacritty doesn't, through `reply`.
    pub fn answering(reply: impl Fn(String) + Send + 'static) -> Self {
        Parser {
            reply: Some(Box::new(reply)),
            ..Self::default()
        }
    }

    pub fn advance<T: EventListener>(&mut self, term: &mut Term<T>, bytes: &[u8]) {
        let mut bounded = Bounded {
            term,
            tracked: &mut self.tracked,
            reply: self.reply.as_deref(),
        };
        self.processor.advance(&mut bounded, bytes);
    }

    /// Shows a synchronized frame that never ended.
    pub fn stop_sync<T: EventListener>(&mut self, term: &mut Term<T>) {
        let mut bounded = Bounded {
            term,
            tracked: &mut self.tracked,
            reply: self.reply.as_deref(),
        };
        self.processor.stop_sync(&mut bounded);
    }

    /// The program set mode 2031: tell it when the background turns dark or light.
    pub fn scheme_notify(&self) -> bool {
        self.tracked.scheme_notify
    }

    pub fn sync_timeout(&self) -> &StdSyncHandler {
        self.processor.sync_timeout()
    }

    /// A fresh processor in place of one whose buffer grew; the stacks it filled stay counted.
    pub fn renew(&mut self) {
        self.processor = Processor::new();
    }
}

struct Bounded<'a, T> {
    term: &'a mut Term<T>,
    tracked: &'a mut Tracked,
    reply: Option<&'a (dyn Fn(String) + Send)>,
}

impl<T: EventListener> Bounded<'_, T> {
    fn screen(&self) -> usize {
        self.term.mode().contains(TermMode::ALT_SCREEN) as usize
    }

    /// The cell a zero-width char would join (alacritty's `input`) holds all it may.
    fn zerowidth_full(&self) -> bool {
        let grid = self.term.grid();
        let cursor = &grid.cursor;
        let mut point = cursor.point;
        if !cursor.input_needs_wrap {
            point.column.0 = point.column.saturating_sub(1);
        }
        if grid[point].flags.contains(Flags::WIDE_CHAR_SPACER) {
            point.column.0 = point.column.saturating_sub(1);
        }
        grid[point]
            .zerowidth()
            .is_some_and(|z| z.len() >= MAX_ZEROWIDTH)
    }
}

macro_rules! forward {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {
        $(
            #[inline]
            fn $name(&mut self, $($arg: $ty),*) {
                <Term<T> as Handler>::$name(self.term, $($arg),*)
            }
        )*
    };
}

/// Every method goes on to the `Term`: one left out would be the trait's no-op.
impl<T: EventListener> Handler for Bounded<'_, T> {
    fn input(&mut self, c: char) {
        if c.width() == Some(0) && self.zerowidth_full() {
            return;
        }
        Handler::input(self.term, c)
    }

    fn set_title(&mut self, title: Option<String>) {
        Handler::set_title(
            self.term,
            title.map(|mut t| {
                if t.len() > MAX_TITLE {
                    let mut end = MAX_TITLE;
                    while !t.is_char_boundary(end) {
                        end -= 1;
                    }
                    t.truncate(end);
                }
                t
            }),
        )
    }

    fn push_title(&mut self) {
        if self.tracked.titles < MAX_DEPTH {
            self.tracked.titles += 1;
            Handler::push_title(self.term)
        }
    }

    fn pop_title(&mut self) {
        self.tracked.titles = self.tracked.titles.saturating_sub(1);
        Handler::pop_title(self.term)
    }

    /// Past the cap the push takes the top's place: the modes it sets are the ones in force.
    fn push_keyboard_mode(&mut self, mode: KeyboardModes) {
        let depth = &mut self.tracked.keys[self.screen()];
        if *depth >= MAX_DEPTH {
            Handler::pop_keyboard_modes(self.term, 1);
        } else {
            *depth += 1;
        }
        Handler::push_keyboard_mode(self.term, mode)
    }

    fn pop_keyboard_modes(&mut self, to_pop: u16) {
        let depth = &mut self.tracked.keys[self.screen()];
        *depth = depth.saturating_sub(to_pop.into());
        Handler::pop_keyboard_modes(self.term, to_pop)
    }

    fn reset_state(&mut self) {
        *self.tracked = Tracked::default();
        Handler::reset_state(self.term)
    }

    fn set_private_mode(&mut self, mode: PrivateMode) {
        if mode == SCHEME_NOTIFY {
            self.tracked.scheme_notify = true;
        }
        Handler::set_private_mode(self.term, mode)
    }

    fn unset_private_mode(&mut self, mode: PrivateMode) {
        if mode == SCHEME_NOTIFY {
            self.tracked.scheme_notify = false;
        }
        Handler::unset_private_mode(self.term, mode)
    }

    /// DECRQM for 2031 says it's honored (alacritty answers "not recognized"): a program that asks
    /// first turns it on only then.
    fn report_private_mode(&mut self, mode: PrivateMode) {
        match self.reply {
            Some(reply) if mode == SCHEME_NOTIFY => {
                let state = if self.tracked.scheme_notify { 1 } else { 2 };
                reply(format!("\x1b[?2031;{state}$y"))
            }
            _ => Handler::report_private_mode(self.term, mode),
        }
    }

    forward! {
        set_cursor_style(style: Option<CursorStyle>);
        set_cursor_shape(shape: CursorShape);
        goto(line: i32, col: usize);
        goto_line(line: i32);
        goto_col(col: usize);
        insert_blank(count: usize);
        move_up(rows: usize);
        move_down(rows: usize);
        identify_terminal(intermediate: Option<char>);
        device_status(arg: usize);
        move_forward(cols: usize);
        move_backward(cols: usize);
        move_down_and_cr(rows: usize);
        move_up_and_cr(rows: usize);
        put_tab(count: u16);
        backspace();
        carriage_return();
        linefeed();
        bell();
        substitute();
        newline();
        set_horizontal_tabstop();
        scroll_up(rows: usize);
        scroll_down(rows: usize);
        insert_blank_lines(rows: usize);
        delete_lines(rows: usize);
        erase_chars(count: usize);
        delete_chars(count: usize);
        move_backward_tabs(count: u16);
        move_forward_tabs(count: u16);
        save_cursor_position();
        restore_cursor_position();
        clear_line(mode: LineClearMode);
        clear_screen(mode: ClearMode);
        clear_tabs(mode: TabulationClearMode);
        set_tabs(interval: u16);
        reverse_index();
        terminal_attribute(attr: Attr);
        set_mode(mode: Mode);
        unset_mode(mode: Mode);
        report_mode(mode: Mode);
        set_scrolling_region(top: usize, bottom: Option<usize>);
        set_keypad_application_mode();
        unset_keypad_application_mode();
        set_active_charset(index: CharsetIndex);
        configure_charset(index: CharsetIndex, charset: StandardCharset);
        set_color(index: usize, color: Rgb);
        dynamic_color_sequence(prefix: String, index: usize, terminator: &str);
        reset_color(index: usize);
        clipboard_store(clipboard: u8, base64: &[u8]);
        clipboard_load(clipboard: u8, terminator: &str);
        decaln();
        text_area_size_pixels();
        text_area_size_chars();
        set_hyperlink(link: Option<Hyperlink>);
        set_mouse_cursor_icon(icon: CursorIcon);
        report_keyboard_mode();
        set_keyboard_mode(mode: KeyboardModes, behavior: KeyboardModesApplyBehavior);
        set_modify_other_keys(mode: ModifyOtherKeys);
        report_modify_other_keys();
        set_scp(char_path: ScpCharPath, update_mode: ScpUpdateMode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::{Event, VoidListener};
    use alacritty_terminal::index::{Column, Line};
    use alacritty_terminal::term::{Config, test::TermSize};
    use std::sync::{Arc, Mutex};

    fn term<T: EventListener>(listener: T) -> Term<T> {
        let config = Config {
            kitty_keyboard: true,
            ..Config::default()
        };
        Term::new(config, &TermSize::new(20, 5), listener)
    }

    fn keys<T>(t: &Term<T>) -> TermMode {
        *t.mode() & TermMode::KITTY_KEYBOARD_PROTOCOL
    }

    #[test]
    fn keyboard_pushes_past_the_cap_take_the_tops_place_on_each_screen() {
        let mut t = term(VoidListener);
        let mut p = Parser::new();
        p.advance(&mut t, b"\x1b[>1u\x1b[?1049h");
        assert_eq!(keys(&t), TermMode::empty());
        p.advance(&mut t, &b"\x1b[>2u".repeat(10_000));
        p.advance(&mut t, b"\x1b[>4u");
        assert_eq!(keys(&t), TermMode::REPORT_ALTERNATE_KEYS);
        p.advance(&mut t, b"\x1b[<u");
        assert_eq!(keys(&t), TermMode::REPORT_EVENT_TYPES);
        // What's left of the 10,001: the cap, less the one popped.
        p.advance(&mut t, format!("\x1b[<{}u", MAX_DEPTH - 1).as_bytes());
        assert_eq!(keys(&t), TermMode::empty());
        p.advance(&mut t, b"\x1b[?1049l");
        assert_eq!(keys(&t), TermMode::DISAMBIGUATE_ESC_CODES);
        // A reset empties both stacks, and the count starts over with them.
        p.advance(&mut t, &b"\x1b[>2u".repeat(MAX_DEPTH));
        p.advance(&mut t, b"\x1bc");
        p.advance(&mut t, &b"\x1b[>8u".repeat(MAX_DEPTH));
        p.advance(&mut t, format!("\x1b[<{}u", MAX_DEPTH - 1).as_bytes());
        assert_eq!(keys(&t), TermMode::REPORT_ALL_KEYS_AS_ESC);
    }

    #[derive(Clone, Default)]
    struct Titles(Arc<Mutex<Vec<String>>>);
    impl EventListener for Titles {
        fn send_event(&self, event: Event) {
            if let Event::Title(title) = event {
                self.0.lock().unwrap().push(title);
            }
        }
    }

    #[test]
    fn titles_are_cut_and_their_stack_kept_shallow() {
        let titles = Titles::default();
        let mut t = term(titles.clone());
        let mut p = Parser::new();
        p.advance(
            &mut t,
            format!("\x1b]2;{}\x07", "€".repeat(100_000)).as_bytes(),
        );
        let long = titles.0.lock().unwrap().pop().unwrap();
        assert_eq!(long, "€".repeat(MAX_TITLE / 3));
        p.advance(&mut t, &b"\x1b[22t".repeat(10_000));
        p.advance(&mut t, b"\x1b]2;now\x07");
        titles.0.lock().unwrap().clear();
        p.advance(&mut t, &b"\x1b[23t".repeat(MAX_DEPTH + 1));
        let popped = std::mem::take(&mut *titles.0.lock().unwrap());
        assert_eq!(popped.len(), MAX_DEPTH);
        assert!(popped.iter().all(|t| *t == long));
    }

    #[test]
    fn mode_2031_is_kept_until_unset_or_reset() {
        let mut t = term(VoidListener);
        let mut p = Parser::new();
        assert!(!p.scheme_notify());
        p.advance(&mut t, b"\x1b[?2004;2031h");
        assert!(p.scheme_notify());
        assert!(t.mode().contains(TermMode::BRACKETED_PASTE));
        p.advance(&mut t, b"\x1b[?2031l");
        assert!(!p.scheme_notify());
        p.advance(&mut t, b"\x1b[?2031h\x1bc");
        assert!(!p.scheme_notify());
    }

    #[derive(Clone, Default)]
    struct Writes(Arc<Mutex<Vec<String>>>);
    impl EventListener for Writes {
        fn send_event(&self, event: Event) {
            if let Event::PtyWrite(text) = event {
                self.0.lock().unwrap().push(text);
            }
        }
    }

    /// Asked with the DA1 after it, as a program checks for support: answered in order.
    #[test]
    fn decrqm_says_2031_is_honored() {
        let writes = Writes::default();
        let mut t = term(writes.clone());
        let replies = writes.0.clone();
        let mut p = Parser::answering(move |r| replies.lock().unwrap().push(r));
        p.advance(
            &mut t,
            b"\x1b[?2031$p\x1b[c\x1b[?2031h\x1b[?2031$p\x1b[?1049$p",
        );
        let got = std::mem::take(&mut *writes.0.lock().unwrap());
        assert_eq!(got[0], "\x1b[?2031;2$y");
        assert!(got[1].starts_with("\x1b[?"), "{got:?}");
        assert_eq!(got[2..], ["\x1b[?2031;1$y", "\x1b[?1049;2$y"]);
    }

    #[test]
    fn a_cell_keeps_a_bounded_run_of_zero_width_marks() {
        let mut t = term(VoidListener);
        let mut p = Parser::new();
        p.advance(&mut t, format!("a{}b", "\u{301}".repeat(10_000)).as_bytes());
        let row = &t.grid()[Line(0)];
        assert_eq!(
            row[Column(0)].zerowidth().map(<[char]>::len),
            Some(MAX_ZEROWIDTH)
        );
        assert_eq!(row[Column(1)].c, 'b');
    }
}
