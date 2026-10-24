//! A whole list of files as one scroll of diffs (Open All, a commit's files, a guided review), as
//! GitHub's Files changed shows a pull request. Only the files near the screen are read,
//! parsed and laid out; one scrolled far away goes back to the room it takes, so a 500-file
//! change holds a few dozen at a time. J/K walk the files, V marks the one on top viewed, C
//! puts a note on the line under the pointer, else the first changed one in view.

use crate::actions::{AddNote, NextFile, PrevFile, STACKED_CONTEXT, ToggleViewed};
use crate::data::DiffData;
use crate::highlight::{self, Highlighted};
use crate::list::{Heights, Place, Scroll, vlist};
use crate::model::display::{self, CONTEXT as CONTEXT_LINES, FOLD_MIN, Item};
use crate::model::{LinePos, LineRange, REMOVED, Side};
use crate::paint::{self, Metrics};
use crate::source::{Compare, DiffSource};
use crate::style::{DiffOptions, DiffStyle};
use crate::view::Notice;
use gitviber_core::Core;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, MouseButton, MouseMoveEvent, ParentElement as _, Pixels,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

/// More lines than this in one file wait for a click, as GitHub's large diffs do: a lockfile
/// would hold up the rest.
const LARGE: usize = 1500;
/// How far off screen a file starts loading, so it's drawn by the time it scrolls in.
const AHEAD: f32 = 1200.;
/// Files further than this many screens away give their text back.
const KEEP_SCREENS: f32 = 4.;
/// And never more than this many held at once.
const MAX_LOADED: usize = 32;
const LOADING_AT_ONCE: usize = 4;
const HEADER: f32 = 32.;
const MESSAGE: f32 = 40.;
const NOTE: f32 = 22.;

/// One file of the list, as its owner knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct StackedFile {
    pub source: DiffSource,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    /// What the file's content is now (status's oid, the index blob under it…): a new one reads
    /// it again and forgets the folds opened in it.
    pub sig: String,
    pub viewed: bool,
}

