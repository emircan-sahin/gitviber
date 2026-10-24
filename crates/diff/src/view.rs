//! One file's diff: a header, then its lines unified or split, only those on screen laid out.
//! Built from a `DiffSource`; reads both sides through core off the main thread, and again on
//! `refresh` without moving what's at the top of the view.

use crate::actions::*;
use crate::data::DiffData;
use crate::edit::{DiffEditor, EditEvent};
use crate::highlight::{self, Highlighted};
use crate::image::{ImageCompare, image_format};
use crate::list::{Heights, Place, Scroll, vlist};
use crate::model::display::{self, CONTEXT as CONTEXT_LINES, FOLD_MIN, Item};
use crate::model::staging::{self, Change, Picked};
use crate::model::{ADDED, LinePos, LineRange, REMOVED, Row, Side, SideText};
use crate::paint::{self, Metrics};
use crate::source::{Compare, DiffSource};
use crate::style::{DiffOptions, DiffStyle, ImageMode, Layout, Whitespace};
use gitviber_core::Core;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::input::Rope;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::highlighter::{HighlightTheme, SyntaxHighlighter};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    StyledText, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

/// What a diff tells its owner.
#[derive(Clone, Debug)]
pub enum DiffEvent {
    /// The diff loaded, or changed on a refresh.
    Loaded { additions: u32, deletions: u32 },
    /// Lines picked with the mouse or keys; None once cleared.
    SelectionChanged(Option<LineRange>),
    /// C, or a line's note button: a review note on these lines.
    AddNote(LineRange),
    /// The header's Viewed button, or V.
    ToggleViewed,
    /// The header's Open File: the working tree's file.
    OpenFile(String),
    /// Lines were staged, unstaged, discarded, stashed or reverted.
    LinesChanged,
    /// The edited new side was saved.
    Saved(String),
    /// Something to tell the user: a failed stage, a copied patch.
    Notice(Notice),
}

#[derive(Clone, Debug)]
pub struct Notice {
    pub level: NoticeLevel,
    pub title: String,
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeLevel {
    Info,
    Success,
    Error,
}

/// What a staging action does with the lines it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineAction {
    Stage,
    Unstage,
    Discard,
    Stash,
    Revert,
}

impl LineAction {
    /// The diff the action works on.
    fn fits(self, compare: &Compare) -> bool {
        match self {
            LineAction::Stage | LineAction::Discard | LineAction::Stash => {
                *compare == Compare::Unstaged
            }
            LineAction::Unstage => *compare == Compare::Staged,
            LineAction::Revert => matches!(compare, Compare::Commit { .. }),
        }
    }

    fn label(self) -> &'static str {
        match self {
            LineAction::Stage => "Stage",
            LineAction::Unstage => "Unstage",
            LineAction::Discard => "Discard",
            LineAction::Stash => "Stash",
            LineAction::Revert => "Revert",
        }
    }

    fn failed(self) -> String {
        format!("{} failed", self.label())
    }

    /// The hover bar's buttons over a change, in order.
    fn for_compare(compare: &Compare) -> &'static [LineAction] {
        match compare {
            Compare::Unstaged => &[LineAction::Stage, LineAction::Stash, LineAction::Discard],
            Compare::Staged => &[LineAction::Unstage],
            _ => &[],
        }
    }
}

/// Where a selection's rows are: unified rows, or one column of split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
    Both,
    Left,
    Right,
}

/// Picked rows by their item index; one row alone is the cursor, which acts on its change.
#[derive(Clone, Copy, Debug)]
struct Sel {
    column: Column,
    anchor: usize,
    head: usize,
}

impl Sel {
    fn items(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head) + 1
    }

    fn is_cursor(&self) -> bool {
        self.anchor == self.head
    }
}

enum State {
    Loading,
    Ready,
    Failed(String),
}

/// What was at the top of the view: a line, and how far into its row.
struct Anchor {
    pos: LinePos,
    offset: Pixels,
}

pub struct DiffView {
    core: Arc<Core>,
    source: DiffSource,
    focus: FocusHandle,
    state: State,
    data: Option<DiffData>,
    hl: [Option<Highlighted>; 2],
    seq: u64,
    options: DiffOptions,
    items: Vec<Item>,
    heights: Rc<Heights>,
    line: Pixels,
    scroll: Scroll,
    /// Unchanged lines (new line numbers) opened from their fold.
    revealed: HashSet<u32>,
    changes: Vec<Change>,
    sel: Option<Sel>,
    dragging: bool,
    /// The change under the pointer, for its buttons.
    hover: Option<usize>,
    hover_item: Option<usize>,
    busy: bool,
    viewed: Option<bool>,
    header: bool,
    notes: bool,
    /// A changed image shows its two versions instead of lines.
    image: Option<Entity<ImageCompare>>,
    /// The new side being typed into (an unstaged diff's file on disk).
    edit: Option<Entity<DiffEditor>>,
    /// The file changed under the editor: shown again at the next frame.
    edit_stale: bool,
    _edit: Option<Subscription>,
    _load: Task<()>,
    _highlight: Task<()>,
    _action: Task<()>,
}

impl EventEmitter<DiffEvent> for DiffView {}

impl Focusable for DiffView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DiffView {
    /// Starts reading `source` at once; the view draws when it's in.
    pub fn new(source: DiffSource, cx: &mut Context<Self>) -> Self {
        let mut view = DiffView::new_waiting(source, cx);
        view.load(cx);
        view
    }

    /// A diff already read (a prefetch, two texts of `DiffData::from_texts`): drawn at once.
    pub fn with_data(source: DiffSource, data: DiffData, cx: &mut Context<Self>) -> Self {
        let mut view = DiffView::new_waiting(source, cx);
        view.seq += 1;
        let seq = view.seq;
        view.loaded(seq, Ok(data), cx);
        view
    }

    fn new_waiting(source: DiffSource, cx: &mut Context<Self>) -> Self {
        let image = image_format(&source.path).map(|_| {
            let source = source.clone();
            cx.new(|cx| ImageCompare::new(source, cx))
        });
        DiffView {
            core: crate::core(cx),
            source,
            focus: cx.focus_handle(),
            state: State::Loading,
            data: None,
            hl: [None, None],
            seq: 0,
            options: DiffOptions::get(cx),
            items: vec![],
            heights: Rc::default(),
            line: DiffStyle::get(cx).line_px(),
            scroll: Scroll::default(),
            revealed: HashSet::new(),
            changes: vec![],
            sel: None,
            dragging: false,
            hover: None,
            hover_item: None,
            busy: false,
            viewed: None,
            header: true,
            notes: false,
            image,
            edit: None,
            edit_stale: false,
            _edit: None,
            _load: Task::ready(()),
            _highlight: Task::ready(()),
            _action: Task::ready(()),
        }
    }

