//! The Search panel: the worktree's text through `git grep` (core's grep.rs), matches grouped by
//! file. Typing searches once it pauses; a newer search stops the running one in git. Picking a
//! match opens the file at its line with the match selected.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gitviber_core::grep::{self, Found, Query};
use gitviber_core::{Core, api};
use gitviber_markdown::FindQuery;
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{FileIcon, Toggle as _};
use gitviber_ui::workspace::{Panel, PanelContext, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle, Focusable as _,
    HighlightStyle, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render,
    ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _, StyledText,
    Subscription, Task, UniformListScrollHandle, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, uniform_list,
};

use crate::backend;
use crate::{OpenOptions, Reveal, open_file};

pub const PANEL_ID: &str = "search";
const DEBOUNCE: Duration = Duration::from_millis(250);
const ROW_HEIGHT: f32 = 24.;

#[derive(Clone, Copy)]
enum Item {
    File(usize),
    Hit(usize, usize),
}

pub struct SearchPanel {
    workspace: WeakEntity<Workspace>,
    core: Arc<Core>,
    input: Entity<InputState>,
    include: Entity<InputState>,
    exclude: Entity<InputState>,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    globs: bool,
    found: Option<Found>,
    /// The query `found` answers, for marking its matches.
    found_for: Option<Query>,
    error: Option<SharedString>,
    pending: bool,
    /// A search sent to git and not answered: stopped there, not just ignored, when another starts.
    running: bool,
    collapsed: HashSet<usize>,
    items: Vec<Item>,
    picked: Option<(usize, usize)>,
    /// The keyboard's row in `items`, while the results have focus.
    cursor: Option<usize>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    list_focus: FocusHandle,
    _search: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl Panel for SearchPanel {
    fn panel_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.input.read(cx).focus_handle(cx))
    }
}

