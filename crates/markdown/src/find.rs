//! Find in a rendered document: the query with the find box's three toggles (Match Case, Whole
//! Word, Regex), matched against the text a [`MarkdownView`](crate::MarkdownView) shows, and
//! [`FindBar`], the box itself for a host to place over its page.

use std::ops::Range;

use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::find;
use gitviber_ui::widgets::Toggle as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, Escape, Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    KeyDownEvent, Keystroke, ParentElement as _, Render, SharedString, Styled as _, Subscription,
    Window, div, px,
};

/// Matches found at most, as the old find box's: past it the count says "+".
pub const MAX_MATCHES: usize = 10_000;

/// What a find box looks for. Empty text finds nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindQuery {
    pub text: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

impl FindQuery {
    /// Flips a toggle on its key: ⌥⌘C / ⌥⌘W / ⌥⌘R on macOS, Alt+C / W / R elsewhere. True when
    /// the key was one (stop its propagation, then find again).
    pub fn flip_on_key(&mut self, key: &Keystroke) -> bool {
        let m = &key.modifiers;
        if !m.alt || m.shift || m.control || m.platform != cfg!(target_os = "macos") {
            return false;
        }
        let flag = match key.key.as_str() {
            "c" => &mut self.match_case,
            "w" => &mut self.whole_word,
            "r" => &mut self.regex,
            _ => return false,
        };
        *flag = !*flag;
        true
    }
}

/// git grep -w's word characters.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where `query` matches in `text`: byte ranges in order, at most `limit`, empty matches left
/// out. Whole word wants no word character on either side, trying later starts when one doesn't
/// fit, as git grep -w does. Err: why a regex doesn't parse.
pub fn find_matches(
    text: &str,
    query: &FindQuery,
    limit: usize,
) -> Result<Vec<Range<usize>>, String> {
    if query.text.is_empty() {
        return Ok(vec![]);
    }
    let pattern = if query.regex {
        query.text.clone()
    } else {
        regex::escape(&query.text)
    };
    let re = regex::RegexBuilder::new(&pattern)
        .case_insensitive(!query.match_case)
        .multi_line(true)
        .build()
        // The parser's message ends with the line that says what's wrong.
        .map_err(|e| {
            let msg = e.to_string();
            let last = msg.lines().rev().find(|l| !l.trim().is_empty());
            last.unwrap_or("Invalid regex")
                .trim()
                .trim_start_matches("error: ")
                .to_string()
        })?;
    let mut out = Vec::new();
    let mut at = 0;
    while out.len() < limit {
        let Some(m) = re.find_at(text, at) else { break };
        let (start, end) = (m.start(), m.end());
        let whole = !query.whole_word
            || !(text[..start].chars().next_back().is_some_and(is_word)
                || text[end..].chars().next().is_some_and(is_word));
        if start < end && whole {
            out.push(start..end);
            at = end;
            continue;
        }
        // Try again one character on.
        match text[start..].chars().next() {
            Some(c) => at = start + c.len_utf8(),
            None => break,
        }
    }
    Ok(out)
}

/// What the box asks its host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindBarEvent {
    /// The text or a toggle changed: find [`FindBar::query`] again, from the first match.
    Changed,
    /// ↵ / ⇧↵, ⌘G / ⇧⌘G or an arrow: the next match (1) or the one before (-1).
    Step(isize),
    /// Esc or ×: drop the box (and the marks) and focus the page.
    Close,
}

/// The find box: query, toggles, "n of m", previous / next, close. The host places it (over the
/// top right of its page), finds on [`FindBarEvent::Changed`] and reports with [`Self::set_found`].
pub struct FindBar {
    input: Entity<InputState>,
    query: FindQuery,
    /// The current match (0-based) and how many; Err: why the query can't run.
    found: Result<(Option<usize>, usize), String>,
    _sub: Subscription,
}

impl EventEmitter<FindBarEvent> for FindBar {}