impl StackedFile {
    pub fn new(source: DiffSource) -> Self {
        StackedFile {
            source,
            additions: None,
            deletions: None,
            sig: String::new(),
            viewed: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum StackedEvent {
    /// A file's Viewed button, or V on the file on top.
    ToggleViewed(DiffSource),
    /// A file's path was clicked: open its own diff.
    OpenFile(DiffSource),
    /// C, or a line's note button.
    AddNote {
        source: DiffSource,
        range: LineRange,
    },
    Notice(Notice),
}

struct Loaded {
    data: DiffData,
    items: Vec<Item>,
    /// Whether `items` fold unchanged lines.
    collapse: bool,
    hl: [Option<Highlighted>; 2],
    _highlight: Task<()>,
}

enum Body {
    Idle,
    /// Dropping the task stops the read.
    Loading {
        _task: Task<()>,
    },
    Ready(Box<Loaded>),
    Failed(String),
}

/// What a file keeps while the list is open.
struct Slot {
    file: StackedFile,
    /// Opened or closed by hand; None: closed once viewed (staged ones count as viewed, and stay open).
    shut: Option<bool>,
    /// Its large diff was asked for.
    large: bool,
    /// Unchanged lines (new line numbers) opened from their fold.
    revealed: HashSet<u32>,
    /// Its body's height as last laid out: the room it keeps while it isn't.
    height: Option<Pixels>,
    body: Body,
}

impl Slot {
    fn is_open(&self) -> bool {
        !self
            .shut
            .unwrap_or(self.file.source.compare != Compare::Staged && self.file.viewed)
    }

    fn estimate(&self, line: Pixels) -> Pixels {
        let lines = (self.file.additions.unwrap_or(0) + self.file.deletions.unwrap_or(0)) as usize;
        line * (lines.min(LARGE) + 2 * CONTEXT_LINES + 1) as f32
    }
}

/// A row of the scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Header(usize),
    /// The header's note: a mode change, a newline at the end…
    Note(usize),
    /// A file not read yet, or given back: its height.
    Room(usize),
    Message(usize),
    Line(usize, Item),
}

impl Row {
    fn file(&self) -> usize {
        match *self {
            Row::Header(f) | Row::Note(f) | Row::Room(f) | Row::Message(f) | Row::Line(f, _) => f,
        }
    }
}

pub struct StackedDiff {
    core: Arc<Core>,
    focus: FocusHandle,
    slots: Vec<Slot>,
    rows: Vec<Row>,
    /// Where each file's header is in `rows`.
    starts: Vec<usize>,
    heights: Rc<Heights>,
    line: Pixels,
    scroll: Scroll,
    dirty: bool,
    options: DiffOptions,
    hover: Option<(usize, usize)>,
    notes: bool,
    /// The bar above the files: what they are ("All Changes", a commit's subject); None hides it.
    title: Option<SharedString>,
}

impl EventEmitter<StackedEvent> for StackedDiff {}

impl Focusable for StackedDiff {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl StackedDiff {
    pub fn new(files: Vec<StackedFile>, cx: &mut Context<Self>) -> Self {
        let mut view = StackedDiff {
            core: crate::core(cx),
            focus: cx.focus_handle(),
            slots: vec![],
            rows: vec![],
            starts: vec![],
            heights: Rc::default(),
            line: DiffStyle::get(cx).line_px(),
            scroll: Scroll::default(),
            dirty: true,
            options: DiffOptions::get(cx),
            hover: None,
            notes: false,
            title: None,
        };
        view.set_files(files, cx);
        view
    }

    /// The list changed (a refresh of status): files keep what they had while their sig is
    /// the same; a new sig reads one again.
    pub fn set_files(&mut self, files: Vec<StackedFile>, cx: &mut Context<Self>) {
        let mut old: Vec<Option<Slot>> = std::mem::take(&mut self.slots)
            .into_iter()
            .map(Some)
            .collect();
        let keys: Vec<String> = old
            .iter()
            .map(|s| s.as_ref().map(|s| s.file.source.key()).unwrap_or_default())
            .collect();
        for file in files {
            let key = file.source.key();
            let kept = keys
                .iter()
                .position(|k| *k == key)
                .and_then(|i| old[i].take());
            let slot = match kept {
                Some(mut slot) if slot.file.sig == file.sig => {
                    slot.file = file;
                    slot
                }
                Some(slot) => Slot {
                    file,
                    shut: slot.shut,
                    large: slot.large,
                    revealed: HashSet::new(),
                    height: slot.height,
                    body: Body::Idle,
                },
                None => Slot {
                    file,
                    shut: None,
                    large: false,
                    revealed: HashSet::new(),
                    height: None,
                    body: Body::Idle,
                },
            };
            self.slots.push(slot);
        }
        self.dirty = true;
        cx.notify();
    }

    /// Lines take review notes (C, the line buttons).
    pub fn set_notes(&mut self, notes: bool, cx: &mut Context<Self>) {
        self.notes = notes;
        cx.notify();
    }

    pub fn set_title(&mut self, title: Option<SharedString>, cx: &mut Context<Self>) {
        self.title = title;
        cx.notify();
    }

    /// Scrolls by `dy` (tests, and the app's page keys).
    pub fn scroll_by(&mut self, dy: Pixels, cx: &mut Context<Self>) {
        self.scroll.set_top(self.scroll.top() + dy);
        cx.notify();
    }

    pub fn files(&self) -> impl Iterator<Item = &StackedFile> {
        self.slots.iter().map(|s| &s.file)
    }

    /// Files whose text is held now (the rest keep only their height).
    pub fn loaded_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| matches!(s.body, Body::Ready(_)))
            .count()
    }

    pub fn set_all_open(&mut self, open: bool, cx: &mut Context<Self>) {
        for s in &mut self.slots {
            s.shut = Some(!open);
        }
        self.dirty = true;
        cx.notify();
    }

    pub fn toggle_open(&mut self, file: usize, cx: &mut Context<Self>) {
        if let Some(s) = self.slots.get_mut(file) {
            s.shut = Some(s.is_open());
            self.dirty = true;
            cx.notify();
        }
    }

    /// The file whose header is at the top of the view.
    pub fn top_file(&self) -> Option<usize> {
        let ix = self.heights.index_at(self.scroll.top() + px(1.));
        self.rows.get(ix).map(Row::file)
    }

    /// Where the view is, to come back to: the top file's key and how far into it.
    pub fn scroll_position(&self) -> Option<(String, Pixels)> {
        let f = self.top_file()?;
        let top = self.heights.top(self.starts[f]);
        Some((self.slots[f].file.source.key(), self.scroll.top() - top))
    }

    pub fn restore_scroll(&mut self, key: &str, offset: Pixels, cx: &mut Context<Self>) {
        self.rebuild_if_dirty(None);
        if let Some(f) = self.slots.iter().position(|s| s.file.source.key() == key) {
            self.scroll.scroll_to(self.starts[f], offset, Place::Top);
            cx.notify();
        }
    }

    /// J / K: the next or previous file's header to the top.
    pub fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.slots.is_empty() {
            return;
        }
        let at = self.top_file().unwrap_or(0);
        let to = if forward {
            (at + 1).min(self.slots.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        self.scroll.scroll_to(self.starts[to], px(0.), Place::Top);
        cx.notify();
    }

    fn toggle_top_viewed(&mut self, cx: &mut Context<Self>) {
        let Some(f) = self.top_file() else { return };
        if self.slots[f].file.source.compare == Compare::Staged {
            return;
        }
        self.mark_viewed(f, cx);
    }

    fn mark_viewed(&mut self, f: usize, cx: &mut Context<Self>) {
        self.slots[f].shut = None;
        cx.emit(StackedEvent::ToggleViewed(
            self.slots[f].file.source.clone(),
        ));
        // Closing the file being read would leave the view somewhere in the next one.
        if self.heights.top(self.starts[f]) < self.scroll.top() {
            self.scroll.scroll_to(self.starts[f], px(0.), Place::Top);
        }
    }

    fn add_note(&mut self, cx: &mut Context<Self>) {
        if !self.notes {
            cx.propagate();
            return;
        }
        let pick = self.hover.or_else(|| self.first_change_in_view());
        if let Some((f, ix)) = pick
            && let Some(range) = self.row_range(f, ix)
        {
            cx.emit(StackedEvent::AddNote {
                source: self.slots[f].file.source.clone(),
                range,
            });
        }
    }

    fn row_range(&self, f: usize, ix: usize) -> Option<LineRange> {
        let Row::Line(_, Item::Line(r)) = *self.rows.get(ix)? else {
            return None;
        };
        let Body::Ready(l) = &self.slots[f].body else {
            return None;
        };
        let row = &l.data.rows[r as usize];
        let pos = if row.k == REMOVED {
            LinePos {
                side: Side::Old,
                line: row.o,
            }
        } else {
            LinePos {
                side: Side::New,
                line: row.n,
            }
        };
        Some(LineRange::single(pos))
    }

    fn first_change_in_view(&self) -> Option<(usize, usize)> {
        let top = self.heights.index_at(self.scroll.top() + px(HEADER));
        let bottom = self
            .heights
            .index_at(self.scroll.top() + self.scroll.viewport().size.height);
        let mut first = None;
        for ix in top..=bottom.min(self.rows.len().saturating_sub(1)) {
            if let Row::Line(f, Item::Line(r)) = self.rows[ix]
                && let Body::Ready(l) = &self.slots[f].body
            {
                first.get_or_insert((f, ix));
                if l.data.rows[r as usize].k != 0 {
                    return Some((f, ix));
                }
            }
        }
        first
    }

    /// Hands a file its diff already read (a prefetch); `parse_now` also parses it here, for a
    /// demo window that must be whole at its first frame.
    #[doc(hidden)]
    pub fn preload(&mut self, f: usize, data: DiffData, parse_now: bool, cx: &mut Context<Self>) {
        if f >= self.slots.len() {
            return;
        }
        self.slots[f].body = Body::Loading {
            _task: Task::ready(()),
        };
        self.loaded(f, Ok(data), cx);
        let language = highlight::language_for(&self.slots[f].file.source.path);
        let theme = DiffStyle::get(cx).syntax.clone();
        if parse_now && let Body::Ready(l) = &mut self.slots[f].body {
            for (i, side) in [&l.data.old, &l.data.new].into_iter().enumerate() {
                if side.exists
                    && side.textual()
                    && let Some(h) = highlight::parse(language, &side.rope)
                {
                    l.hl[i] = Some(Highlighted::new(h, side.rope.clone(), theme.clone()));
                }
            }
            l._highlight = Task::ready(());
        }
    }

    // Loading and giving back

    fn load(&mut self, f: usize, cx: &mut Context<Self>) {
        let (core, source, ws) = (
            self.core.clone(),
            self.slots[f].file.source.clone(),
            self.options.whitespace,
        );
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { DiffData::load(&core, &source, ws) })
                .await;
            this.update(cx, |this, cx| this.loaded(f, result, cx)).ok();
        });
        self.slots[f].body = Body::Loading { _task: task };
    }

    fn loaded(&mut self, f: usize, result: Result<DiffData, String>, cx: &mut Context<Self>) {
        let Some(slot) = self.slots.get_mut(f) else {
            return;
        };
        if !matches!(slot.body, Body::Loading { .. }) {
            return;
        }
        slot.body = match result {
            Err(e) => Body::Failed(e),
            Ok(data) => {
                let collapse = self.options.collapse;
                let items = file_items(&data, collapse, &slot.revealed);
                let language =
                    slot.file.source.language.clone().unwrap_or_else(|| {
                        highlight::language_for(&slot.file.source.path).to_string()
                    });
                let ropes = [&data.old, &data.new]
                    .map(|s| (s.exists && s.textual()).then(|| s.rope.clone()));
                let theme = DiffStyle::get(cx).syntax.clone();
                let task = cx.spawn(async move |this, cx| {
                    let parsed = cx
                        .background_spawn(async move {
                            ropes.map(|r| {
                                r.and_then(|r| highlight::parse(&language, &r).map(|h| (h, r)))
                            })
                        })
                        .await;
                    this.update(cx, |this, cx| {
                        if let Some(Body::Ready(l)) = this.slots.get_mut(f).map(|s| &mut s.body) {
                            for (i, p) in parsed.into_iter().enumerate() {
                                if let Some((h, rope)) = p {
                                    l.hl[i] = Some(Highlighted::new(h, rope, theme.clone()));
                                }
                            }
                            cx.notify();
                        }
                    })
                    .ok();
                });
                Body::Ready(Box::new(Loaded {
                    data,
                    items,
                    collapse,
                    hl: [None, None],
                    _highlight: task,
                }))
            }
        };
        self.dirty = true;
        cx.notify();
    }

    /// Reads the files near the screen, gives back those far from it.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        let (top, view_h) = (
            self.scroll.top(),
            self.scroll.viewport().size.height.max(px(400.)),
        );
        let near = (top - px(AHEAD), top + view_h + px(AHEAD));
        let keep = (
            top - view_h * KEEP_SCREENS,
            top + view_h * (KEEP_SCREENS + 1.),
        );
        let (heights, starts) = (self.heights.clone(), self.starts.clone());
        let span = move |f: usize| {
            let start = heights.top(starts[f]);
            let end = starts
                .get(f + 1)
                .map_or(heights.total(), |&s| heights.top(s));
            (start, end)
        };
        let mut loading = self
            .slots
            .iter()
            .filter(|s| matches!(s.body, Body::Loading { .. }))
            .count();
        let mut give_back = vec![];
        for f in 0..self.slots.len() {
            let (start, end) = span(f);
            let is_near = end >= near.0 && start <= near.1;
            let kept = end >= keep.0 && start <= keep.1;
            match self.slots[f].body {
                Body::Idle if is_near && self.slots[f].is_open() && loading < LOADING_AT_ONCE => {
                    self.load(f, cx);
                    loading += 1;
                }
                Body::Loading { .. } if !kept => self.slots[f].body = Body::Idle,
                Body::Ready(_) if !kept || !self.slots[f].is_open() => give_back.push(f),
                _ => {}
            }
        }
        // Past the cap, the farthest go first.
        let mut ready: Vec<(Pixels, usize)> = (0..self.slots.len())
            .filter(|&f| matches!(self.slots[f].body, Body::Ready(_)) && !give_back.contains(&f))
            .map(|f| {
                let (start, end) = span(f);
                let d = if end < top {
                    top - end
                } else if start > top + view_h {
                    start - top - view_h
                } else {
                    px(0.)
                };
                (d, f)
            })
            .collect();
        if ready.len() > MAX_LOADED {
            ready.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            give_back.extend(ready.iter().take(ready.len() - MAX_LOADED).map(|(_, f)| *f));
        }
        for f in give_back {
            self.slots[f].body = Body::Idle;
            self.dirty = true;
        }
    }

    // Rows

    fn rebuild_if_dirty(&mut self, anchor: Option<(usize, Pixels)>) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let anchor = anchor.or_else(|| {
            let f = self.top_file()?;
            Some((f, self.scroll.top() - self.heights.top(self.starts[f])))
        });
        let line = self.line;
        let collapse = self.options.collapse;
        let mut rows = Vec::new();
        let mut heights = Vec::new();
        self.starts.clear();
        for (f, slot) in self.slots.iter_mut().enumerate() {
            self.starts.push(rows.len());
            rows.push(Row::Header(f));
            heights.push(px(HEADER));
            if !slot.is_open() {
                continue;
            }
            let body_start = heights.len();
            match &mut slot.body {
                Body::Idle | Body::Loading { .. } => {
                    rows.push(Row::Room(f));
                    heights.push(slot.height.unwrap_or_else(|| slot.estimate(line)));
                    continue;
                }
                Body::Failed(_) => {
                    rows.push(Row::Message(f));
                    heights.push(px(MESSAGE));
                }
                Body::Ready(l) => {
                    if l.items.is_empty()
                        || l.data.placeholder.is_some()
                        || is_media(&slot.file.source.path)
                        || (l.items.len() > LARGE && !slot.large)
                    {
                        rows.push(Row::Message(f));
                        heights.push(px(MESSAGE));
                    } else {
                        if collapse != l.collapse {
                            l.items = file_items(&l.data, collapse, &slot.revealed);
                            l.collapse = collapse;
                        }
                        if l.data.note.is_some() {
                            rows.push(Row::Note(f));
                            heights.push(px(NOTE));
                        }
                        rows.extend(l.items.iter().map(|it| Row::Line(f, *it)));
                        heights.extend(std::iter::repeat_n(line, l.items.len()));
                    }
                }
            }
            slot.height = Some(heights[body_start..].iter().fold(px(0.), |a, h| a + *h));
        }
        self.rows = rows;
        self.heights = Rc::new(Heights::from_heights(heights));
        if let Some((f, offset)) = anchor
            && let Some(&ix) = self.starts.get(f)
        {
            self.scroll.scroll_to(ix, offset, Place::Top);
        }
    }

    fn reveal(&mut self, f: usize, start: u32, count: u32, cx: &mut Context<Self>) {
        let Some(slot) = self.slots.get_mut(f) else {
            return;
        };
        let Body::Ready(l) = &mut slot.body else {
            return;
        };
        slot.revealed
            .extend(display::reveal(&l.data.rows, start, count));
        l.items = file_items(&l.data, self.options.collapse, &slot.revealed);
        self.dirty = true;
        cx.notify();
    }

    fn sync_options(&mut self, cx: &mut Context<Self>) {
        let now = DiffOptions::get(cx);
        let line = DiffStyle::get(cx).line_px();
        if now.whitespace != self.options.whitespace {
            for s in &mut self.slots {
                s.body = Body::Idle;
            }
        }
        if now != self.options || line != self.line {
            self.options = now;
            self.line = line;
            self.dirty = true;
        }
    }

    // Drawing

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let style = DiffStyle::get(cx).clone();
        let lines = self
            .slots
            .iter()
            .filter_map(|s| {
                if let Body::Ready(l) = &s.body {
                    Some(l.data.old.lines.max(l.data.new.lines))
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(1);
        let m = Metrics::new(window, &style, lines, self.line, self.scroll.left());
        range
            .map(|ix| self.render_row(ix, &m, &style, cx))
            .collect()
    }

    fn render_row(
        &self,
        ix: usize,
        m: &Metrics,
        style: &DiffStyle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &style.colors;
        match self.rows[ix] {
            Row::Header(f) => self.header(f, style, cx),
            Row::Note(f) => {
                let note = if let Body::Ready(l) = &self.slots[f].body {
                    l.data.note.clone()
                } else {
                    None
                };
                div()
                    .h(px(NOTE))
                    .px_4()
                    .flex()
                    .items_center()
                    .text_size(style.ui_font_size - px(0.5))
                    .text_color(c.subtle)
                    .children(note)
                    .into_any_element()
            }
            Row::Room(_) => div().size_full().bg(c.background).into_any_element(),
            Row::Message(f) => self.message(f, style, cx),
            Row::Line(f, Item::Gap { start, count }) => {
                let more = count.min(display::FOLD_REVEAL);
                h_flex()
                    .id(("stacked-gap", ix))
                    .w_full()
                    .h(m.line)
                    .gap_1p5()
                    .px_3()
                    .bg(c.panel)
                    .text_size(style.ui_font_size - px(1.))
                    .text_color(c.subtle)
                    .cursor_pointer()
                    .hover(|s| s.bg(c.elevated).text_color(c.foreground))
                    .child(Icon::new(Lucide::ChevronsUpDown).size_3())
                    .child(paint::gap_label(count))
                    .when(count > more, |el| el.child(format!("· show {more} more")))
                    .on_click(cx.listener(move |this, _, _, cx| this.reveal(f, start, count, cx)))
                    .into_any_element()
            }
            Row::Line(f, item) => {
                let Body::Ready(l) = &self.slots[f].body else {
                    return div().into_any_element();
                };
                let r = match item {
                    Item::Line(r) => r,
                    Item::Pair { old, new } => new.or(old).unwrap_or(0),
                    Item::Gap { .. } => 0,
                };
                let row = &l.data.rows[r as usize];
                let t = paint::tone(row.k, style);
                let num = if row.k == 0 { c.subtle } else { c.muted };
                let side = if row.k == REMOVED {
                    Side::Old
                } else {
                    Side::New
                };
                let note = self.notes && self.hover == Some((f, ix));
                h_flex()
                    .id(("stacked-row", ix))
                    .relative()
                    .w_full()
                    .h(m.line)
                    .line_height(m.line)
                    .font_family(style.font_family.clone())
                    .text_size(style.font_size)
                    .bg(t.line)
                    .child(paint::number(
                        (row.k != crate::model::ADDED).then_some(row.o),
                        t.gutter,
                        num,
                        m,
                    ))
                    .child(paint::number(
                        (row.k != REMOVED).then_some(row.n),
                        t.gutter,
                        num,
                        m,
                    ))
                    .child(paint::sign(&t, style, m))
                    .child(paint::code_cell(
                        paint::code(&l.data, &l.hl, side, row, style),
                        m,
                    ))
                    .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                        if this.hover != Some((f, ix)) {
                            this.hover = Some((f, ix));
                            cx.notify();
                        }
                    }))
                    .when(note, |el| {
                        el.child(
                            div()
                                .id(("stacked-note", ix))
                                .absolute()
                                .top_0()
                                .left_0()
                                .w(px(16.))
                                .h_full()
                                .rounded_sm()
                                .bg(c.primary)
                                .text_color(c.primary_foreground)
                                .flex()
                                .justify_center()
                                .cursor_pointer()
                                .child("+")
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(range) = this.row_range(f, ix) {
                                        cx.emit(StackedEvent::AddNote {
                                            source: this.slots[f].file.source.clone(),
                                            range,
                                        });
                                    }
                                })),
                        )
                    })
                    .into_any_element()
            }
        }
    }

    fn header(&self, f: usize, style: &DiffStyle, cx: &mut Context<Self>) -> AnyElement {
        let c = &style.colors;
        let slot = &self.slots[f];
        let file = &slot.file;
        let open = slot.is_open();
        let counts = match &slot.body {
            Body::Ready(l) => Some((l.data.additions, l.data.deletions)),
            _ => file.additions.zip(file.deletions),
        };
        let staged = file.source.compare == Compare::Staged;
        h_flex()
            .id(("stacked-header", f))
            .h(px(HEADER))
            .w_full()
            .gap_2()
            .pl_1p5()
            .pr_2()
            .bg(c.panel)
            .border_b_1()
            .border_color(c.border)
            .text_size(style.ui_font_size)
            .text_color(c.foreground)
            .child(
                Button::new(("toggle", f))
                    .ghost()
                    .xsmall()
                    .icon(if open {
                        Lucide::ChevronDown
                    } else {
                        Lucide::ChevronRight
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_open(f, cx))),
            )
            .child(
                div()
                    .id(("open-file", f))
                    .min_w_0()
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(paint::path_label(&file.source.path, style))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.emit(StackedEvent::OpenFile(this.slots[f].file.source.clone()))
                    })),
            )
            .when_some(file.source.old_path.clone(), |el, old| {
                el.child(
                    div()
                        .text_color(c.subtle)
                        .truncate()
                        .child(format!("← {old}")),
                )
            })
            .when_some(counts, |el, (a, d)| {
                el.child(paint::line_counts(a, d, style))
            })
            .when(!file.source.file.status.is_empty(), |el| {
                el.child(paint::status_pill(&file.source.file.status, style))
            })
            .child(div().flex_1())
            .child(
                Button::new(("viewed", f))
                    .xsmall()
                    .icon(Lucide::Check)
                    .label("Viewed")
                    .toggled(file.viewed)
                    .tooltip(if staged {
                        "Unstage"
                    } else if file.viewed {
                        "Mark as not viewed"
                    } else {
                        "Mark as viewed"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.mark_viewed(f, cx))),
            )
            .into_any_element()
    }

    fn message(&self, f: usize, style: &DiffStyle, cx: &mut Context<Self>) -> AnyElement {
        let slot = &self.slots[f];
        let (text, action): (String, Option<&'static str>) = match &slot.body {
            Body::Failed(e) => (format!("Could not load: {e}"), None),
            Body::Ready(_) if is_media(&slot.file.source.path) => {
                ("Open the file to compare its versions".into(), Some("Open"))
            }
            Body::Ready(l) => match &l.data.placeholder {
                Some(p) => (p.clone(), None),
                None => (
                    format!("A large diff, {} lines", l.items.len()),
                    Some("Show"),
                ),
            },
            _ => (String::new(), None),
        };
        h_flex()
            .h(px(MESSAGE))
            .px_4()
            .gap_3()
            .text_size(style.ui_font_size)
            .text_color(style.colors.muted)
            .child(text)
            .when_some(action, |el, label| {
                el.child(
                    Button::new(("message", f))
                        .xsmall()
                        .label(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if label == "Show" {
                                this.slots[f].large = true;
                                this.dirty = true;
                                cx.notify();
                            } else {
                                cx.emit(StackedEvent::OpenFile(this.slots[f].file.source.clone()));
                            }
                        })),
                )
            })
            .into_any_element()
    }
}