impl SearchPanel {
    pub fn new(ctx: &PanelContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let include = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. *.ts, src/**"));
        let exclude = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. *.test.ts, dist"));
        let mut subs = Vec::new();
        for state in [&input, &include, &exclude] {
            subs.push(
                cx.subscribe_in(state, window, |this, _, e: &InputEvent, _, cx| {
                    if matches!(e, InputEvent::Change) {
                        this.schedule(DEBOUNCE, cx)
                    }
                }),
            );
        }
        SearchPanel {
            workspace: ctx.workspace.clone(),
            core: backend::core(cx),
            input,
            include,
            exclude,
            match_case: false,
            whole_word: false,
            regex: false,
            globs: false,
            found: None,
            found_for: None,
            error: None,
            pending: false,
            running: false,
            collapsed: HashSet::new(),
            items: Vec::new(),
            picked: None,
            cursor: None,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            list_focus: cx.focus_handle(),
            _search: Task::ready(()),
            _subs: subs,
            _live: Live::new("SearchPanel"),
        }
    }

    /// Find in Files: the box focused, with `seed` (the code view's selection) in it.
    pub fn ask(&mut self, seed: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let seed = seed.filter(|s| !s.is_empty() && !s.contains('\n'));
        let seeded = seed.is_some();
        self.input.update(cx, |s, cx| {
            if let Some(seed) = seed {
                s.set_value(seed, window, cx);
            }
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        // A value set from here isn't typing: no change event runs it.
        if seeded {
            self.schedule(Duration::ZERO, cx);
        }
    }

    fn query(&self, cx: &App) -> Query {
        Query {
            text: self.input.read(cx).value().to_string(),
            match_case: self.match_case,
            whole_word: self.whole_word,
            regex: self.regex,
            include: self.include.read(cx).value().to_string(),
            exclude: self.exclude.read(cx).value().to_string(),
        }
    }

    fn stop_running(&mut self) {
        if self.running {
            self.running = false;
            api::files::cancel_search(&self.core);
        }
    }

    /// Runs the search after `delay`; anything newer replaces it.
    fn schedule(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.stop_running();
        let query = self.query(cx);
        if query.text.is_empty() {
            self._search = Task::ready(());
            self.found = None;
            self.error = None;
            self.pending = false;
            self.rebuild();
            cx.notify();
            return;
        }
        self.pending = true;
        cx.notify();
        let core = self.core.clone();
        self._search = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let ran = query.clone();
            if this.update(cx, |this, _| this.running = true).is_err() {
                return;
            }
            let result = cx
                .background_spawn(async move { api::files::search_files(&core, ran) })
                .await;
            _ = this.update(cx, |this, cx| {
                this.running = false;
                match result {
                    Err(e) if e == grep::CANCELLED => return,
                    Ok(found) => {
                        this.found = Some(found);
                        this.found_for = Some(query);
                        this.error = None;
                        this.collapsed.clear();
                    }
                    Err(e) => {
                        this.found = None;
                        this.error = Some(e.into());
                    }
                }
                this.pending = false;
                this.picked = None;
                this.cursor = None;
                this.rebuild();
                cx.notify();
            });
        });
    }

    fn rebuild(&mut self) {
        self.items.clear();
        let Some(found) = &self.found else { return };
        for (f, file) in found.files.iter().enumerate() {
            self.items.push(Item::File(f));
            if !self.collapsed.contains(&f) {
                self.items
                    .extend((0..file.hits.len()).map(|h| Item::Hit(f, h)));
            }
        }
    }

    fn toggle_file(&mut self, f: usize, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&f) {
            self.collapsed.insert(f);
        }
        self.rebuild();
        cx.notify();
    }

    fn open_hit(
        &mut self,
        f: usize,
        h: usize,
        pin: bool,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(found), Some(query)) = (&self.found, &self.found_for) else {
            return;
        };
        let Some(file) = found.files.get(f) else {
            return;
        };
        let Some(hit) = file.hits.get(h) else { return };
        self.picked = Some((f, h));
        let reveal = Reveal::Match {
            line: hit.line,
            query: query.clone(),
        };
        open_file(
            &self.workspace,
            &file.path,
            OpenOptions {
                preview: !pin,
                reveal: Some(reveal),
                focus,
            },
            window,
            cx,
        );
        cx.notify();
    }

    /// The keyboard's row, how many rows, and Match Case.
    #[cfg(test)]
    pub(crate) fn keyboard(&self) -> (Option<usize>, usize, bool) {
        (self.cursor, self.items.len(), self.match_case)
    }

    /// The box's keys: ⌥⌘C/W/R flip a toggle, ↓ goes to the results, Esc clears the query, then
    /// goes back to the Explorer.
    fn on_box_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &e.keystroke;
        let mut q = FindQuery {
            text: String::new(),
            match_case: self.match_case,
            whole_word: self.whole_word,
            regex: self.regex,
        };
        if q.flip_on_key(ks) {
            cx.stop_propagation();
            (self.match_case, self.whole_word, self.regex) = (q.match_case, q.whole_word, q.regex);
            return self.schedule(Duration::ZERO, cx);
        }
        let m = &ks.modifiers;
        if m.alt || m.platform || m.control || m.shift {
            return;
        }
        match ks.key.as_str() {
            "down" if !self.items.is_empty() => {
                let picked = self.picked.and_then(|(f, h)| {
                    self.items
                        .iter()
                        .position(|i| matches!(i, Item::Hit(a, b) if (*a, *b) == (f, h)))
                });
                self.cursor = Some(self.cursor.or(picked).unwrap_or(0));
                self.list_focus.focus(window, cx);
                cx.notify();
            }
            "escape" if !self.input.read(cx).value().is_empty() => {
                self.input.update(cx, |s, cx| s.set_value("", window, cx));
                self.schedule(Duration::ZERO, cx);
            }
            "escape" => self.leave(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Back to the Explorer, focused.
    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = self.workspace.upgrade() else {
            return;
        };
        ws.update(cx, |ws, cx| ws.show_panel(crate::explorer::PANEL_ID, cx));
        let panel = Workspace::ensure_panel(&ws, crate::explorer::PANEL_ID, window, cx);
        if let Some(focus) = panel.and_then(|p| p.focus_handle(cx)) {
            focus.focus(window, cx);
        }
    }

    /// The results' keys, as the sidebar lists have them: ↑/↓, Home/End and PgUp/PgDn move
    /// (landing on a match previews it), ↵ keeps a match's tab, Space previews it, both open or
    /// close a file; → opens a file or goes into the match's code, ← closes it or goes up to it.
    fn on_list_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &e.keystroke;
        let m = &ks.modifiers;
        if m.alt || m.platform || m.control || m.shift || self.items.is_empty() {
            return;
        }
        let n = self.items.len();
        let i = self.cursor.filter(|c| *c < n).unwrap_or(0);
        let height = f32::from(self.scroll.0.borrow().base_handle.bounds().size.height);
        let page = ((height / ROW_HEIGHT) as usize).saturating_sub(1).max(1);
        let to = match ks.key.as_str() {
            "down" => Some((i + 1).min(n - 1)),
            "up" => Some(i.saturating_sub(1)),
            "home" => Some(0),
            "end" => Some(n - 1),
            "pagedown" => Some((i + page).min(n - 1)),
            "pageup" => Some(i.saturating_sub(page)),
            _ => None,
        };
        if let Some(to) = to {
            cx.stop_propagation();
            return self.land(to, window, cx);
        }
        self.cursor = Some(i);
        match (ks.key.as_str(), self.items[i]) {
            ("enter" | "space", Item::File(f)) => self.toggle_file(f, cx),
            ("enter", Item::Hit(f, h)) => self.open_hit(f, h, true, false, window, cx),
            ("space", Item::Hit(f, h)) => self.open_hit(f, h, false, false, window, cx),
            ("right", Item::File(f)) if self.collapsed.contains(&f) => self.toggle_file(f, cx),
            ("right", Item::File(_)) if i + 1 < n => self.land(i + 1, window, cx),
            ("right", Item::Hit(f, h)) => self.open_hit(f, h, false, true, window, cx),
            ("left", Item::File(f)) if !self.collapsed.contains(&f) => self.toggle_file(f, cx),
            ("left", Item::Hit(f, _)) => {
                if let Some(up) = self
                    .items
                    .iter()
                    .position(|it| matches!(it, Item::File(g) if *g == f))
                {
                    self.land(up, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The keyboard's row moves to `ix`, scrolled to; a match there is previewed.
    fn land(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.cursor = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        if let Some(Item::Hit(f, h)) = self.items.get(ix).copied()
            && self.picked != Some((f, h))
        {
            self.open_hit(f, h, false, false, window, cx);
        }
        cx.notify();
    }

    fn all_collapsed(&self) -> bool {
        self.found.as_ref().is_some_and(|f| {
            !f.files.is_empty() && (0..f.files.len()).all(|i| self.collapsed.contains(&i))
        })
    }

    fn toggle(
        &self,
        id: &'static str,
        icon: IconName,
        tip: &'static str,
        on: bool,
        flip: fn(&mut Self),
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .ghost()
            .xsmall()
            .icon(icon)
            .tooltip(tip)
            .toggle(on, cx)
            .on_click(cx.listener(move |this, _, _, cx| {
                flip(this);
                this.schedule(Duration::ZERO, cx);
            }))
    }

    fn render_item(&self, ix: usize, keyboard: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme();
        let Some(found) = &self.found else {
            return div().into_any_element();
        };
        match self.items[ix] {
            Item::File(f) => {
                let file = &found.files[f];
                let open = !self.collapsed.contains(&f);
                let (dir, name) = file.path.rsplit_once('/').unwrap_or(("", &file.path));
                div()
                    .id(("search-file", f))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(24.))
                    .px_2()
                    .text_size(px(12.))
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover))
                    .when(keyboard, |d| d.bg(t.hover))
                    .child(
                        Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(px(12.))
                        .text_color(t.subtle),
                    )
                    .child(FileIcon::file(&file.path))
                    .child(
                        div()
                            .flex_none()
                            .text_color(t.foreground.opacity(0.9))
                            .child(name.to_string()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(t.subtle)
                            .child(dir.to_string()),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .px_1()
                            .rounded(px(3.))
                            .bg(t.elevated)
                            .font_family(t.fonts.code.clone())
                            .text_size(px(10.))
                            .text_color(t.muted_foreground)
                            .child(file.hits.len().to_string()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.cursor = Some(ix);
                        this.toggle_file(f, cx)
                    }))
                    .into_any_element()
            }
            Item::Hit(f, h) => {
                let hit = &found.files[f].hits[h];
                let text = hit.text.trim_start().to_string();
                let marks = self
                    .found_for
                    .as_ref()
                    .map(|q| find_matches(&text, q, 50))
                    .unwrap_or_default();
                let style = HighlightStyle {
                    background_color: Some(t.find_match),
                    ..Default::default()
                };
                let styled =
                    StyledText::new(text).with_highlights(marks.into_iter().map(|r| (r, style)));
                div()
                    .id(("search-hit", ix))
                    .relative()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(24.))
                    .pl(px(32.))
                    .pr_2()
                    .cursor_pointer()
                    .when(self.picked == Some((f, h)), |d| {
                        d.bg(t.primary.opacity(0.15))
                    })
                    .hover(|s| s.bg(t.hover))
                    .when(keyboard, |d| d.bg(t.hover))
                    .child(
                        div()
                            .w(px(28.))
                            .flex_none()
                            .text_right()
                            .font_family(t.fonts.code.clone())
                            .text_size(px(10.5))
                            .text_color(t.subtle)
                            .child(hit.line.to_string()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(t.fonts.code.clone())
                            .text_size(px(11.5))
                            .text_color(t.foreground.opacity(0.8))
                            .child(styled),
                    )
                    .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                        this.cursor = Some(ix);
                        this.open_hit(f, h, e.click_count() >= 2, false, window, cx)
                    }))
                    .into_any_element()
            }
        }
    }
}

/// Where `query` matches in `text` (byte ranges), found again here as git found the line; regex
/// queries aren't marked.
pub fn find_matches(text: &str, query: &Query, max: usize) -> Vec<std::ops::Range<usize>> {
    if query.text.is_empty() || query.regex {
        return vec![];
    }
    let (hay, needle) = if query.match_case {
        (text.to_string(), query.text.clone())
    } else {
        (text.to_lowercase(), query.text.to_lowercase())
    };
    // Lowercasing can change a string's length; marks would then land wrong.
    if hay.len() != text.len() {
        return vec![];
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let (start, end) = (from + at, from + at + needle.len());
        let whole = !query.whole_word
            || (!word(hay[..start].chars().next_back()) && !word(hay[end..].chars().next()));
        if whole {
            out.push(start..end);
            if out.len() >= max {
                break;
            }
        }
        from = end.max(start + 1);
        while !hay.is_char_boundary(from) {
            from += 1;
        }
    }
    out
}

impl Render for SearchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let globs = self.globs
            || !self.include.read(cx).value().is_empty()
            || !self.exclude.read(cx).value().is_empty();
        let status: Option<SharedString> = self.error.clone().or_else(|| {
            let found = self.found.as_ref()?;
            let files = found.files.len();
            let counted = if found.count == 0 {
                "No results found.".to_string()
            } else {
                format!(
                    "{} {} in {} {}",
                    found.count,
                    if found.count == 1 {
                        "result"
                    } else {
                        "results"
                    },
                    files,
                    if files == 1 { "file" } else { "files" }
                )
            };
            Some(
                if found.posix {
                    format!("{counted} · POSIX regex (git without PCRE)")
                } else {
                    counted
                }
                .into(),
            )
        });
        let has_results = self.found.as_ref().is_some_and(|f| f.count > 0);
        let all_collapsed = self.all_collapsed();
        let footer = self.found.as_ref().and_then(|f| {
            if f.capped {
                Some(format!(
                    "Showing the first {} results. Narrow the search to see the rest.",
                    grep::MAX_HITS
                ))
            } else if f.timed_out {
                Some(
                    "The search stopped after 10 seconds; these are the results by then."
                        .to_string(),
                )
            } else {
                None
            }
        });
        let count = self.items.len();
        // An empty panel says what it searches rather than nothing at all.
        let idle = self.found.is_none()
            && self.error.is_none()
            && !self.pending
            && self.input.read(cx).value().is_empty();
        div()
            .id("search")
            .key_context("SearchPanel")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .text_color(t.foreground)
            .child(
                div()
                    .capture_key_down(cx.listener(Self::on_box_key))
                    // ↵ searches now. Taken here, not as the Input's PressEnter: a one-line Input
                    // lets the key go on to be typed, and that Change would wait out the pause.
                    .on_action(
                        cx.listener(|this, _: &Enter, _, cx| this.schedule(Duration::ZERO, cx)),
                    )
                    .flex_none()
                    .p_2()
                    .gap(px(6.))
                    .flex()
                    .flex_col()
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&self.input).small().suffix(
                                        div()
                                            .flex()
                                            .child(self.toggle(
                                                "case",
                                                IconName::CaseSensitive,
                                                "Match Case",
                                                self.match_case,
                                                |s| s.match_case = !s.match_case,
                                                cx,
                                            ))
                                            .child(self.toggle(
                                                "word",
                                                IconName::WholeWord,
                                                "Match Whole Word",
                                                self.whole_word,
                                                |s| s.whole_word = !s.whole_word,
                                                cx,
                                            ))
                                            .child(self.toggle(
                                                "regex",
                                                IconName::Regex,
                                                "Use Regular Expression",
                                                self.regex,
                                                |s| s.regex = !s.regex,
                                                cx,
                                            )),
                                    ),
                                ),
                            )
                            .child(
                                Button::new("globs")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Ellipsis)
                                    .tooltip("Toggle search details")
                                    .toggle(globs, cx)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.globs = !this.globs;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when(globs, |d| {
                        d.child(
                            div()
                                .text_size(px(10.5))
                                .text_color(t.subtle)
                                .child("files to include"),
                        )
                        .child(Input::new(&self.include).xsmall())
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(t.subtle)
                                .child("files to exclude"),
                        )
                        .child(Input::new(&self.exclude).xsmall())
                    }),
            )
            .when(status.is_some() || self.pending, |d| {
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .py_1()
                        .pl_3()
                        .pr_1()
                        .text_size(px(11.5))
                        .when(self.pending, |d| {
                            d.child(
                                Icon::new(IconName::LoaderCircle)
                                    .size(px(12.))
                                    .text_color(t.subtle),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(if self.error.is_some() {
                                    t.removed
                                } else {
                                    t.muted_foreground
                                })
                                .child(if self.pending && self.found.is_none() {
                                    "Searching…".into()
                                } else {
                                    status.clone().unwrap_or_default()
                                }),
                        )
                        .when(has_results, |d| {
                            d.child(
                                Button::new("again")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::RefreshCw)
                                    .tooltip("Refresh")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.schedule(Duration::ZERO, cx)
                                    })),
                            )
                            .child(
                                Button::new("collapse")
                                    .ghost()
                                    .xsmall()
                                    .icon(if all_collapsed {
                                        IconName::ChevronsUpDown
                                    } else {
                                        IconName::ChevronsDownUp
                                    })
                                    .tooltip(if all_collapsed {
                                        "Expand all"
                                    } else {
                                        "Collapse all"
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.collapsed = if all_collapsed {
                                            HashSet::new()
                                        } else {
                                            (0..this.found.as_ref().map_or(0, |f| f.files.len()))
                                                .collect()
                                        };
                                        this.rebuild();
                                        cx.notify();
                                    })),
                            )
                        }),
                )
            })
            .child(
                div()
                    .id("search-results-box")
                    .key_context("SearchResults")
                    .track_focus(&self.list_focus)
                    .on_key_down(cx.listener(Self::on_list_key))
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when(idle, |d| {
                        d.child(
                            div()
                                .px_3()
                                .py_2()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .text_size(px(11.5))
                                .text_color(t.muted_foreground)
                                .child("Type to search the text of the repo's files.")
                                .child(div().text_color(t.subtle).child(
                                    "Ignored files are left out; ⋯ narrows the search to some paths.",
                                )),
                        )
                    })
                    .child(
                        uniform_list(
                            "search-results",
                            count,
                            cx.processor(|this, range: std::ops::Range<usize>, window, cx| {
                                let at = this.cursor.filter(|_| this.list_focus.is_focused(window));
                                range
                                    .map(|ix| this.render_item(ix, at == Some(ix), cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .flex_1(),
                    ),
            )
            .children(footer.map(|text| {
                div()
                    .flex_none()
                    .px_4()
                    .py_2()
                    .text_center()
                    .text_size(px(11.))
                    .text_color(t.subtle)
                    .child(text)
            }))
    }
}

impl Drop for SearchPanel {
    fn drop(&mut self) {
        // The view went: a search still running in git stops.
        self.stop_running();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str, case: bool, word: bool) -> Query {
        Query {
            text: text.into(),
            match_case: case,
            whole_word: word,
            ..Default::default()
        }
    }

    #[test]
    fn matches_marked_as_git_found_them() {
        assert_eq!(
            find_matches("Foo foo food", &q("foo", false, false), 10),
            vec![0..3, 4..7, 8..11]
        );
        assert_eq!(
            find_matches("Foo foo food", &q("foo", true, false), 10),
            vec![4..7, 8..11]
        );
        assert_eq!(
            find_matches("Foo foo food", &q("foo", false, true), 10),
            vec![0..3, 4..7]
        );
        assert_eq!(
            find_matches("aaaa", &q("aa", false, false), 10),
            vec![0..2, 2..4]
        );
        assert!(
            find_matches(
                "x",
                &Query {
                    regex: true,
                    ..q("x", false, false)
                },
                10
            )
            .is_empty()
        );
    }
}
