//! ⌘F over a diff's lines, removed ones too: ↵ / ⇧↵ step through the matches (a folded one is
//! opened), ⌥⌘C and ⌥⌘W toggle Match Case and Whole Word, Esc closes.

use super::*;
use crate::model::find::{self, FindOptions};
use crate::model::row_pos;
use gpui_kit::Hsla;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};

/// Matches counted at most, as the old find box did.
const MAX_MATCHES: usize = 2000;

pub(super) struct Find {
    input: Entity<InputState>,
    options: FindOptions,
    /// In row order: a row's text is the same in both columns of split, so a row is enough.
    hits: Vec<Hit>,
    current: usize,
    _sub: Subscription,
}

struct Hit {
    row: u32,
    pos: LinePos,
    range: Range<usize>,
}

impl DiffView {
    /// Lines to look through: a text diff, not an image, a preview or the editor.
    pub(super) fn can_find(&self) -> bool {
        self.image.is_none()
            && self.edit.is_none()
            && !self.previewing()
            && self.data.as_ref().is_some_and(|d| d.placeholder.is_none())
    }

    /// ⌘F: the box, focused with its query selected to type over. A diff with no lines to look
    /// through (an image, a preview, the editor) leaves ⌘F to the view around it.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_find() {
            return cx.propagate();
        }
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
            let sub = cx.subscribe_in(&input, window, |this, _, e: &InputEvent, _, cx| match e {
                InputEvent::Change => this.search(true, cx),
                InputEvent::PressEnter { shift, .. } => this.find_step(!*shift, cx),
                _ => {}
            });
            self.find = Some(Find {
                input,
                options: FindOptions::default(),
                hits: vec![],
                current: 0,
                _sub: sub,
            });
        }
        if let Some(f) = &self.find {
            f.input.update(cx, |i, cx| {
                i.focus(window, cx);
                i.select_all(window, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Looks again (the query or a toggle changed, or the diff did); `show` goes to the first.
    pub(super) fn search(&mut self, show: bool, cx: &mut Context<Self>) {
        let Some(f) = &self.find else { return };
        let query = f.input.read(cx).value().to_string();
        let hits = match &self.data {
            Some(data) if !query.is_empty() => {
                let mut hits = vec![];
                for (r, row) in data.rows.iter().enumerate() {
                    let pos = row_pos(row);
                    let line = data.side(pos.side).line(pos.line);
                    for range in find::matches(&line, &query, f.options) {
                        hits.push(Hit {
                            row: r as u32,
                            pos,
                            range,
                        });
                    }
                    if hits.len() >= MAX_MATCHES {
                        hits.truncate(MAX_MATCHES);
                        break;
                    }
                }
                hits
            }
            _ => vec![],
        };
        let Some(f) = &mut self.find else { return };
        f.hits = hits;
        f.current = if show {
            0
        } else {
            f.current.min(f.hits.len().saturating_sub(1))
        };
        if show {
            self.show_hit(cx);
        }
        cx.notify();
    }

    /// The next match (↵), or the one before (⇧↵); round.
    pub fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(f) = &mut self.find else { return };
        if f.hits.is_empty() {
            return;
        }
        let n = f.hits.len();
        f.current = if forward {
            (f.current + 1) % n
        } else {
            (f.current + n - 1) % n
        };
        self.show_hit(cx);
        cx.notify();
    }

    fn show_hit(&mut self, cx: &mut Context<Self>) {
        if let Some(pos) = self
            .find
            .as_ref()
            .and_then(|f| f.hits.get(f.current))
            .map(|h| h.pos)
        {
            self.scroll_to_line(pos, cx);
        }
    }

    fn toggle_find_option(&mut self, whole_word: bool, cx: &mut Context<Self>) {
        let Some(f) = &mut self.find else { return };
        if whole_word {
            f.options.whole_word = !f.options.whole_word;
        } else {
            f.options.match_case = !f.options.match_case;
        }
        self.search(true, cx);
    }

    /// Row `r`'s matches, to draw over its code: the current one stronger.
    pub(super) fn find_marks(&self, r: u32, style: &DiffStyle) -> Vec<(Range<usize>, Hsla)> {
        let Some(f) = &self.find else { return vec![] };
        let from = f.hits.partition_point(|h| h.row < r);
        f.hits[from..]
            .iter()
            .enumerate()
            .take_while(|(_, h)| h.row == r)
            .map(|(i, h)| {
                let strong = from + i == f.current;
                let color = style
                    .colors
                    .primary
                    .opacity(if strong { 0.6 } else { 0.25 });
                (h.range.clone(), color)
            })
            .collect()
    }

    /// Where the box is: the match shown and how many (tests).
    #[doc(hidden)]
    pub fn find_state(&self) -> Option<(usize, usize, Option<LinePos>)> {
        self.find.as_ref().map(|f| {
            let at = f.hits.get(f.current).map(|h| h.pos);
            (f.current, f.hits.len(), at)
        })
    }

    /// Types `query` into the find box (tests).
    #[doc(hidden)]
    pub fn set_find_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = &self.find {
            f.input
                .update(cx, |i, cx| i.set_value(query.to_string(), window, cx));
        }
        self.search(true, cx);
    }

    pub(super) fn find_bar(&self, style: &DiffStyle, cx: &mut Context<Self>) -> Option<AnyElement> {
        let f = self.find.as_ref()?;
        let c = &style.colors;
        let query = !f.input.read(cx).value().is_empty();
        let count: SharedString = match (query, f.hits.len()) {
            (false, _) => "".into(),
            (true, 0) => "No results".into(),
            (true, n) if n >= MAX_MATCHES => format!("{} of {n}+", f.current + 1).into(),
            (true, n) => format!("{} of {n}", f.current + 1).into(),
        };
        let mac = cfg!(target_os = "macos");
        Some(
            // Over the lines: a click here mustn't pick the line under it.
            h_flex()
                .occlude()
                .absolute()
                .top_2()
                .right_4()
                .w(px(360.))
                .gap_1()
                .p_1()
                .rounded(px(6.))
                .bg(c.elevated)
                .border_1()
                .border_color(c.border)
                .text_size(style.ui_font_size)
                .key_context(FIND_CONTEXT)
                .on_action(cx.listener(|this, _: &Escape, window, cx| this.close_find(window, cx)))
                .on_action(
                    cx.listener(|this, _: &FindMatchCase, _, cx| {
                        this.toggle_find_option(false, cx)
                    }),
                )
                .on_action(
                    cx.listener(|this, _: &FindWholeWord, _, cx| this.toggle_find_option(true, cx)),
                )
                .child(div().flex_1().child(Input::new(&f.input).xsmall()))
                .child(
                    icon_button(
                        "find-case",
                        Lucide::CaseSensitive,
                        if mac {
                            "Match case (⌥⌘C)"
                        } else {
                            "Match case (Alt+C)"
                        },
                        f.options.match_case,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_find_option(false, cx))),
                )
                .child(
                    icon_button(
                        "find-word",
                        Lucide::WholeWord,
                        if mac {
                            "Match whole word (⌥⌘W)"
                        } else {
                            "Match whole word (Alt+W)"
                        },
                        f.options.whole_word,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_find_option(true, cx))),
                )
                .child(
                    div()
                        .w(px(64.))
                        .text_right()
                        .text_size(style.ui_font_size - px(1.))
                        .text_color(c.subtle)
                        .child(count),
                )
                .child(
                    icon_button("find-prev", Lucide::ArrowUp, "Previous match (⇧↵)", false)
                        .on_click(cx.listener(|this, _, _, cx| this.find_step(false, cx))),
                )
                .child(
                    icon_button("find-next", Lucide::ArrowDown, "Next match (↵)", false)
                        .on_click(cx.listener(|this, _, _, cx| this.find_step(true, cx))),
                )
                .child(
                    icon_button("find-close", Lucide::Close, "Close (esc)", false)
                        .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
                )
                .into_any_element(),
        )
    }
}