    pub fn source(&self) -> &DiffSource {
        &self.source
    }

    /// Shows another file in this view; nothing of the last one stays.
    pub fn set_source(&mut self, source: DiffSource, cx: &mut Context<Self>) {
        if source == self.source {
            return;
        }
        self.image = image_format(&source.path).map(|_| {
            let source = source.clone();
            cx.new(|cx| ImageCompare::new(source, cx))
        });
        self.source = source;
        self.edit = None;
        self._edit = None;
        self.data = None;
        self.hl = [None, None];
        self.revealed.clear();
        self.sel = None;
        self.state = State::Loading;
        self.scroll.set_top(px(0.));
        self.rebuild(None, cx);
        self.load(cx);
    }

    /// Reads the file again, keeping the line at the top where it is.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
    }

    /// The repo changed on disk: a diff of the working tree or the index reads again.
    pub fn repo_changed(&mut self, cx: &mut Context<Self>) {
        if !self.source.compare.is_fixed() {
            self.refresh(cx);
        }
    }

    /// Shows the Viewed button, pressed or not; None hides it.
    pub fn set_viewed(&mut self, viewed: Option<bool>, cx: &mut Context<Self>) {
        self.viewed = viewed;
        cx.notify();
    }

    pub fn set_header(&mut self, header: bool, cx: &mut Context<Self>) {
        self.header = header;
        cx.notify();
    }

    /// Lines take review notes: C and the line buttons emit `DiffEvent::AddNote`.
    pub fn set_notes(&mut self, notes: bool, cx: &mut Context<Self>) {
        self.notes = notes;
        cx.notify();
    }

    /// Lines added and removed, once loaded.
    pub fn counts(&self) -> Option<(u32, u32)> {
        self.data.as_ref().map(|d| (d.additions, d.deletions))
    }

    pub fn is_loaded(&self) -> bool {
        self.data.is_some()
    }

    /// Its lines were laid out at least once (tests).
    #[doc(hidden)]
    pub fn is_drawn(&self) -> bool {
        self.scroll.viewport().size.height > px(0.)
    }

    /// The picked lines, on the side they're on (unified: the new side when any are on it).
    pub fn selection(&self) -> Option<LineRange> {
        let sel = self.sel?;
        let data = self.data.as_ref()?;
        let mut old = (u32::MAX, 0);
        let mut new = (u32::MAX, 0);
        for ix in sel.items() {
            for (side, r) in self.item_rows(ix, sel.column) {
                let row = &data.rows[r as usize];
                let (slot, line) = match side {
                    Side::Old => (&mut old, row.o),
                    Side::New => (&mut new, row.n),
                };
                if line > 0 {
                    *slot = (slot.0.min(line), slot.1.max(line));
                }
            }
        }
        if new.1 > 0 {
            Some(LineRange {
                side: Side::New,
                start: new.0,
                end: new.1,
            })
        } else if old.1 > 0 {
            Some(LineRange {
                side: Side::Old,
                start: old.0,
                end: old.1,
            })
        } else {
            None
        }
    }

    /// Picks lines `range` of a side, as a drag over them would (a note's lines shown again).
    pub fn select_lines(&mut self, range: LineRange, cx: &mut Context<Self>) {
        let Some(data) = &self.data else { return };
        let column = match (self.options.layout, range.side) {
            (Layout::Unified, _) => Column::Both,
            (Layout::Split, Side::Old) => Column::Left,
            (Layout::Split, Side::New) => Column::Right,
        };
        let hit: Vec<usize> = (0..self.items.len())
            .filter(|&ix| {
                self.item_rows(ix, column).iter().any(|&(side, r)| {
                    let row = &data.rows[r as usize];
                    let line = if side == Side::Old { row.o } else { row.n };
                    side == range.side && line >= range.start && line <= range.end
                })
            })
            .collect();
        if let (Some(&first), Some(&last)) = (hit.first(), hit.last()) {
            self.sel = Some(Sel {
                column,
                anchor: first,
                head: last,
            });
            self.scroll.scroll_to(first, px(0.), Place::Nearest);
            cx.emit(DiffEvent::SelectionChanged(self.selection()));
            cx.notify();
        }
    }

    /// A line's text as the file has it.
    pub fn line_text(&self, pos: LinePos) -> Option<String> {
        let side = self.data.as_ref()?.side(pos.side);
        (pos.line >= 1 && pos.line <= side.lines).then(|| side.line(pos.line))
    }

    /// One side's lines, for anchoring a note: None when that side isn't there.
    pub fn side_text(&self, side: Side) -> Option<&SideText> {
        self.data
            .as_ref()
            .map(|d| d.side(side))
            .filter(|s| s.exists)
    }

    /// Scrolls to a line, opening its fold.
    pub fn scroll_to_line(&mut self, pos: LinePos, cx: &mut Context<Self>) {
        let Some(data) = &self.data else { return };
        let Some(r) = data.rows.iter().position(|r| match pos.side {
            Side::Old => r.o >= pos.line && r.o > 0,
            Side::New => r.n >= pos.line && r.n > 0,
        }) else {
            return;
        };
        if let Some(n) = (data.rows[r].k == 0).then_some(data.rows[r].n) {
            self.revealed.insert(n);
            self.rebuild(self.anchor(), cx);
        }
        if let Some(ix) = self.item_of_row(r as u32) {
            self.scroll.scroll_to(ix, px(0.), Place::Center);
        }
        cx.notify();
    }

    /// The new side can be typed into: an unstaged diff of a text file on disk whose bytes the
    /// editor would write back as they were (UTF-8, LF endings).
    pub fn is_editable(&self) -> bool {
        let Some(d) = &self.data else { return false };
        self.source.compare == Compare::Unstaged
            && self.source.file.submodule.is_none()
            && d.new.exists
            && d.new.textual()
            && !d.new.lossy
            && !d.new.crlf
    }

    pub fn is_editing(&self) -> bool {
        self.edit.is_some()
    }

    /// Into the editor, or back to the diff (not while there's typing to save).
    pub fn toggle_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(edit) = &self.edit {
            if edit.read(cx).is_dirty(cx) {
                self.notice(
                    NoticeLevel::Info,
                    "Unsaved changes",
                    Some("Save them with ⌘S first."),
                    cx,
                );
                return;
            }
            self.edit = None;
            self._edit = None;
            cx.notify();
            return;
        }
        let Some(data) = self.data.as_ref().filter(|_| self.is_editable()) else {
            return;
        };
        let (base, text) = (data.old.text(), data.new.text());
        let path = self.source.path.clone();
        let edit = cx.new(|cx| DiffEditor::new(path, &base, &text, window, cx));
        self._edit = Some(cx.subscribe(&edit, |this, _, e: &EditEvent, cx| match e {
            EditEvent::Saved => {
                cx.emit(DiffEvent::Saved(this.source.path.clone()));
                this.refresh(cx);
            }
            EditEvent::Failed(e) => this.notice(NoticeLevel::Error, "Could not save", Some(e), cx),
        }));
        self.edit = Some(edit);
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        match &self.edit {
            Some(edit) => edit.update(cx, |e, cx| e.save(cx)),
            None => cx.propagate(),
        }
    }

    pub fn next_change(&mut self, cx: &mut Context<Self>) {
        self.step_change(true, cx);
    }

    pub fn prev_change(&mut self, cx: &mut Context<Self>) {
        self.step_change(false, cx);
    }

    // Loading

    fn load(&mut self, cx: &mut Context<Self>) {
        if let Some(image) = &self.image {
            image.update(cx, |i, cx| i.refresh(cx));
            return;
        }
        self.seq += 1;
        let seq = self.seq;
        let (core, source, ws) = (
            self.core.clone(),
            self.source.clone(),
            self.options.whitespace,
        );
        self._load = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { DiffData::load(&core, &source, ws) })
                .await;
            this.update(cx, |this, cx| this.loaded(seq, result, cx))
                .ok();
        });
    }

    fn loaded(&mut self, seq: u64, result: Result<DiffData, String>, cx: &mut Context<Self>) {
        if seq != self.seq {
            return;
        }
        let data = match result {
            Ok(data) => data,
            Err(e) => {
                // A refresh that failed keeps what was shown.
                if self.data.is_none() {
                    self.state = State::Failed(e);
                }
                cx.notify();
                return;
            }
        };
        self.state = State::Ready;
        if self.data.as_ref().is_some_and(|d| d.same_as(&data)) {
            cx.notify();
            return;
        }
        let anchor = self.anchor();
        let new_text = self
            .data
            .as_ref()
            .is_none_or(|d| d.new.rope != data.new.rope);
        let old_text = self
            .data
            .as_ref()
            .is_none_or(|d| d.old.rope != data.old.rope);
        if new_text {
            // Line numbers of the version before: unfolding them here opens other lines.
            self.revealed.clear();
            self.hl[1] = None;
        }
        if old_text {
            self.hl[0] = None;
        }
        self.changes = staging::changes(&data.rows);
        self.edit_stale = self.edit.is_some();
        let counts = (data.additions, data.deletions);
        self.data = Some(data);
        self.sel = None;
        self.hover = None;
        self.rebuild(anchor, cx);
        self.highlight(cx);
        cx.emit(DiffEvent::Loaded {
            additions: counts.0,
            deletions: counts.1,
        });
        cx.notify();
    }

    /// What `highlight` parses: the sides not parsed yet, and their language.
    fn to_parse(&self) -> Option<([Option<Rope>; 2], String)> {
        let data = self.data.as_ref()?;
        let path = if data.new.exists {
            &self.source.path
        } else {
            self.source.original_path()
        };
        let language = self
            .source
            .language
            .clone()
            .unwrap_or_else(|| highlight::language_for(path).to_string());
        let wants = |i: usize, s: &SideText| {
            (self.hl[i].is_none() && s.exists && s.textual()).then(|| s.rope.clone())
        };
        let ropes = [wants(0, &data.old), wants(1, &data.new)];
        ropes
            .iter()
            .any(Option::is_some)
            .then_some((ropes, language))
    }

    fn parsed(
        &mut self,
        parsed: [Option<(SyntaxHighlighter, Rope)>; 2],
        theme: &Arc<HighlightTheme>,
    ) {
        for (i, p) in parsed.into_iter().enumerate() {
            if let Some((h, rope)) = p {
                self.hl[i] = Some(Highlighted::new(h, rope, theme.clone()));
            }
        }
    }

    /// Parses the sides not parsed yet, in the background.
    fn highlight(&mut self, cx: &mut Context<Self>) {
        let Some((ropes, language)) = self.to_parse() else {
            return;
        };
        let theme = DiffStyle::get(cx).syntax.clone();
        let seq = self.seq;
        self._highlight = cx.spawn(async move |this, cx| {
            let parsed = cx
                .background_spawn(async move { parse_all(ropes, &language) })
                .await;
            this.update(cx, |this, cx| {
                if this.seq == seq {
                    this.parsed(parsed, &theme);
                    cx.notify();
                }
            })
            .ok();
        });
    }

    /// Parses now, on this thread: for a demo window that must be whole at its first frame.
    #[doc(hidden)]
    pub fn highlight_blocking(&mut self, cx: &mut Context<Self>) {
        let Some((ropes, language)) = self.to_parse() else {
            return;
        };
        let theme = DiffStyle::get(cx).syntax.clone();
        self._highlight = Task::ready(());
        self.parsed(parse_all(ropes, &language), &theme);
    }

    // What shows

    fn rebuild(&mut self, anchor: Option<Anchor>, cx: &mut Context<Self>) {
        self.line = DiffStyle::get(cx).line_px();
        let Some(data) = &self.data else {
            self.items.clear();
            self.heights = Rc::default();
            return;
        };
        let shown = if self.options.collapse {
            display::shown_rows(&data.rows, CONTEXT_LINES, FOLD_MIN, &self.revealed)
        } else {
            display::all_rows(&data.rows)
        };
        self.items = match self.options.layout {
            Layout::Unified => display::unified(&shown),
            Layout::Split => display::split(&data.rows, &shown),
        };
        self.heights = Rc::new(Heights::uniform(self.items.len(), self.line));
        self.sel = None;
        if let Some(a) = anchor
            && let Some(ix) = self.item_at_pos(a.pos)
        {
            self.scroll.scroll_to(ix, a.offset, Place::Top);
        }
    }

    fn anchor(&self) -> Option<Anchor> {
        let data = self.data.as_ref()?;
        if self.items.is_empty() {
            return None;
        }
        let top = self.scroll.top();
        let ix = self.heights.index_at(top);
        let r = item_first_row(&self.items[ix]) as usize;
        let row = data.rows.get(r)?;
        let pos = if row.n > 0 {
            LinePos {
                side: Side::New,
                line: row.n,
            }
        } else {
            LinePos {
                side: Side::Old,
                line: row.o,
            }
        };
        Some(Anchor {
            pos,
            offset: top - self.heights.top(ix),
        })
    }

    /// The item showing the first row at or after `pos`.
    fn item_at_pos(&self, pos: LinePos) -> Option<usize> {
        let data = self.data.as_ref()?;
        let r = data.rows.iter().position(|r| match pos.side {
            Side::New => r.n >= pos.line,
            Side::Old => r.o >= pos.line && r.o > 0,
        })?;
        self.item_of_row(r as u32)
    }

    fn item_of_row(&self, r: u32) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }
        Some(
            self.items
                .partition_point(|it| item_first_row(it) <= r)
                .saturating_sub(1),
        )
    }

    /// The rows an item shows in `column`, with their sides.
    fn item_rows(&self, ix: usize, column: Column) -> Vec<(Side, u32)> {
        let Some(data) = &self.data else {
            return vec![];
        };
        match self.items.get(ix) {
            Some(Item::Line(r)) => vec![(
                if data.rows[*r as usize].k == REMOVED {
                    Side::Old
                } else {
                    Side::New
                },
                *r,
            )],
            Some(Item::Pair { old, new }) => {
                let mut out = vec![];
                if column != Column::Right
                    && let Some(o) = old
                {
                    out.push((Side::Old, *o));
                }
                if column != Column::Left
                    && let Some(n) = new
                {
                    out.push((Side::New, *n));
                }
                out
            }
            _ => vec![],
        }
    }

    fn change_of_item(&self, ix: usize) -> Option<usize> {
        let rows: Vec<u32> = match self.items.get(ix)? {
            Item::Line(r) => vec![*r],
            Item::Pair { old, new } => old.iter().chain(new.iter()).copied().collect(),
            Item::Gap { .. } => vec![],
        };
        rows.into_iter()
            .find_map(|r| staging::change_of_row(&self.changes, r))
    }

    fn sync_options(&mut self, cx: &mut Context<Self>) {
        let now = DiffOptions::get(cx);
        if now == self.options {
            if DiffStyle::get(cx).line_px() != self.line {
                let anchor = self.anchor();
                self.rebuild(anchor, cx);
            }
            return;
        }
        let reload = now.whitespace != self.options.whitespace;
        let anchor = self.anchor();
        self.options = now;
        self.rebuild(anchor, cx);
        if reload && self.image.is_none() {
            self.load(cx);
        }
    }

    fn reveal(&mut self, start: u32, count: u32, cx: &mut Context<Self>) {
        let Some(data) = &self.data else { return };
        let lines: Vec<u32> = display::reveal(&data.rows, start, count).collect();
        self.revealed.extend(lines);
        let anchor = self.anchor();
        self.rebuild(anchor, cx);
        cx.notify();
    }

    fn step_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        let firsts: Vec<usize> = self
            .changes
            .iter()
            .filter_map(|c| self.item_of_row(c.rows.start))
            .collect();
        if firsts.is_empty() {
            return;
        }
        let at = self
            .sel
            .map(|s| s.head)
            .unwrap_or_else(|| self.heights.index_at(self.scroll.top()));
        let target = if forward {
            firsts
                .iter()
                .copied()
                .find(|&ix| ix > at)
                .or(firsts.first().copied())
        } else {
            firsts
                .iter()
                .rev()
                .copied()
                .find(|&ix| ix < at)
                .or(firsts.last().copied())
        };
        if let Some(ix) = target {
            let column = if self.options.layout == Layout::Split {
                Column::Right
            } else {
                Column::Both
            };
            self.sel = Some(Sel {
                column,
                anchor: ix,
                head: ix,
            });
            self.scroll.scroll_to(ix, px(0.), Place::Center);
            cx.emit(DiffEvent::SelectionChanged(self.selection()));
            cx.notify();
        }
    }

    // Mouse

    fn press(
        &mut self,
        ix: usize,
        column: Column,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        match self.sel.as_mut() {
            Some(s) if e.modifiers.shift && s.column == column => s.head = ix,
            _ => {
                self.sel = Some(Sel {
                    column,
                    anchor: ix,
                    head: ix,
                })
            }
        }
        self.dragging = true;
        cx.emit(DiffEvent::SelectionChanged(self.selection()));
        cx.notify();
    }

    fn moved(&mut self, ix: usize, column: Column, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let hover = self.change_of_item(ix);
        let mut changed = hover != self.hover || Some(ix) != self.hover_item;
        self.hover = hover;
        self.hover_item = Some(ix);
        if self.dragging
            && e.pressed_button == Some(MouseButton::Left)
            && let Some(s) = self.sel.as_mut()
            && s.column == column
            && s.head != ix
        {
            s.head = ix;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    fn released(&mut self, cx: &mut Context<Self>) {
        if self.dragging {
            self.dragging = false;
            cx.emit(DiffEvent::SelectionChanged(self.selection()));
        }
    }

    fn select_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(s) = self.sel.as_mut() else { return };
        let last = self.items.len().saturating_sub(1);
        s.head = if down {
            (s.head + 1).min(last)
        } else {
            s.head.saturating_sub(1)
        };
        let head = s.head;
        self.scroll.scroll_to(head, px(0.), Place::Nearest);
        cx.emit(DiffEvent::SelectionChanged(self.selection()));
        cx.notify();
    }

    // Line actions

    /// What an action takes: the picked lines, else the change at the cursor or under the pointer.
    fn picked(&self, from_hover: bool) -> Option<Picked> {
        let data = self.data.as_ref()?;
        if from_hover {
            return self
                .hover
                .and_then(|c| self.changes.get(c))
                .map(staging::whole);
        }
        let sel = self.sel?;
        if sel.is_cursor() {
            let c = self.change_of_item(sel.head)?;
            return Some(staging::whole(&self.changes[c]));
        }
        let items = sel.items();
        Some(match sel.column {
            Column::Both => {
                let rows: Vec<u32> = items
                    .clone()
                    .flat_map(|ix| self.item_rows(ix, Column::Both))
                    .map(|(_, r)| r)
                    .collect();
                let (lo, hi) = (rows.iter().min().copied()?, rows.iter().max().copied()?);
                staging::pick_rows(&data.rows, &self.changes, lo..hi + 1)
            }
            column => {
                let side = if column == Column::Left {
                    Side::Old
                } else {
                    Side::New
                };
                let r = self.selection()?;
                let (from, to) = if r.side == side {
                    (r.start, r.end)
                } else {
                    self.column_lines(items, column)?
                };
                staging::pick(&self.changes, side, from, to)
            }
        })
    }

    /// First and last line of a split column's picked items, gaps on that side included.
    fn column_lines(&self, items: Range<usize>, column: Column) -> Option<(u32, u32)> {
        let data = self.data.as_ref()?;
        let lines: Vec<u32> = items
            .flat_map(|ix| self.item_rows(ix, column))
            .map(|(side, r)| {
                if side == Side::Old {
                    data.rows[r as usize].o
                } else {
                    data.rows[r as usize].n
                }
            })
            .filter(|l| *l > 0)
            .collect();
        Some((*lines.iter().min()?, *lines.iter().max()?))
    }

    pub fn line_action(&mut self, action: LineAction, cx: &mut Context<Self>) {
        self.run_action(action, false, cx);
    }

    fn run_action(&mut self, action: LineAction, from_hover: bool, cx: &mut Context<Self>) {
        if self.busy || !action.fits(&self.source.compare) {
            return;
        }
        let Some(picked) = self.picked(from_hover).filter(|p| !p.is_empty()) else {
            self.notice(
                NoticeLevel::Info,
                "No change at the cursor",
                Some("Click into a change, or go to one with Next Change."),
                cx,
            );
            return;
        };
        let Some(data) = &self.data else { return };
        let side = |s: &SideText| s.exists.then(|| s.text());
        let request = gitviber_core::lines::Request {
            path: self.source.path.clone(),
            old_path: self.source.old_path.clone(),
            kind: self.source.compare.kind().to_string(),
            action: match action {
                LineAction::Stage => "stage",
                LineAction::Unstage => "unstage",
                LineAction::Discard => "discard",
                LineAction::Stash => "stash",
                LineAction::Revert => "revert",
            }
            .to_string(),
            original: side(&data.old),
            modified: side(&data.new),
            removed: picked.removed,
            added: picked.added,
        };
        let core = self.core.clone();
        let compare = self.source.compare.clone();
        self.busy = true;
        self.hover = None;
        self._action = cx.spawn(async move |this, cx| {
            let path = request.path.clone();
            let result = cx
                .background_spawn(async move {
                    use gitviber_core::api;
                    match (action, compare) {
                        (LineAction::Stash, _) => {
                            api::stash::stash_lines(&core, String::new(), request)
                        }
                        (LineAction::Revert, Compare::Commit { sha }) => api::patch::revert_lines(
                            &core,
                            gitviber_core::revert::RevertLines {
                                sha,
                                path: request.path,
                                old_path: request.old_path,
                                removed: request.removed,
                                added: request.added,
                            },
                        )
                        .map(|_| ()),
                        _ => api::changes::change_lines(&core, request),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        if action == LineAction::Stash {
                            this.notice(
                                NoticeLevel::Success,
                                &format!("Stashed lines of {path}"),
                                Some("Name it in Stashes."),
                                cx,
                            );
                        }
                        cx.emit(DiffEvent::LinesChanged);
                    }
                    Err(e) => this.notice(NoticeLevel::Error, &action.failed(), Some(&e), cx),
                }
                this.refresh(cx);
            })
            .ok();
        });
    }

    fn notice(
        &mut self,
        level: NoticeLevel,
        title: &str,
        detail: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DiffEvent::Notice(Notice {
            level,
            title: title.to_string(),
            detail: detail.map(str::to_string),
        }));
    }

    /// The picked lines' text, each from the side it's on.
    pub fn selected_text(&self) -> Option<String> {
        let (sel, data) = (self.sel?, self.data.as_ref()?);
        let mut out = String::new();
        for ix in sel.items() {
            for (side, r) in self.item_rows(ix, sel.column) {
                let row = &data.rows[r as usize];
                let line = if side == Side::Old { row.o } else { row.n };
                out.push_str(&data.side(side).line(line));
                out.push('\n');
            }
        }
        Some(out)
    }

    fn copy_lines(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn copy_patch(&mut self, cx: &mut Context<Self>) {
        let (Some(picked), Some(data)) = (self.picked(false).filter(|p| !p.is_empty()), &self.data)
        else {
            return;
        };
        let side = |s: &SideText| s.exists.then(|| s.text());
        let request = gitviber_core::patch::LinesPatch {
            path: self.source.path.clone(),
            old_path: self.source.old_path.clone(),
            original: side(&data.old),
            modified: side(&data.new),
            removed: picked.removed,
            added: picked.added,
        };
        let core = self.core.clone();
        self._action = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(
                    async move { gitviber_core::api::patch::lines_patch(&core, request) },
                )
                .await;
            this.update(cx, |this, cx| match result {
                Ok(patch) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(patch));
                    this.notice(NoticeLevel::Success, "Patch copied", None, cx);
                }
                Err(e) => this.notice(NoticeLevel::Error, "Could not copy the patch", Some(&e), cx),
            })
            .ok();
        });
    }

    fn add_note(&mut self, cx: &mut Context<Self>) {
        if !self.notes {
            cx.propagate();
            return;
        }
        if let Some(range) = self.selection().or_else(|| self.first_change_in_view()) {
            cx.emit(DiffEvent::AddNote(range));
        }
    }

    /// The first changed line on screen, else the first line: where C puts a note with nothing picked.
    fn first_change_in_view(&self) -> Option<LineRange> {
        let data = self.data.as_ref()?;
        let top = self.heights.index_at(self.scroll.top());
        let bottom = self
            .heights
            .index_at(self.scroll.top() + self.scroll.viewport().size.height);
        let rows = (top..=bottom.min(self.items.len().saturating_sub(1)))
            .flat_map(|ix| self.item_rows(ix, Column::Both));
        let mut first = None;
        for (side, r) in rows {
            let row = &data.rows[r as usize];
            let pos = LinePos {
                side,
                line: if side == Side::Old { row.o } else { row.n },
            };
            first.get_or_insert(pos);
            if row.k != 0 {
                return Some(LineRange::single(pos));
            }
        }
        first.map(LineRange::single)
    }
}