impl FindBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
        let sub = cx.subscribe(&input, |this: &mut Self, input, e: &InputEvent, cx| {
            let text = input.read(cx).value();
            // Only a real edit: a key the box lets through can report one with the same text.
            if matches!(e, InputEvent::Change) && *text != *this.query.text {
                this.query.text = text.to_string();
                cx.emit(FindBarEvent::Changed);
            }
        });
        FindBar {
            input,
            query: FindQuery::default(),
            found: Ok((None, 0)),
            _sub: sub,
        }
    }

    pub fn query(&self) -> &FindQuery {
        &self.query
    }

    /// Focuses the box with its text selected, to type over (⌘F again).
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| {
            i.focus(window, cx);
            i.select_all(window, cx);
        });
    }

    /// What the host found: the current match (0-based) and how many, or why it couldn't look.
    pub fn set_found(
        &mut self,
        found: Result<(Option<usize>, usize), String>,
        cx: &mut Context<Self>,
    ) {
        if self.found != found {
            self.found = found;
            cx.notify();
        }
    }

    fn flip(&mut self, which: u8, cx: &mut Context<Self>) {
        let flag = match which {
            0 => &mut self.query.match_case,
            1 => &mut self.query.whole_word,
            _ => &mut self.query.regex,
        };
        *flag = !*flag;
        cx.emit(FindBarEvent::Changed);
        cx.notify();
    }
}