/// The display items of one file, unified.
fn file_items(data: &DiffData, collapse: bool, revealed: &HashSet<u32>) -> Vec<Item> {
    let shown = if collapse {
        display::shown_rows(&data.rows, CONTEXT_LINES, FOLD_MIN, revealed)
    } else {
        display::all_rows(&data.rows)
    };
    display::unified(&shown)
}

pub fn is_media(path: &str) -> bool {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "avif"
            | "bmp"
            | "ico"
            | "tif"
            | "tiff"
            | "heic"
            | "mp4"
            | "m4v"
            | "mov"
            | "webm"
            | "ogv"
            | "mkv"
            | "3gp"
            | "mp3"
            | "wav"
            | "m4a"
            | "aac"
            | "flac"
            | "ogg"
            | "oga"
            | "opus"
            | "aif"
            | "aiff"
            | "pdf"
    )
}

impl Render for StackedDiff {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_options(cx);
        self.rebuild_if_dirty(None);
        self.schedule(cx);
        self.rebuild_if_dirty(None);
        let style = DiffStyle::get(cx).clone();
        let widest = self
            .slots
            .iter()
            .filter_map(|s| {
                if let Body::Ready(l) = &s.body {
                    Some(l.data.widest)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0);
        let m = Metrics::new(window, &style, 9999, self.line, px(0.));
        self.scroll
            .set_max_left(m.max_left(widest, self.scroll.viewport().size.width, 1., 2.));
        // The top file's header stays at the top while its lines scroll under it.
        let sticky = self
            .top_file()
            .filter(|&f| self.heights.top(self.starts[f]) < self.scroll.top());
        let view = cx.entity().downgrade();
        let body = div()
            .id("stacked-body")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .child(vlist(
                "stacked-rows",
                &self.scroll,
                self.heights.clone(),
                cx.entity_id(),
                move |range, window, cx| {
                    view.update(cx, |this, cx| this.render_rows(range, window, cx))
                        .unwrap_or_default()
                },
            ))
            .when_some(sticky, |el, f| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .child(self.header(f, &style, cx)),
                )
            })
            .child(Scrollbar::vertical(&self.scroll))
            .child(Scrollbar::horizontal(&self.scroll))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered && this.hover.take().is_some() {
                    cx.notify();
                }
            }));
        let empty = self.slots.is_empty();
        let bar = self.title.clone().map(|title| {
            let c = &style.colors;
            let (add, del) = self.slots.iter().fold((0, 0), |(a, d), s| match &s.body {
                Body::Ready(l) => (a + l.data.additions, d + l.data.deletions),
                _ => (
                    a + s.file.additions.unwrap_or(0),
                    d + s.file.deletions.unwrap_or(0),
                ),
            });
            let n = self.slots.len();
            let reviewed = self.slots.iter().filter(|s| s.file.viewed).count();
            let staged = self
                .slots
                .iter()
                .all(|s| s.file.source.compare == Compare::Staged);
            h_flex()
                .h(px(36.))
                .flex_none()
                .gap_2()
                .pl_3()
                .pr_2()
                .border_b_1()
                .border_color(c.border)
                .text_size(style.ui_font_size)
                .child(div().min_w_0().truncate().child(title))
                .child(div().flex_none().text_color(c.muted).child(if n == 1 {
                    "1 file".to_string()
                } else {
                    format!("{n} files")
                }))
                .child(paint::line_counts(add, del, &style))
                .when(!staged && n > 0, |el| {
                    el.child(
                        div()
                            .flex_none()
                            .text_color(c.muted)
                            .child(format!("{reviewed}/{n} reviewed")),
                    )
                })
                .child(div().flex_1())
                .child(
                    Button::new("expand-all")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::ChevronsUpDown)
                        .tooltip("Expand all files")
                        .on_click(cx.listener(|this, _, _, cx| this.set_all_open(true, cx))),
                )
                .child(
                    Button::new("collapse-all")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::ChevronsDownUp)
                        .tooltip("Collapse all files")
                        .on_click(cx.listener(|this, _, _, cx| this.set_all_open(false, cx))),
                )
        });
        v_flex()
            .id("stacked-diff")
            .key_context(STACKED_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .bg(style.colors.background)
            .text_color(style.colors.foreground)
            .children(bar)
            .when(empty, |el| {
                el.child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(style.colors.muted)
                        .child(SharedString::from("No changes.")),
                )
            })
            .when(!empty, |el| el.child(body))
            .on_action(cx.listener(|this, _: &NextFile, _, cx| this.step(true, cx)))
            .on_action(cx.listener(|this, _: &PrevFile, _, cx| this.step(false, cx)))
            .on_action(cx.listener(|this, _: &ToggleViewed, _, cx| this.toggle_top_viewed(cx)))
            .on_action(cx.listener(|this, _: &AddNote, _, cx| this.add_note(cx)))
    }
}