fn parse_all(ropes: [Option<Rope>; 2], language: &str) -> [Option<(SyntaxHighlighter, Rope)>; 2] {
    ropes.map(|r| r.and_then(|r| highlight::parse(language, &r).map(|h| (h, r))))
}

fn item_first_row(item: &Item) -> u32 {
    match *item {
        Item::Line(r) => r,
        Item::Pair { old, new } => old.into_iter().chain(new).min().unwrap_or(0),
        Item::Gap { start, .. } => start,
    }
}

impl DiffView {
    fn code(&self, side: Side, row: &Row, style: &DiffStyle) -> StyledText {
        match &self.data {
            Some(data) => paint::code(data, &self.hl, side, row, style),
            None => StyledText::new(""),
        }
    }

    fn render_items(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let style = DiffStyle::get(cx).clone();
        let Some(data) = &self.data else {
            return vec![];
        };
        let m = Metrics::new(
            window,
            &style,
            data.old.lines.max(data.new.lines),
            self.line,
            self.scroll.left(),
        );
        // The hovered change's buttons go on its first row in view.
        let bar_item = self.hover.and_then(|c| {
            let rows = self.changes.get(c)?.rows.clone();
            range.clone().find(|&ix| {
                self.item_rows(ix, Column::Both)
                    .iter()
                    .any(|(_, r)| rows.contains(r))
            })
        });
        let sel = self.sel;
        range
            .map(|ix| {
                let selected = sel.filter(|s| s.items().contains(&ix));
                let el = match self.items[ix] {
                    Item::Line(r) => self.unified_row(ix, r, selected.is_some(), &m, &style, cx),
                    Item::Pair { old, new } => {
                        self.split_row(ix, old, new, selected.map(|s| s.column), &m, &style, cx)
                    }
                    Item::Gap { start, count } => self.gap_row(ix, start, count, &style, cx),
                };
                let el = if bar_item == Some(ix) {
                    el.child(self.hover_bar(&style, cx))
                } else {
                    el
                };
                let el = if sel.is_some_and(|s| !s.is_cursor() && s.head == ix) && !self.dragging {
                    el.child(self.selection_bar(&style, cx))
                } else {
                    el
                };
                el.into_any_element()
            })
            .collect()
    }