impl Render for FindBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let query = !self.query.text.is_empty();
        let (count, bad): (SharedString, bool) = match &self.found {
            _ if !query => ("".into(), false),
            Err(_) => ("Invalid regex".into(), true),
            Ok((_, 0)) => ("No results".into(), true),
            Ok((at, n)) => {
                let at = at.map_or(0, |i| i + 1);
                let more = if *n >= MAX_MATCHES { "+" } else { "" };
                (format!("{at} of {n}{more}").into(), false)
            }
        };
        let steps = matches!(self.found, Ok((_, n)) if n > 0);
        let mac = cfg!(target_os = "macos");
        let key = |k: &str| {
            if mac {
                format!("⌥⌘{k}")
            } else {
                format!("Alt+{k}")
            }
        };
        let toggle = |id: &'static str, icon: IconName, tip: String, on: bool, which: u8| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .tooltip(tip)
                .toggle(on, cx)
                .on_click(cx.listener(move |this, _, _, cx| this.flip(which, cx)))
        };
        let arrow = |id: &'static str, icon: IconName, tip: &'static str, dir: isize| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .tooltip(tip)
                .disabled(!steps)
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(FindBarEvent::Step(dir))))
        };
        div()
            .key_context("MarkdownFind")
            .capture_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if this.query.flip_on_key(&e.keystroke) {
                    cx.stop_propagation();
                    cx.emit(FindBarEvent::Changed);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|_, _: &Escape, _, cx| cx.emit(FindBarEvent::Close)))
            // A one-line Input lets Enter go on, and the key would then be typed into it: taken
            // here, it steps and goes no further.
            .on_action(cx.listener(|_, e: &Enter, _, cx| {
                cx.emit(FindBarEvent::Step(if e.shift { -1 } else { 1 }))
            }))
            .on_action(cx.listener(|_, _: &find::Next, _, cx| cx.emit(FindBarEvent::Step(1))))
            .on_action(cx.listener(|_, _: &find::Prev, _, cx| cx.emit(FindBarEvent::Step(-1))))
            .flex()
            .items_center()
            .gap_0p5()
            .p_1()
            .rounded(px(6.))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.elevated)
            .shadow_md()
            .child(div().w(px(180.)).child(Input::new(&self.input).xsmall()))
            .child(toggle(
                "find-case",
                IconName::CaseSensitive,
                format!("Match Case ({})", key("C")),
                self.query.match_case,
                0,
            ))
            .child(toggle(
                "find-word",
                IconName::WholeWord,
                format!("Match Whole Word ({})", key("W")),
                self.query.whole_word,
                1,
            ))
            .child(toggle(
                "find-regex",
                IconName::Regex,
                format!("Use Regular Expression ({})", key("R")),
                self.query.regex,
                2,
            ))
            .child(
                div()
                    .min_w(px(64.))
                    .text_right()
                    .text_size(px(11.))
                    .text_color(if bad { t.destructive } else { t.subtle })
                    .child(count),
            )
            .child(arrow(
                "find-prev",
                IconName::ArrowUp,
                "Previous Match (⇧↵)",
                -1,
            ))
            .child(arrow("find-next", IconName::ArrowDown, "Next Match (↵)", 1))
            .child(
                Button::new("find-close")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::Close))
                    .tooltip("Close (Esc)")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FindBarEvent::Close))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str, match_case: bool, whole_word: bool, regex: bool) -> FindQuery {
        FindQuery {
            text: text.into(),
            match_case,
            whole_word,
            regex,
        }
    }

    #[test]
    fn plain_text_ignores_case_unless_asked() {
        let text = "Foo foo FOO food";
        assert_eq!(
            find_matches(text, &q("foo", false, false, false), 99)
                .unwrap()
                .len(),
            4
        );
        assert_eq!(
            find_matches(text, &q("foo", true, false, false), 99).unwrap(),
            vec![4..7, 12..15]
        );
        // Regex characters are literal without the toggle.
        assert_eq!(
            find_matches("a.b axb", &q("a.b", false, false, false), 99).unwrap(),
            vec![0..3]
        );
    }

    #[test]
    fn whole_word_tries_later_starts() {
        // "foo" inside "food" is skipped; the regex's later match still counts.
        let found = find_matches("food foo_x foo", &q("foo", false, true, false), 99).unwrap();
        assert_eq!(found, vec![11..14]);
        let found = find_matches("baa a", &q("a+", false, true, true), 99).unwrap();
        assert_eq!(found, vec![4..5]);
    }

    #[test]
    fn regex_errors_and_empty_matches() {
        let err = find_matches("x", &q("(", false, false, true), 99).unwrap_err();
        assert!(err.contains("unclosed"), "{err}");
        assert!(
            find_matches("abc", &q("x*", false, false, true), 99)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            find_matches("ç é ç", &q("ç", false, false, false), 1).unwrap(),
            vec![0..2]
        );
        assert_eq!(
            find_matches("a\nb", &q("^b", false, false, true), 9).unwrap(),
            vec![2..3]
        );
    }

    struct Host(Entity<crate::MarkdownView>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.0.clone())
        }
    }

    #[gpui_kit::test]
    fn a_view_finds_steps_and_follows_its_text(cx: &mut gpui_kit::TestAppContext) {
        use std::cell::RefCell;
        use std::rc::Rc;
        gitviber_ui::test_support::config_dir("markdown-test");
        cx.update(gitviber_ui::init);
        let doc = "# Foo\n\nfoo **bar** foo\n\n- a foo item\n";
        let (host, cx) = cx.add_window_view(|window, cx| {
            Host(cx.new(|cx| crate::MarkdownView::new(doc, Default::default(), window, cx)))
        });
        let view = host.read_with(cx, |h, _| h.0.clone());
        let changed = Rc::new(RefCell::new(vec![]));
        let seen = changed.clone();
        let _sub = cx.update(|_, cx| {
            cx.subscribe(&view, move |_, e: &crate::FindChanged, _| {
                seen.borrow_mut().push(e.matches)
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        let found = view.update(cx, |v, cx| v.find(&q("foo", false, false, false), cx));
        assert_eq!(found, Ok(4));
        assert_eq!(
            view.update(cx, |v, cx| v.find(&q("foo", true, false, false), cx)),
            Ok(3)
        );
        // Markup isn't text: "foo bar" spans the bold.
        assert_eq!(
            view.update(cx, |v, cx| v.find(&q("foo bar", false, false, false), cx)),
            Ok(1)
        );
        view.update(cx, |v, cx| {
            assert_eq!(v.find(&q("foo", false, false, false), cx), Ok(4));
            assert_eq!(v.matches(), (None, 4));
            assert_eq!(v.step_match(-1, cx), Some(3));
            assert_eq!(v.step_match(1, cx), Some(0));
            v.select_match(Some(2), true, cx);
            assert_eq!(v.matches(), (Some(2), 4));
            v.select_match(Some(9), false, cx);
            assert_eq!(v.matches(), (None, 4));
            v.select_match(Some(3), false, cx);
            v.set_source("One foo left.", cx);
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |v, _| v.matches()), (Some(0), 1));
        assert_eq!(changed.borrow().as_slice(), &[1]);
        view.update(cx, |v, cx| {
            assert!(v.find(&q("(", false, false, true), cx).is_err());
            assert_eq!(v.matches(), (None, 0));
            assert_eq!(v.find(&q("left", false, false, false), cx), Ok(1));
            assert_eq!(v.find(&FindQuery::default(), cx), Ok(0));
            assert_eq!(v.matches(), (None, 0));
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// The box under a window root, as the app has it, so ⇧⌘G could reach Focus Git Panel.
    struct BarHost(Entity<FindBar>, std::rc::Rc<std::cell::Cell<bool>>);

    impl Render for BarHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let git = self.1.clone();
            div()
                .key_context(gitviber_ui::keymap::ROOT_CONTEXT)
                .on_action(move |_: &gitviber_ui::keymap::view::FocusGitPanel, _, _| git.set(true))
                .size_full()
                .child(self.0.clone())
        }
    }

    #[gpui_kit::test]
    fn find_next_and_previous_keys_step_from_the_box(cx: &mut gpui_kit::TestAppContext) {
        use std::cell::RefCell;
        use std::rc::Rc;
        gitviber_ui::test_support::config_dir("markdown-test");
        cx.update(gitviber_ui::init);
        let git = Rc::new(std::cell::Cell::new(false));
        let (host, cx) = cx.add_window_view(|window, cx| {
            BarHost(cx.new(|cx| FindBar::new(window, cx)), git.clone())
        });
        let bar = host.read_with(cx, |h, _| h.0.clone());
        let steps = Rc::new(RefCell::new(vec![]));
        let seen = steps.clone();
        let _sub = cx.update(|_, cx| {
            cx.subscribe(&bar, move |_, e: &FindBarEvent, _| {
                if let FindBarEvent::Step(dir) = e {
                    seen.borrow_mut().push(*dir)
                }
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| bar.update(cx, |b, cx| b.focus(window, cx)));
        let (next, prev) = if cfg!(target_os = "macos") {
            ("cmd-g", "shift-cmd-g")
        } else {
            ("f3", "shift-f3")
        };
        cx.simulate_keystrokes(&format!("{next} {prev} {next}"));
        assert_eq!(*steps.borrow(), [1, -1, 1]);
        assert!(!git.get(), "⇧⌘G stays in the find box");
    }

    #[test]
    fn option_keys_flip_toggles() {
        let mut query = FindQuery::default();
        let mac = if cfg!(target_os = "macos") {
            "cmd-"
        } else {
            ""
        };
        assert!(query.flip_on_key(&Keystroke::parse(&format!("alt-{mac}c")).unwrap()));
        assert!(query.flip_on_key(&Keystroke::parse(&format!("alt-{mac}r")).unwrap()));
        assert!(!query.flip_on_key(&Keystroke::parse(&format!("alt-shift-{mac}w")).unwrap()));
        assert!(!query.flip_on_key(&Keystroke::parse("c").unwrap()));
        assert_eq!(
            (query.match_case, query.whole_word, query.regex),
            (true, false, true)
        );
    }
}