    fn row_shell(
        &self,
        ix: usize,
        column: Column,
        m: &Metrics,
        style: &DiffStyle,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        h_flex()
            .id(("diff-row", ix))
            .relative()
            .w_full()
            .h(m.line)
            .line_height(m.line)
            .font_family(style.font_family.clone())
            .text_size(style.font_size)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.press(ix, column, e, window, cx)
                }),
            )
            .on_mouse_move(
                cx.listener(move |this, e: &MouseMoveEvent, _, cx| this.moved(ix, column, e, cx)),
            )
    }

    fn unified_row(
        &self,
        ix: usize,
        r: u32,
        selected: bool,
        m: &Metrics,
        style: &DiffStyle,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let data = self.data.as_ref().expect("rows without data");
        let row = &data.rows[r as usize];
        let t = paint::tone(row.k, style);
        let c = &style.colors;
        let num = if row.k == 0 { c.subtle } else { c.muted };
        let side = if row.k == REMOVED {
            Side::Old
        } else {
            Side::New
        };
        let note = self.notes && self.hover_item == Some(ix);
        self.row_shell(ix, Column::Both, m, style, cx)
            .bg(t.line)
            .child(paint::number(
                (row.k != ADDED).then_some(row.o),
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
            .child(paint::code_cell(self.code(side, row, style), m))
            .when(selected, |el| el.child(paint::overlay(c.selection)))
            .when(note, |el| el.child(self.note_button(ix, style, cx)))
    }

    #[allow(clippy::too_many_arguments)]
    fn split_row(
        &self,
        ix: usize,
        old: Option<u32>,
        new: Option<u32>,
        selected: Option<Column>,
        m: &Metrics,
        style: &DiffStyle,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let data = self.data.as_ref().expect("rows without data");
        let c = &style.colors;
        let half = |this: &Self,
                    r: Option<u32>,
                    side: Side,
                    cx: &mut Context<Self>|
         -> gpui_kit::Stateful<gpui_kit::Div> {
            let column = if side == Side::Old {
                Column::Left
            } else {
                Column::Right
            };
            let el = h_flex()
                .id(("diff-half", ix * 2 + side as usize))
                .relative()
                .flex_1()
                .min_w_0()
                .h_full()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        this.press(ix, column, e, window, cx)
                    }),
                )
                .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                    this.moved(ix, column, e, cx)
                }));
            let Some(r) = r else { return el.bg(c.panel) };
            let row = &data.rows[r as usize];
            let t = paint::tone(row.k, style);
            let n = if side == Side::Old { row.o } else { row.n };
            let num = if row.k == 0 { c.subtle } else { c.muted };
            el.bg(t.line)
                .child(paint::number(Some(n), t.gutter, num, m))
                .child(paint::sign(&t, style, m))
                .child(paint::code_cell(this.code(side, row, style), m))
                .when(selected == Some(column), |el| {
                    el.child(paint::overlay(c.selection))
                })
        };
        let left = half(self, old, Side::Old, cx);
        let right = half(self, new, Side::New, cx)
            .border_l_1()
            .border_color(c.border);
        let note = self.notes && self.hover_item == Some(ix);
        h_flex()
            .id(("diff-row", ix))
            .relative()
            .w_full()
            .h(m.line)
            .line_height(m.line)
            .font_family(style.font_family.clone())
            .text_size(style.font_size)
            .child(left)
            .child(right)
            .when(note, |el| el.child(self.note_button(ix, style, cx)))
    }

    fn gap_row(
        &self,
        ix: usize,
        start: u32,
        count: u32,
        style: &DiffStyle,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let c = &style.colors;
        let more = count.min(display::FOLD_REVEAL);
        let label = paint::gap_label(count);
        h_flex()
            .id(("diff-gap", ix))
            .relative()
            .w_full()
            .h(self.line)
            .gap_1p5()
            .px_3()
            .bg(c.panel)
            .text_size(style.ui_font_size - px(1.))
            .text_color(c.subtle)
            .cursor_pointer()
            .hover(|s| s.bg(c.elevated).text_color(c.foreground))
            .child(Icon::new(Lucide::ChevronsUpDown).size_3())
            .child(label)
            .when(count > more, |el| {
                el.child(
                    div()
                        .text_color(c.subtle)
                        .child(format!("· show {more} more")),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| this.reveal(start, count, cx)))
    }

    /// Stage, stash and discard over the change under the pointer.
    fn hover_bar(&self, style: &DiffStyle, cx: &mut Context<Self>) -> AnyElement {
        let c = &style.colors;
        let actions = LineAction::for_compare(&self.source.compare);
        if actions.is_empty() || self.busy {
            return div().into_any_element();
        }
        h_flex()
            .absolute()
            .top_0()
            .right(px(14.))
            .h_full()
            .gap_1()
            .children(actions.iter().map(|&a| {
                let destructive = a == LineAction::Discard;
                div()
                    .id(SharedString::from(format!("hunk-{}", a.label())))
                    .px_1p5()
                    .rounded_sm()
                    .text_size(style.ui_font_size - px(1.))
                    .line_height(self.line - px(4.))
                    .bg(if destructive {
                        c.destructive
                    } else {
                        c.primary
                    })
                    .text_color(c.primary_foreground)
                    .cursor_pointer()
                    .child(a.label())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| this.run_action(a, true, cx)))
            }))
            .into_any_element()
    }

    /// The picked lines' actions, at the end of the selection.
    fn selection_bar(&self, style: &DiffStyle, cx: &mut Context<Self>) -> AnyElement {
        let c = &style.colors;
        let mut actions: Vec<(&'static str, Option<LineAction>)> =
            LineAction::for_compare(&self.source.compare)
                .iter()
                .map(|a| (a.label(), Some(*a)))
                .collect();
        if matches!(self.source.compare, Compare::Commit { .. }) {
            actions.push(("Revert", Some(LineAction::Revert)));
        }
        actions.push(("Copy", None));
        h_flex()
            .absolute()
            .top_0()
            .right(px(14.))
            .h_full()
            .gap_1()
            .children(actions.into_iter().map(|(label, a)| {
                div()
                    .id(SharedString::from(format!("sel-{label}")))
                    .px_1p5()
                    .rounded_sm()
                    .text_size(style.ui_font_size - px(1.))
                    .line_height(self.line - px(4.))
                    .bg(c.elevated)
                    .border_1()
                    .border_color(c.border)
                    .text_color(if a == Some(LineAction::Discard) {
                        c.destructive
                    } else {
                        c.foreground
                    })
                    .cursor_pointer()
                    .child(format!("{label} lines"))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| match a {
                        Some(a) => this.run_action(a, false, cx),
                        None => this.copy_lines(cx),
                    }))
            }))
            .into_any_element()
    }

    fn note_button(&self, ix: usize, style: &DiffStyle, cx: &mut Context<Self>) -> AnyElement {
        let c = &style.colors;
        div()
            .id(("note", ix))
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
                let picked = this
                    .sel
                    .is_some_and(|s| s.items().contains(&ix) && !s.is_cursor());
                let range = if picked {
                    this.selection()
                } else {
                    this.item_rows(ix, Column::Both)
                        .last()
                        .and_then(|(side, r)| this.pos_of(*side, *r))
                        .map(LineRange::single)
                };
                if let Some(range) = range {
                    cx.emit(DiffEvent::AddNote(range));
                }
            }))
            .into_any_element()
    }

    fn pos_of(&self, side: Side, r: u32) -> Option<LinePos> {
        let row = self.data.as_ref()?.rows.get(r as usize)?;
        Some(LinePos {
            side,
            line: if side == Side::Old { row.o } else { row.n },
        })
    }

    fn render_header(&self, style: &DiffStyle, cx: &mut Context<Self>) -> AnyElement {
        let c = &style.colors;
        let o = self.options;
        let counts = self.counts();
        let status = self.source.file.status.clone();
        let note = self.data.as_ref().and_then(|d| d.note.clone());
        let open_file =
            self.source.compare.new_side_on_disk() || self.source.compare == Compare::Staged;
        let text_diff =
            self.image.is_none() && self.data.as_ref().is_some_and(|d| d.placeholder.is_none());
        let image = self.image.is_some();
        h_flex()
            .h(px(34.))
            .flex_none()
            .w_full()
            .gap_2()
            .pl_3()
            .pr_2()
            .border_b_1()
            .border_color(c.border)
            .bg(c.background)
            .text_size(style.ui_font_size)
            .text_color(c.foreground)
            .child(paint::path_label(&self.source.path, style))
            .when_some(self.source.old_path.clone(), |el, old| {
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
            .when(!status.is_empty(), |el| {
                el.child(paint::status_pill(&status, style))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(c.subtle)
                    .truncate()
                    .children(note),
            )
            .when(text_diff, |el| {
                el.child(
                    icon_button(
                        "prev-change",
                        Lucide::ArrowUp,
                        "Previous change (⇧F7)",
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.prev_change(cx))),
                )
                .child(
                    icon_button("next-change", Lucide::ArrowDown, "Next change (F7)", false)
                        .on_click(cx.listener(|this, _, _, cx| this.next_change(cx))),
                )
                .child(
                    Button::new("unified")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Rows2)
                        .label("Unified")
                        .toggled(o.layout == Layout::Unified)
                        .on_click(cx.listener(|_, _, _, cx| {
                            DiffOptions::update(cx, |o| o.layout = Layout::Unified)
                        })),
                )
                .child(
                    Button::new("split")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Columns2)
                        .label("Split")
                        .toggled(o.layout == Layout::Split)
                        .tooltip("Unified / split (⌥S)")
                        .on_click(cx.listener(|_, _, _, cx| {
                            DiffOptions::update(cx, |o| o.layout = Layout::Split)
                        })),
                )
                .child(
                    icon_button(
                        "collapse",
                        Lucide::FoldVertical,
                        "Collapse unchanged lines (⌥C)",
                        o.collapse,
                    )
                    .on_click(cx.listener(|_, _, _, cx| {
                        DiffOptions::update(cx, |o| o.collapse = !o.collapse)
                    })),
                )
                .child(
                    icon_button(
                        "whitespace",
                        Lucide::Space,
                        "Ignore whitespace changes",
                        o.whitespace != Whitespace::Exact,
                    )
                    .on_click(cx.listener(|_, _, _, cx| {
                        DiffOptions::update(cx, |o| {
                            o.whitespace = if o.whitespace == Whitespace::Exact {
                                Whitespace::Amount
                            } else {
                                Whitespace::Exact
                            }
                        })
                    })),
                )
            })
            .when(self.is_editable(), |el| {
                el.child(
                    Button::new("edit")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Pencil)
                        .label("Edit")
                        .toggled(self.edit.is_some())
                        .tooltip("Edit the new side; ⌘S saves")
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_edit(window, cx))),
                )
            })
            .when(image, |el| {
                el.children(
                    [
                        (ImageMode::Side, "2-up"),
                        (ImageMode::Swipe, "Swipe"),
                        (ImageMode::Onion, "Onion skin"),
                    ]
                    .map(|(mode, label)| {
                        Button::new(label)
                            .ghost()
                            .xsmall()
                            .label(label)
                            .toggled(o.image == mode)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                DiffOptions::update(cx, |o| o.image = mode)
                            }))
                    }),
                )
            })
            .when(open_file && self.source.file.status != "D", |el| {
                el.child(
                    Button::new("open-file")
                        .xsmall()
                        .icon(Lucide::FileCode)
                        .label("Open file")
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.emit(DiffEvent::OpenFile(this.source.path.clone()))
                        })),
                )
            })
            .when_some(self.viewed, |el, viewed| {
                el.child(
                    Button::new("viewed")
                        .xsmall()
                        .icon(Lucide::Check)
                        .label("Viewed")
                        .toggled(viewed)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(DiffEvent::ToggleViewed))),
                )
            })
            .into_any_element()
    }

    fn render_message(&self, text: String, style: &DiffStyle) -> AnyElement {
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .text_size(style.ui_font_size + px(0.5))
            .text_color(style.colors.muted)
            .child(text)
            .into_any_element()
    }
}

fn icon_button(id: &'static str, icon: Lucide, tip: &'static str, on: bool) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(icon)
        .tooltip(tip)
        .toggled(on)
}

impl Render for DiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_options(cx);
        if std::mem::take(&mut self.edit_stale)
            && let (Some(edit), Some(data)) = (&self.edit, &self.data)
        {
            let (base, text) = (data.old.text(), data.new.text());
            edit.update(cx, |e, cx| e.file_changed(&base, &text, window, cx));
        }
        let style = DiffStyle::get(cx).clone();
        for h in self.hl.iter_mut().flatten() {
            if !Arc::ptr_eq(h.theme(), &style.syntax) {
                h.set_theme(style.syntax.clone());
            }
        }
        // How far the code scrolls sideways: its widest line past the room it has.
        if let Some(data) = &self.data {
            let m = Metrics::new(
                window,
                &style,
                data.old.lines.max(data.new.lines),
                self.line,
                px(0.),
            );
            let (columns, numbers) = if self.options.layout == Layout::Split {
                (2., 1.)
            } else {
                (1., 2.)
            };
            self.scroll.set_max_left(m.max_left(
                data.widest,
                self.scroll.viewport().size.width,
                columns,
                numbers,
            ));
        }
        let body: AnyElement = match (&self.state, &self.data) {
            _ if self.edit.is_some() => self
                .edit
                .clone()
                .map(|e| div().flex_1().min_h_0().child(e).into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            _ if self.image.is_some() => self
                .image
                .clone()
                .map(|i| div().flex_1().min_h_0().child(i).into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            (State::Failed(e), None) => self.render_message(format!("Could not load: {e}"), &style),
            (_, None) => div().flex_1().into_any_element(),
            (_, Some(data)) if data.placeholder.is_some() => {
                self.render_message(data.placeholder.clone().unwrap_or_default(), &style)
            }
            (_, Some(_)) => {
                let view = cx.entity().downgrade();
                div()
                    .id("diff-body")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .bg(style.colors.background)
                    .child(vlist(
                        "diff-rows",
                        &self.scroll,
                        self.heights.clone(),
                        cx.entity_id(),
                        move |range, window, cx| {
                            view.update(cx, |this, cx| this.render_items(range, window, cx))
                                .unwrap_or_default()
                        },
                    ))
                    .child(Scrollbar::vertical(&self.scroll))
                    .child(Scrollbar::horizontal(&self.scroll))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.released(cx)),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.released(cx)),
                    )
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !hovered && (this.hover.is_some() || this.hover_item.is_some()) {
                            this.hover = None;
                            this.hover_item = None;
                            cx.notify();
                        }
                    }))
                    .into_any_element()
            }
        };
        v_flex()
            .id("diff-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .bg(style.colors.background)
            .text_color(style.colors.foreground)
            .when(self.header, |el| el.child(self.render_header(&style, cx)))
            .child(body)
            .on_action(cx.listener(|this, _: &NextChange, _, cx| this.next_change(cx)))
            .on_action(cx.listener(|this, _: &PrevChange, _, cx| this.prev_change(cx)))
            .on_action(cx.listener(|_, _: &ToggleSplit, _, cx| {
                DiffOptions::update(cx, |o| {
                    o.layout = if o.layout == Layout::Split {
                        Layout::Unified
                    } else {
                        Layout::Split
                    }
                })
            }))
            .on_action(cx.listener(|_, _: &ToggleCollapse, _, cx| {
                DiffOptions::update(cx, |o| o.collapse = !o.collapse)
            }))
            .on_action(cx.listener(|_, _: &ToggleWhitespace, _, cx| {
                DiffOptions::update(cx, |o| {
                    o.whitespace = if o.whitespace == Whitespace::Exact {
                        Whitespace::Amount
                    } else {
                        Whitespace::Exact
                    }
                })
            }))
            .on_action(cx.listener(|this, _: &StageLines, _, cx| {
                this.run_action(LineAction::Stage, false, cx)
            }))
            .on_action(cx.listener(|this, _: &UnstageLines, _, cx| {
                this.run_action(LineAction::Unstage, false, cx)
            }))
            .on_action(cx.listener(|this, _: &DiscardLines, _, cx| {
                this.run_action(LineAction::Discard, false, cx)
            }))
            .on_action(cx.listener(|this, _: &StashLines, _, cx| {
                this.run_action(LineAction::Stash, false, cx)
            }))
            .on_action(cx.listener(|this, _: &RevertLines, _, cx| {
                this.run_action(LineAction::Revert, false, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyLines, _, cx| this.copy_lines(cx)))
            .on_action(cx.listener(|this, _: &CopyPatch, _, cx| this.copy_patch(cx)))
            .on_action(cx.listener(|this, _: &AddNote, _, cx| this.add_note(cx)))
            .on_action(cx.listener(|this, _: &ToggleViewed, _, cx| {
                if this.viewed.is_some() {
                    cx.emit(DiffEvent::ToggleViewed)
                } else {
                    cx.propagate()
                }
            }))
            .on_action(cx.listener(|this, _: &ClearSelection, _, cx| {
                if this.sel.take().is_some() {
                    cx.emit(DiffEvent::SelectionChanged(None));
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save(cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.select_step(false, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.select_step(true, cx)))
    }
}
