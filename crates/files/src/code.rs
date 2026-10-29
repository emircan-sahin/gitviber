//! A file's text in gpui-component's code editor: tree-sitter colors, line numbers, find (⌘F),
//! go to line (⌃G), Go to Definition / References through core, the change bars against HEAD in
//! the gutter and the whole file's on the scrollbar (with find's matches), blocks of the host's
//! between lines (review notes), and ⌘S writing it back with its own line endings, asking first
//! when the file changed on disk since the typing began. Read-only for what can't be written back
//! as it was (outside the repo, lossy text, mixed line endings). The editing commands (edit.rs)
//! and the bracket at the caret are the editable view's.

use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gitviber_core::definitions::{self, Location};
use gitviber_core::{Core, api};
/// The widening rules, for the vault's editor too.
pub use gitviber_diff::model::indent;
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::{self, editor, file, find, review, search};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::base::input::{
    LineBlock, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle, SyntaxContext,
};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{
    DefinitionProvider, Editor, EditorState, Input, InputEvent, InputState, Position, Rope,
    RopeExt as _, TabSize,
};
use gpui_kit::component::{input as edit, native_menu::NativeMenu};
use gpui_kit::{
    App, AppContext as _, Bounds, ClickEvent, ClipboardItem, Context, Entity, FocusHandle,
    Focusable as _, InteractiveElement, IntoElement, KeyBinding, ParentElement as _, Pixels,
    PromptLevel, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Task, WeakEntity, Window, actions, canvas, div, fill, point, prelude::FluentBuilder as _, px,
    relative, size,
};

use crate::backend;
use crate::edit::{self as lines, LineEdit};
use crate::text::{self, Bar, BarKind};
use crate::{OpenOptions, Reveal, open_file};

actions!(
    code_view,
    [
        GoToLine,
        CloseOverlay,
        ToggleComment,
        JumpToBracket,
        SelectNextOccurrence,
        SelectLine,
        DeleteLine,
        MoveLineUp,
        MoveLineDown,
        CopyLineUp,
        CopyLineDown,
        InsertLineBelow,
        InsertLineAbove,
        ScrollPageDown,
        ScrollPageUp,
    ]
);

/// Past this the text isn't parsed for colors or folds: a minified bundle costs more than they help.
pub const MAX_HIGHLIGHT_BYTES: usize = 2_000_000;
/// Past this the bars aren't worked out again while typing.
const MAX_LIVE_BARS_BYTES: usize = 1_500_000;
const SETTLE: Duration = Duration::from_millis(150);

pub(crate) fn bind_keys(cx: &mut App) {
    // The code's own field, not the find or go-to box over it.
    let code = Some("CodeView > Input && mode == editor");
    let k = |mac: &'static str, other: &'static str| {
        if cfg!(target_os = "macos") {
            mac
        } else {
            other
        }
    };
    cx.bind_keys([
        KeyBinding::new("ctrl-g", GoToLine, Some("CodeView")),
        KeyBinding::new("escape", CloseOverlay, Some("CodeOverlay")),
        // The old editor's keys (Monaco's). ⌘/ goes on to the shortcut overlay where the file
        // can't be commented; ⇧⌘\ comes from macOS as ⌘|.
        KeyBinding::new(k("cmd-/", "ctrl-/"), ToggleComment, code),
        KeyBinding::new(k("cmd-|", "ctrl-shift-\\"), JumpToBracket, code),
        KeyBinding::new(k("cmd-d", "ctrl-d"), SelectNextOccurrence, code),
        KeyBinding::new(k("cmd-l", "ctrl-l"), SelectLine, code),
        KeyBinding::new(k("shift-cmd-k", "ctrl-shift-k"), DeleteLine, code),
        KeyBinding::new("alt-up", MoveLineUp, code),
        KeyBinding::new("alt-down", MoveLineDown, code),
        KeyBinding::new("shift-alt-up", CopyLineUp, code),
        KeyBinding::new("shift-alt-down", CopyLineDown, code),
        KeyBinding::new(k("cmd-enter", "ctrl-enter"), InsertLineBelow, code),
        KeyBinding::new(
            k("shift-cmd-enter", "ctrl-shift-enter"),
            InsertLineAbove,
            code,
        ),
        // Read-only code pages as the shortcut overlay says; typed code takes the space.
        KeyBinding::new("space", ScrollPageDown, code),
        KeyBinding::new("shift-space", ScrollPageUp, code),
    ]);
}

/// The text a save compares the disk with, as read or written (line endings and all).
fn text_hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// Puts the selections back, the active one first.
fn select(s: &mut EditorState, selections: &[Range<usize>], cx: &mut Context<EditorState>) {
    let mut each = selections.iter().cloned();
    if let Some(first) = each.next() {
        s.set_selected_range(first, cx);
    }
    for r in each {
        s.add_selection(r, cx);
    }
}

/// The non-empty selections' text in document order, as Copy joins it, with the file's own
/// indentation.
fn selection_text(s: &EditorState, wide: usize) -> Option<String> {
    let mut ranges: Vec<Range<usize>> = s
        .selected_ranges()
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect();
    ranges.sort_by_key(|r| r.start);
    let texts: Vec<String> = ranges
        .into_iter()
        .map(|r| indent::narrow(&s.text().slice(r).to_string(), wide))
        .collect();
    (!texts.is_empty()).then(|| texts.join("\n"))
}

/// Deletes every selection as one step to undo, a caret left at each.
fn delete_selections(s: &mut EditorState, window: &mut Window, cx: &mut Context<EditorState>) {
    let mut ranges = s.selected_ranges();
    ranges.sort_by_key(|r| r.start);
    let (Some(first), Some(last)) = (ranges.first(), ranges.last()) else {
        return;
    };
    let span = first.start..last.end.max(first.end);
    let (mut kept, mut carets, mut at) = (String::new(), vec![], span.start);
    for r in &ranges {
        let start = r.start.max(at);
        kept.push_str(&s.text().slice(at..start).to_string());
        carets.push(span.start + kept.len()..span.start + kept.len());
        at = r.end.max(at);
    }
    s.set_selected_range(span, cx);
    s.replace(kept, window, cx);
    select(s, &carets, cx);
}

/// Copy, Cut and Paste in an editor that shows a file's `wide`-space levels 4 wide (see
/// `indent::widen`): what leaves has the file's own indentation, and what comes in is widened
/// where it lands. A paste at several carets is the editor's own, as it shares out lines.
pub fn indent_clipboard<E: InteractiveElement>(
    el: E,
    state: &Entity<EditorState>,
    wide: usize,
) -> E {
    if wide == 0 {
        return el;
    }
    let (copy, cut, paste) = (state.clone(), state.clone(), state.clone());
    el.capture_action(move |_: &edit::Copy, _, cx| {
        if let Some(text) = selection_text(copy.read(cx), wide) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            cx.stop_propagation();
        }
    })
    .capture_action(move |_: &edit::Cut, window, cx| {
        let s = cut.read(cx);
        let Some(text) = selection_text(s, wide).filter(|_| s.is_editable()) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        cx.stop_propagation();
        cut.update(cx, |s, cx| delete_selections(s, window, cx));
    })
    .capture_action(move |_: &edit::Paste, window, cx| {
        let s = paste.read(cx);
        let ranges = s.selected_ranges();
        let ([range], true) = (ranges.as_slice(), s.is_editable()) else {
            return;
        };
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            return;
        };
        let row = s.text().offset_to_point(range.start).row;
        let before = s.text().slice(s.text().line_start_offset(row)..range.start);
        let widened = indent::widen_paste(&text, &before.to_string(), wide);
        if widened == text {
            return;
        }
        cx.stop_propagation();
        paste.update(cx, |s, cx| {
            s.replace(widened, window, cx);
            // Shown where it ends, as the editor's own paste.
            let at = s.cursor_position();
            s.set_cursor_position(at, window, cx);
        });
    })
}

/// Where the text comes from, for saving and for definitions.
#[derive(Clone, Debug)]
pub struct CodeSource {
    /// Repo-relative (or absolute for a file outside the repo).
    pub path: String,
    /// The repo's root; None outside it.
    pub root: Option<PathBuf>,
}

pub enum CodeEvent {
    Saved,
    DirtyChanged,
    /// The file changed on disk and shows as it is now (not typed into).
    Reloaded,
    /// Add Review Note, on these lines (1-based, inclusive): the selection's, else the caret's.
    AddNote {
        lines: (u32, u32),
    },
}

/// Something drawn between two lines of the code (a review note): under 1-based `line`, as wide
/// as the text and as tall as its content, in whole rows of the line height. It follows its
/// line as the text is typed into; a fold over the line hides it.
#[derive(Clone)]
pub struct CodeBlock {
    /// Stable across updates: its measured height is kept under it.
    pub id: SharedString,
    pub line: u32,
    /// Its height until it's laid out; then what it measures.
    pub height: Pixels,
    pub render: RenderBlock,
}

pub type RenderBlock = Rc<dyn Fn(&mut Window, &mut App) -> gpui_kit::AnyElement>;

impl CodeBlock {
    pub fn new(
        id: impl Into<SharedString>,
        line: u32,
        height: Pixels,
        render: impl Fn(&mut Window, &mut App) -> gpui_kit::AnyElement + 'static,
    ) -> Self {
        CodeBlock {
            id: id.into(),
            line,
            height,
            render: Rc::new(render),
        }
    }
}

/// A list of places (definitions, references) over the code.
struct Places {
    title: SharedString,
    places: Vec<Location>,
}

pub struct CodeView {
    workspace: WeakEntity<Workspace>,
    core: Arc<Core>,
    source: CodeSource,
    state: Entity<EditorState>,
    readonly: bool,
    crlf: bool,
    /// Spaces a level the file is indented with that show 4 wide (`indent::widen`); 0 when it
    /// shows as it is. The provider reads it too.
    wide: Rc<std::cell::Cell<usize>>,
    /// The file's language, highlighted or not (the status bar names it).
    lang: &'static str,
    /// HEAD's text, for the bars once the file is typed into; None: no bars (new, outside).
    head: Option<Arc<String>>,
    bars: Arc<Vec<Bar>>,
    map: Rc<RefCell<crate::code_map::CodeMap>>,
    dirty: bool,
    /// Bumped by each edit, so a save that raced typing leaves the file dirty.
    edits: u64,
    /// The file on disk the typing began from (`text_hash`): a save finding another asks first.
    base: u64,
    /// What a save is writing now, so its own change on disk isn't someone else's.
    saving: Option<u64>,
    /// The file changed on disk under unsaved typing (the header says so).
    disk_changed: bool,
    /// The bracket at the caret and its match, in the editable view.
    brackets: Option<RangeDecorationCollection>,
    /// The caret and edit count they were found for.
    brackets_at: (usize, u64),
    goto: Option<(Entity<InputState>, Subscription)>,
    places: Option<Places>,
    /// Who last changed each line, while Settings asks for it (View → Blame).
    blame: Option<Arc<Blamed>>,
    blame_on: bool,
    /// The right-click menu has Copy GitHub Link / Open on GitHub.
    github: bool,
    /// Review notes are on: Add Review Note is offered.
    notes: bool,
    focus: FocusHandle,
    _change: Subscription,
    _settings: Subscription,
    _caret: Subscription,
    _blame: Task<()>,
    _brackets: Task<()>,
    _bars: Task<()>,
    _save: Task<()>,
    _lookup: Task<()>,
    _live: Live,
}

impl gpui_kit::EventEmitter<CodeEvent> for CodeView {}

/// A file's blame with each commit's label made once.
pub struct Blamed {
    blame: gitviber_core::git::Blame,
    labels: Vec<SharedString>,
}

/// Characters in a label: short SHA, author, age.
const BLAME_CHARS: f32 = 33.;
const AUTHOR_CHARS: usize = 16;

impl Blamed {
    fn new(blame: gitviber_core::git::Blame) -> Self {
        let now = gitviber_ui::widgets::now_unix();
        let labels = blame
            .commits
            .iter()
            .map(|c| {
                if c.sha.bytes().all(|b| b == b'0') {
                    return SharedString::from("Uncommitted");
                }
                let mut author: String = c.author_name.chars().take(AUTHOR_CHARS).collect();
                if c.author_name.chars().count() > AUTHOR_CHARS {
                    author.pop();
                    author.push('…');
                }
                let when = gitviber_ui::widgets::relative_time(c.timestamp, now);
                format!(
                    "{} {author:<AUTHOR_CHARS$} {when}",
                    &c.sha[..c.sha.len().min(7)]
                )
                .into()
            })
            .collect();
        Blamed { blame, labels }
    }

    /// The commit of `line` (0-based); lines git has none for (past its end) are new.
    fn index(&self, line: usize) -> Option<usize> {
        self.blame.lines.get(line).map(|&i| i as usize)
    }

    fn commit(&self, line: usize) -> Option<&gitviber_core::git::BlameCommit> {
        self.blame.commits.get(self.index(line)?)
    }
}

/// The width of the blame column at the code font's size.
fn blame_width(font_size: Pixels) -> Pixels {
    font_size * (0.6 * (BLAME_CHARS + 2.))
}

/// ⌘-click and F12 look names up through core.
struct Definitions {
    core: Arc<Core>,
    path: String,
    root: PathBuf,
    /// The view's widening (`CodeView::wide`), as it was when the file was shown.
    wide: Rc<std::cell::Cell<usize>>,
}

/// The 0-based UTF-16 column of `offset` in its line, as core counts columns.
fn utf16_column(text: &Rope, offset: usize) -> (u32, u32) {
    let p = text.offset_to_point(offset);
    let start = text.line_start_offset(p.row);
    let line: String = text.slice(start..offset).to_string();
    (p.row as u32 + 1, line.encode_utf16().count() as u32)
}

/// Line `row` (0-based) of `text`.
fn line_text(text: &Rope, row: usize) -> String {
    let row = row.min(text.lines_len().saturating_sub(1));
    text.slice(text.line_start_offset(row)..text.line_end_offset(row))
        .to_string()
}

/// A lookup at `offset` of the shown `text`, in the file's own text and columns.
fn request(path: String, text: &Rope, offset: usize, wide: usize) -> definitions::Request {
    let (line, column) = utf16_column(text, offset);
    let shown = line_text(text, line as usize - 1);
    definitions::Request {
        path,
        text: indent::narrow(&text.to_string(), wide),
        line,
        column: indent::narrow_column(&shown, column as usize, wide) as u32,
        rev: None,
    }
}

/// Line `line` (1-based) of `text` as the file has it.
fn file_line(text: &str, line: u32) -> &str {
    text.split('\n').nth(line.max(1) as usize - 1).unwrap_or("")
}

/// A UTF-16 column of `line` (1-based) as a byte offset.
fn offset_of(text: &Rope, line: u32, column: u32) -> usize {
    let row = (line.max(1) as usize - 1).min(text.lines_len().saturating_sub(1));
    let start = text.line_start_offset(row);
    let end = text.line_end_offset(row);
    let s = text.slice(start..end).to_string();
    let mut units = 0;
    for (i, c) in s.char_indices() {
        if units >= column as usize {
            return start + i;
        }
        units += c.len_utf16();
    }
    end
}

fn file_uri(root: &std::path::Path, path: &str) -> Option<lsp_types::Uri> {
    let abs = root.join(path);
    let mut out = String::from("file://");
    for b in abs.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out.parse().ok()
}

fn lsp_range(line: u32, start: u32, end: u32) -> lsp_types::Range {
    lsp_types::Range {
        start: Position::new(line.saturating_sub(1), start),
        end: Position::new(line.saturating_sub(1), end),
    }
}

impl DefinitionProvider for Definitions {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<Vec<lsp_types::LocationLink>>> {
        let (core, path, root, text, wide) = (
            self.core.clone(),
            self.path.clone(),
            self.root.clone(),
            text.clone(),
            self.wide.get(),
        );
        cx.background_spawn(async move {
            let request = request(path, &text, offset, wide);
            let found = core
                .in_repo(|r| definitions::find_at(r, &request))
                .map_err(gpui_kit::private::anyhow::Error::msg)?;
            // A path or module string is underlined whole, at its columns as shown.
            let origin = found.link.map(|(start, end)| {
                let line = file_line(&request.text, request.line);
                let at = |c: u32| indent::widen_column(line, c as usize, wide) as u32;
                lsp_range(request.line, at(start), at(end))
            });
            Ok(found
                .places
                .into_iter()
                .filter_map(|l| {
                    Some(lsp_types::LocationLink {
                        origin_selection_range: origin,
                        target_uri: file_uri(&root, &l.path)?,
                        target_range: lsp_range(l.line, l.column, l.end_column),
                        target_selection_range: lsp_range(l.line, l.column, l.end_column),
                    })
                })
                .collect())
        })
    }
}

/// A `file://` uri back to a path under `root`.
fn path_of_uri(root: &std::path::Path, uri: &str) -> Option<String> {
    let encoded = uri.strip_prefix("file://")?;
    let decoded = gitviber_markdown::links::decode(encoded);
    let rel = std::path::Path::new(&decoded).strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().into_owned())
}

/// The Tab key's step: a level as shown, which is 4 for a widened file.
fn tab_size(text: &str, wide: usize) -> TabSize {
    let unit = text::indent_unit(text);
    TabSize {
        tab_size: if wide > 0 || unit == 0 {
            indent::TAB
        } else {
            unit
        },
        hard_tabs: false,
    }
}

/// A file's text as the editor holds it: lines joined with \n (the file's \r\n goes back on at
/// save), and its indentation widened.
fn shown(text: String, crlf: bool, wide: usize) -> String {
    let text = if crlf {
        text.replace("\r\n", "\n")
    } else {
        text
    };
    if wide > 0 {
        indent::widen(&text, wide)
    } else {
        text
    }
}

impl CodeView {
    /// `text`: the file's text (moved in, not copied); `head`: HEAD's, for the change bars.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace: WeakEntity<Workspace>,
        source: CodeSource,
        text: String,
        head: Option<String>,
        bars: Vec<Bar>,
        readonly: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let lang = crate::language::language_for(&source.path, Some(&text));
        let language = if text.len() > MAX_HIGHLIGHT_BYTES {
            "text"
        } else {
            lang
        };
        let crlf = text::uses_crlf(&text);
        // Past this the bars aren't worked out again, so HEAD's text isn't kept either.
        let head = head.filter(|_| text.len() <= MAX_LIVE_BARS_BYTES);
        let wrap = gitviber_ui::Settings::get(cx).word_wrap;
        let big = text.len() > MAX_HIGHLIGHT_BYTES;
        let base = text_hash(&text);
        let wide = indent::indent_unit([text.as_str()]);
        let tab = tab_size(&text, wide);
        let text = shown(text, crlf, wide);
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .line_number(true)
                .folding(!big)
                .soft_wrap(wrap)
                .tab_size(tab)
                .default_value(text)
        });
        let wide = Rc::new(std::cell::Cell::new(wide));
        let core = backend::core(cx);
        if let Some(root) = source.root.clone() {
            let provider = Definitions {
                core: core.clone(),
                path: source.path.clone(),
                root: root.clone(),
                wide: wide.clone(),
            };
            let ws = workspace.clone();
            let me = cx.weak_entity();
            state.update(cx, |s, _| {
                let lsp = s.lsp_mut();
                lsp.definition_provider = Some(std::rc::Rc::new(provider));
                lsp.show_document = Some(std::rc::Rc::new(move |params, window, cx| {
                    let Some(path) = path_of_uri(&root, params.uri.as_str()) else {
                        return false;
                    };
                    let line = params.selection.map(|r| r.start.line + 1).unwrap_or(1);
                    let column = params.selection.map(|r| r.start.character).unwrap_or(0);
                    let reveal = Some(Reveal::Line { line, column });
                    let same = me
                        .upgrade()
                        .is_some_and(|me| me.read(cx).source.path == path);
                    if same {
                        _ = me.update(cx, |this, cx| {
                            this.reveal(reveal.clone().unwrap(), window, cx)
                        });
                    } else {
                        open_file(
                            &ws,
                            &path,
                            OpenOptions {
                                preview: false,
                                reveal,
                                focus: true,
                            },
                            window,
                            cx,
                        );
                    }
                    true
                }));
            });
        }
        if readonly {
            state.update(cx, |s, cx| s.set_readonly(true, cx));
        }
        let change = cx.subscribe(&state, |this: &mut Self, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.edits += 1;
                if !this.dirty {
                    this.dirty = true;
                    cx.emit(CodeEvent::DirtyChanged);
                }
                this.bars_soon(cx);
                cx.notify();
            }
        });
        // The caret moved or the text changed: the bracket there is found again.
        let caret = cx.observe(&state, |this: &mut Self, _, cx| this.brackets_soon(cx));
        // Word wrap and blame follow the settings as they change.
        let settings = cx.observe_global_in::<gitviber_ui::Settings>(window, |this, window, cx| {
            let s = gitviber_ui::Settings::get(cx);
            let (wrap, blame) = (s.word_wrap, s.blame);
            this.state
                .update(cx, |st, cx| st.set_soft_wrap(wrap, window, cx));
            if blame != this.blame_on {
                this.blame_on = blame;
                this.load_blame(cx);
            }
            // Ligatures and the code font are read as it draws.
            cx.notify();
        });
        let mut this = CodeView {
            workspace,
            core,
            source,
            state,
            readonly,
            crlf,
            wide,
            lang,
            head: head.map(Arc::new),
            bars: Arc::new(bars),
            map: Rc::default(),
            dirty: false,
            edits: 0,
            base,
            saving: None,
            disk_changed: false,
            brackets: None,
            brackets_at: (usize::MAX, 0),
            goto: None,
            places: None,
            blame: None,
            blame_on: false,
            github: false,
            notes: false,
            focus: cx.focus_handle(),
            _change: change,
            _settings: settings,
            _caret: caret,
            _blame: Task::ready(()),
            _brackets: Task::ready(()),
            _bars: Task::ready(()),
            _save: Task::ready(()),
            _lookup: Task::ready(()),
            _live: Live::new("CodeView"),
        };
        this.blame_on = gitviber_ui::Settings::get(cx).blame;
        this.load_blame(cx);
        this
    }

    /// The selected lines, 1-based and inclusive; a selection ending at a line's start leaves
    /// that line out. None without a selection.
    pub fn selected_lines(&self, cx: &App) -> Option<(u32, u32)> {
        let s = self.state.read(cx);
        let range = s.selected_range();
        if range.is_empty() {
            return None;
        }
        let (start, end) = (
            s.text().offset_to_point(range.start),
            s.text().offset_to_point(range.end),
        );
        let last = if end.column == 0 && end.row > start.row {
            end.row - 1
        } else {
            end.row
        };
        Some((start.row as u32 + 1, last as u32 + 1))
    }

    /// Offers Add Review Note (menu, palette), which comes back as [`CodeEvent::AddNote`].
    pub fn set_notes(&mut self, on: bool) {
        self.notes = on;
    }

    /// Draws `blocks` between the lines, replacing the last set; several under one line stack
    /// in the order given.
    pub fn set_blocks(&mut self, blocks: Vec<CodeBlock>, cx: &mut Context<Self>) {
        let t = cx.app_theme();
        // The editor's rows are whole pixels: unrounded, a block could take a row more than it
        // measures to, and overlap for a frame until it's measured.
        let line_height = (t.fonts.code_size * t.fonts.code_line_height).round();
        let blocks = blocks
            .into_iter()
            .map(|b| LineBlock {
                id: b.id,
                line: b.line.max(1) as usize - 1,
                rows: ((b.height / line_height).ceil() as usize).max(1),
                render: b.render,
            })
            .collect();
        self.state.update(cx, |s, cx| s.set_line_blocks(blocks, cx));
    }

    /// The 1-based line each block is under now (typing moves them), in the order they were set.
    pub fn block_lines(&self, cx: &App) -> Vec<u32> {
        let s = self.state.read(cx);
        s.display_map()
            .gaps()
            .iter()
            .map(|g| g.0 as u32 + 1)
            .collect()
    }

    pub fn source(&self) -> &CodeSource {
        &self.source
    }

    /// Whether the right-click menu has the GitHub items: the file view says, once it knows
    /// origin is on GitHub (it handles them, for previews and media too).
    pub(crate) fn set_github(&mut self, on: bool) {
        self.github = on;
    }

    /// Add Review Note's lines: the selection's, else the caret's.
    fn note_lines(&self, cx: &App) -> (u32, u32) {
        self.selected_lines(cx).unwrap_or_else(|| {
            let s = self.state.read(cx);
            let line = s.text().offset_to_point(s.cursor()).row as u32 + 1;
            (line, line)
        })
    }

    /// The right-click menu: Add Review Note, the editor's own items, References, and the
    /// GitHub ones.
    fn context_menu(&self, menu: NativeMenu, cx: &App) -> NativeMenu {
        let picked = !self.state.read(cx).selected_range().is_empty();
        let in_repo = self.source.root.is_some();
        let menu = if self.notes {
            menu.menu("Add Review Note…", Box::new(review::AddNote))
                .separator()
        } else {
            menu
        };
        let mut menu = menu
            .menu_with_disabled(
                "Go to Definition",
                !in_repo,
                Box::new(editor::GoToDefinition),
            )
            .menu_with_disabled(
                "Go to References",
                !in_repo,
                Box::new(editor::GoToReferences),
            )
            .separator()
            .menu_with_disabled("Cut", self.readonly || !picked, Box::new(edit::Cut))
            .menu_with_disabled("Copy", !picked, Box::new(edit::Copy))
            .menu_with_disabled("Paste", self.readonly, Box::new(edit::Paste));
        if self.github {
            menu = menu
                .menu("Copy GitHub Link", Box::new(editor::CopyGitHubLink))
                .menu("Open on GitHub", Box::new(editor::OpenOnGitHub));
        }
        menu.separator()
            .menu("Select All", Box::new(edit::SelectAll))
    }

    /// Reads who last changed each line in the background, or drops it when blame is off.
    fn load_blame(&mut self, cx: &mut Context<Self>) {
        if !self.blame_on || self.source.root.is_none() {
            self.blame = None;
            self._blame = Task::ready(());
            cx.notify();
            return;
        }
        let (core, path) = (self.core.clone(), self.source.path.clone());
        self._blame = cx.spawn(async move |this, cx| {
            let blame = cx
                .background_spawn(async move { api::history::blame(&core, path) })
                .await;
            _ = this.update(cx, |this, cx| {
                match blame {
                    Ok(blame) => this.blame = Some(Arc::new(Blamed::new(blame))),
                    Err(e) => {
                        this.blame = None;
                        _ = this.workspace.update(cx, |ws, cx| {
                            ws.toast(Toast::error(e).title("Could not blame this file"), cx)
                        });
                    }
                }
                cx.notify();
            });
        });
    }

    /// Shows the commit that last changed `line` (0-based) in History.
    fn show_blamed(&self, line: usize, window: &mut Window, cx: &mut App) {
        let Some(blamed) = &self.blame else { return };
        let Some(c) = blamed.commit(line) else { return };
        if c.sha.bytes().all(|b| b == b'0') {
            return;
        }
        let (subject, body) = c
            .message
            .split_once('\n')
            .map_or((c.message.as_str(), ""), |(s, b)| (s, b.trim_start()));
        let commit = gitviber_core::git::Commit {
            sha: c.sha.clone(),
            short_sha: c.sha.chars().take(7).collect(),
            author_name: c.author_name.clone(),
            author_email: c.author_email.clone(),
            timestamp: c.timestamp,
            committer_name: c.author_name.clone(),
            committed_at: c.timestamp,
            parents: vec![],
            refs: vec![],
            subject: subject.to_string(),
            body: body.to_string(),
            unpushed: false,
            on_origin: false,
            not_in_head: false,
            file: Some(c.path.clone()),
        };
        let mut file = crate::files_selection(&c.path, &c.path);
        if let gitviber_ui::workspace::Selection::Files { file: f } = &mut file {
            let mut change = (**f).clone();
            change.old_path = None;
            *f = Arc::new(change);
        }
        let gitviber_ui::workspace::Selection::Files { file } = file else {
            return;
        };
        let selection = gitviber_ui::workspace::Selection::Commit {
            commit: Arc::new(commit),
            file,
            url: None,
        };
        _ = self
            .workspace
            .update(cx, |ws, cx| ws.open(selection, false, window, cx));
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Each line's commit SHA, once blame has come.
    #[cfg(test)]
    pub(crate) fn blamed_shas(&self) -> Option<Vec<String>> {
        let b = self.blame.as_ref()?;
        Some(
            (0..b.blame.lines.len())
                .filter_map(|l| b.commit(l).map(|c| c.sha.clone()))
                .collect(),
        )
    }

    #[cfg(test)]
    pub(crate) fn code_map(&self) -> Vec<crate::code_map::Mark> {
        self.map.borrow().marks().to_vec()
    }

    /// The brackets framed at the caret.
    #[cfg(test)]
    pub(crate) fn bracket_frames(&self, cx: &App) -> Vec<Range<usize>> {
        self.brackets
            .as_ref()
            .map(|b| b.get_ranges(cx))
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> &Entity<EditorState> {
        &self.state
    }

    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    /// The file's language (a grammar's name; "text" for none).
    pub fn language(&self) -> &'static str {
        self.lang
    }

    pub fn editor_focus(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }

    /// The text as the file has it, typing included, lines joined with \n: a preview and
    /// review notes read the file's own indentation, not the one shown.
    pub fn text(&self, cx: &App) -> Rope {
        let shown = self.state.read(cx).text();
        match self.wide.get() {
            0 => shown.clone(),
            wide => Rope::from(indent::narrow(&shown.to_string(), wide).as_str()),
        }
    }

    /// The file changed on disk: shown as it is now, unless there's typing to keep. Kept typing
    /// is never dropped: the header says the file changed, and ⌘S asks before writing over it.
    pub fn file_changed(
        &mut self,
        text: String,
        head: Option<String>,
        bars: Vec<Bar>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.head = head
            .filter(|_| text.len() <= MAX_LIVE_BARS_BYTES)
            .map(Arc::new);
        let hash = text_hash(&text);
        // Typing is kept as shown; the file as read again shows by its own indentation.
        let wide = if self.dirty {
            self.wide.get()
        } else {
            indent::indent_unit([text.as_str()])
        };
        let tab = (wide != self.wide.get()).then(|| tab_size(&text, wide));
        let text = shown(text, self.crlf, wide);
        let same = {
            let now = self.state.read(cx).text();
            now.len() == text.len() && *now == text.as_str()
        };
        if self.dirty && !same {
            let changed = hash != self.base && self.saving != Some(hash);
            if changed != self.disk_changed {
                self.disk_changed = changed;
                cx.emit(CodeEvent::DirtyChanged);
            }
            self.bars_soon(cx);
            return;
        }
        self.base = hash;
        self.bars = Arc::new(bars);
        if self.dirty || self.disk_changed {
            // What's typed is what's on disk now.
            self.dirty = false;
            self.disk_changed = false;
            cx.emit(CodeEvent::DirtyChanged);
        }
        // The same as shown can still be another step: 4 spaces a level shown as 2 widened.
        self.wide.set(wide);
        if let Some(tab) = tab {
            self.state.update(cx, |s, cx| s.set_tab_size(tab, cx));
        }
        if !same {
            self.state.update(cx, |s, cx| s.set_value(text, window, cx));
            // set_value's own change event isn't typing.
            self.dirty = false;
            self.load_blame(cx);
            cx.emit(CodeEvent::Reloaded);
        }
        cx.notify();
    }

    /// The file changed on disk under unsaved typing.
    pub fn changed_on_disk(&self) -> bool {
        self.disk_changed
    }

    /// Drops the typing, so the file as read next shows (the header's Reload).
    pub(crate) fn discard_typing(&mut self, cx: &mut Context<Self>) {
        if self.dirty || self.disk_changed {
            self.dirty = false;
            self.disk_changed = false;
            cx.emit(CodeEvent::DirtyChanged);
        }
    }

    /// The bars against HEAD for the text now, worked out off the main thread once typing pauses.
    fn bars_soon(&mut self, cx: &mut Context<Self>) {
        let Some(head) = self.head.clone() else {
            return;
        };
        let rope = self.state.read(cx).text().clone();
        if rope.len() > MAX_LIVE_BARS_BYTES {
            return;
        }
        let wide = self.wide.get();
        self._bars = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let bars = cx
                .background_spawn(async move {
                    let now = indent::narrow(&rope.to_string(), wide);
                    let (rows, _) = gitviber_core::diff::rows(
                        &head,
                        &now,
                        gitviber_core::diff::whitespace_mode(None),
                    );
                    text::change_bars(&rows)
                })
                .await;
            _ = this.update(cx, |this, cx| {
                this.bars = Arc::new(bars);
                cx.notify();
            });
        });
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_task(window, cx);
    }

    /// `save`, answered once it's on disk (true) or failed or declined; None where nothing can be
    /// saved. A file changed on disk since the typing began is overwritten only once the user
    /// says so.
    pub fn save_task(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Task<bool>> {
        if self.readonly || self.source.root.is_none() {
            return None;
        }
        let (done, answer) = gitviber_ui::workspace::save_signal(cx);
        let at = self.edits;
        let content = self.content(cx);
        let written = text_hash(&content);
        let (core, path) = (self.core.clone(), self.source.path.clone());
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        // A save still under way goes first: it moves `base` to what it wrote.
        let before = std::mem::replace(&mut self._save, Task::ready(()));
        self._save = cx.spawn_in(window, async move |this, cx| {
            before.await;
            let Ok(base) = this.read_with(cx, |this, _| this.base) else {
                return;
            };
            let disk = {
                let (core, path) = (core.clone(), path.clone());
                cx.background_spawn(async move {
                    let f = api::files::read_file(&core, path).ok()?;
                    // Text this view can't have been editing (now UTF-16, binary) changed too.
                    let other = f.lossy || f.binary || f.too_large;
                    let h = text_hash(&f.text);
                    (f.exists && (other || (h != base && h != written))).then_some(())
                })
                .await
            };
            if disk.is_some() {
                let asked = this.update_in(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Warning,
                        &format!(
                            "{name} changed on disk since you began editing it. Overwrite it with your version?"
                        ),
                        None,
                        &["Overwrite", "Cancel"],
                        cx,
                    )
                });
                let Ok(asked) = asked else { return };
                if asked.await != Ok(0) {
                    done.done(false);
                    return;
                }
            }
            _ = this.update(cx, |this, _| this.saving = Some(written));
            let result = cx
                .background_spawn(async move { api::files::write_file(&core, path, content) })
                .await;
            _ = this.update(cx, |this, cx| {
                this.saving = None;
                match result {
                    Ok(()) => {
                        this.base = written;
                        this.disk_changed = false;
                        if this.edits == at {
                            this.dirty = false;
                        }
                        cx.emit(CodeEvent::Saved);
                        cx.emit(CodeEvent::DirtyChanged);
                        cx.notify();
                        done.done(true);
                    }
                    Err(e) => {
                        _ = this.workspace.update(cx, |ws, cx| {
                            ws.toast(Toast::error(e).title("Could not save"), cx)
                        });
                        done.done(false);
                    }
                }
            });
        });
        Some(answer)
    }

    /// Shows a line: a link's at its column, or a search result's with its match selected.
    pub fn reveal(&mut self, reveal: Reveal, window: &mut Window, cx: &mut Context<Self>) {
        let wide = self.wide.get();
        self.state.update(cx, |s, cx| {
            let text = s.text().clone();
            match reveal {
                Reveal::Line { line, column } => {
                    let shown = line_text(&text, line.max(1) as usize - 1);
                    let own = indent::narrow(&shown, wide);
                    let column = indent::widen_column(&own, column as usize, wide) as u32;
                    let offset = offset_of(&text, line, column);
                    let pos = text.offset_to_position(offset);
                    s.unfold_at(pos, cx);
                    s.set_cursor_position(pos, window, cx);
                }
                Reveal::Match { line, query } => {
                    let start = offset_of(&text, line, 0);
                    let end = text.line_end_offset(
                        (line.max(1) as usize - 1).min(text.lines_len().saturating_sub(1)),
                    );
                    let content = text.slice(start..end).to_string();
                    let found = crate::search::find_matches(&content, &query, 1)
                        .into_iter()
                        .next()
                        .unwrap_or(0..0);
                    let pos = text.offset_to_position(start + found.start);
                    s.unfold_at(pos, cx);
                    s.set_cursor_position(pos, window, cx);
                    s.set_selected_range(start + found.start..start + found.end, cx);
                }
            }
        });
    }

    fn open_goto(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Line, or line:column"));
        input.update(cx, |s, cx| s.focus(window, cx));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, input, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => {
                    let value = input.read(cx).value().to_string();
                    let mut parts = value.trim().splitn(2, [':', ',']);
                    let line = parts.next().and_then(|l| l.trim().parse::<u32>().ok());
                    let column = parts
                        .next()
                        .and_then(|c| c.trim().parse::<u32>().ok())
                        .unwrap_or(1);
                    this.goto = None;
                    if let Some(line) = line {
                        this.reveal(
                            Reveal::Line {
                                line,
                                column: column.saturating_sub(1),
                            },
                            window,
                            cx,
                        );
                    }
                    cx.notify();
                }
                InputEvent::Blur => {
                    this.goto = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        self.goto = Some((input, sub));
        cx.notify();
    }

    /// Definitions or references of the name at the cursor: one opens, several list.
    fn look_up(&mut self, references: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.source.root.clone() else {
            return;
        };
        let (text, offset) = {
            let s = self.state.read(cx);
            (s.text().clone(), s.cursor())
        };
        let (core, path, wide) = (self.core.clone(), self.source.path.clone(), self.wide.get());
        self._lookup = cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let request = request(path, &text, offset, wide);
                    if references {
                        api::files::references(&core, request)
                    } else {
                        api::files::definitions(&core, request)
                    }
                })
                .await;
            _ = this.update_in(cx, |this, window, cx| {
                let places = match found {
                    Err(e) if e == definitions::CANCELLED => return,
                    Err(e) => {
                        _ = this
                            .workspace
                            .update(cx, |ws, cx| ws.toast(Toast::error(e), cx));
                        return;
                    }
                    Ok(places) => places,
                };
                let _ = &root;
                match places.len() {
                    0 => {
                        let what = if references {
                            "No references found."
                        } else {
                            "No definition found."
                        };
                        _ = this
                            .workspace
                            .update(cx, |ws, cx| ws.toast(Toast::info(what), cx));
                    }
                    1 if !references => this.go_to(&places[0], window, cx),
                    n => {
                        let title = if references {
                            format!("{n} references")
                        } else {
                            format!("{n} definitions")
                        };
                        this.places = Some(Places {
                            title: title.into(),
                            places,
                        });
                        cx.notify();
                    }
                }
            });
        });
    }

    fn go_to(&mut self, place: &Location, window: &mut Window, cx: &mut Context<Self>) {
        let reveal = Reveal::Line {
            line: place.line,
            column: place.column,
        };
        if place.path == self.source.path {
            self.reveal(reveal, window, cx);
        } else {
            open_file(
                &self.workspace,
                &place.path,
                OpenOptions {
                    preview: false,
                    reveal: Some(reveal),
                    focus: true,
                },
                window,
                cx,
            );
        }
    }

    /// The selection, for Find in Files to start from.
    pub fn selected_text(&self, cx: &App) -> String {
        let s = self.state.read(cx);
        let range = s.selected_range();
        indent::narrow(&s.text().slice(range).to_string(), self.wide.get())
    }

    /// The text as the file would hold it: its own indentation and line endings.
    fn content(&self, cx: &App) -> String {
        let text = self.text(cx).to_string();
        if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        }
    }

    /// A line command's edit, made as one step to undo. Read-only code lets the key go on.
    fn edit_lines(
        &mut self,
        make: impl FnOnce(&Rope, &[Range<usize>]) -> Option<LineEdit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly {
            return cx.propagate();
        }
        let s = self.state.read(cx);
        let Some(e) = make(s.text(), &s.selected_ranges()) else {
            return;
        };
        self.state.update(cx, |s, cx| {
            s.set_selected_range(e.span, cx);
            s.replace(e.text, window, cx);
            select(s, &e.selections, cx);
        });
    }

    /// ⌘/ where the language has comments; elsewhere the key goes on (the shortcut overlay).
    fn toggle_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match lines::comment_for(self.lang).filter(|_| !self.readonly) {
            Some(c) => self.edit_lines(|t, s| lines::toggle_comment(t, s, c), window, cx),
            None => cx.propagate(),
        }
    }

    /// ⌘D: the word at the caret, then each next place it is, as one more selection.
    fn select_next(&mut self, cx: &mut Context<Self>) {
        let s = self.state.read(cx);
        let Some(next) = lines::next_occurrence(s.text(), &s.selected_ranges()) else {
            return;
        };
        self.state.update(cx, |s, cx| match next {
            lines::Next::Word(r) => s.set_selected_range(r, cx),
            lines::Next::Add(r) => s.add_selection(r, cx),
        });
    }

    /// ⌘L: whole lines, a line more each time.
    fn select_line(&mut self, cx: &mut Context<Self>) {
        let s = self.state.read(cx);
        let whole = lines::select_lines(s.text(), &s.selected_ranges());
        self.state.update(cx, |s, cx| select(s, &whole, cx));
    }

    /// Whether `offset` is code, not in a string or comment, for matching brackets. Each lookup
    /// compares the whole text with the one parsed, so past a few milliseconds the rest of a
    /// scan counts as code: a huge file's frame may be off, the caret never slow.
    fn code_at(state: &EditorState) -> impl Fn(usize) -> bool + '_ {
        let until = std::time::Instant::now() + Duration::from_millis(4);
        move |at| {
            std::time::Instant::now() > until || state.syntax_context_at(at) == SyntaxContext::Code
        }
    }

    /// ⇧⌘\: to the bracket matching the one at the caret, or the end of the pair around it.
    fn jump_to_bracket(&mut self, cx: &mut Context<Self>) {
        if self.readonly {
            return cx.propagate();
        }
        let s = self.state.read(cx);
        let Some(to) = lines::jump_to_bracket(s.text(), s.cursor(), &Self::code_at(s)) else {
            return;
        };
        self.state
            .update(cx, |s, cx| s.set_selected_range(to..to, cx));
    }

    /// The bracket at the caret and its match, framed. Found again when the caret moves, and
    /// once typing pauses when the text changes (strings and comments need the text parsed).
    fn brackets_soon(&mut self, cx: &mut Context<Self>) {
        if self.readonly {
            return;
        }
        let at = (self.state.read(cx).cursor(), self.edits);
        if at == self.brackets_at {
            return;
        }
        let typed = at.1 != self.brackets_at.1;
        self.brackets_at = at;
        self._brackets = cx.spawn(async move |this, cx| {
            if typed {
                cx.background_executor().timer(SETTLE).await;
            }
            _ = this.update(cx, |this, cx| this.show_brackets(cx));
        });
    }

    fn show_brackets(&mut self, cx: &mut Context<Self>) {
        let s = self.state.read(cx);
        let found = s.selected_range().is_empty().then(|| {
            lines::bracket_pair(s.text(), s.cursor(), &Self::code_at(s)).map(|(a, b)| {
                [a, b].map(|at| {
                    RangeDecoration::new(at..at + 1).with_style(RangeDecorationStyle::Frame)
                })
            })
        });
        let frames = found.flatten().map(Vec::from).unwrap_or_default();
        match &self.brackets {
            Some(b) => b.set(frames, cx),
            None if frames.is_empty() => {}
            None => {
                let b = self.state.update(cx, |s, cx| {
                    s.create_range_decorations_collection(frames, cx)
                });
                self.brackets = Some(b);
            }
        }
    }

    /// Space / ⇧Space in read-only code: a page down or up, the caret left where it is.
    fn page(&mut self, down: bool, cx: &mut Context<Self>) {
        if !self.readonly {
            return cx.propagate();
        }
        let s = self.state.read(cx);
        let Some(line) = s.line_height() else {
            return;
        };
        let page = (s.input_bounds().size.height - line).max(line);
        let mut offset = s.scroll_offset();
        offset.y += if down { -page } else { page };
        self.state
            .update(cx, |s, cx| s.set_scroll_offset(offset, cx));
    }

    /// ⌘G / ⇧⌘G while the find box is open; otherwise the key goes on (⇧⌘G: the Git panel).
    fn find_step(&mut self, next: bool, cx: &mut Context<Self>) {
        if !self.state.read(cx).search_session().open {
            return cx.propagate();
        }
        self.state.update(cx, |s, cx| {
            if next {
                s.next_search_match(cx);
            } else {
                s.previous_search_match(cx);
            }
        });
    }

    /// The visible lines' blame labels, each on the first line of its run of lines.
    fn paint_blame(
        state: &Entity<EditorState>,
        blamed: &Blamed,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Where each run starts, read before painting (which needs the app).
        if blamed.blame.unavailable.is_some() {
            return;
        }
        let (runs, line_height) = {
            let s = state.read(cx);
            let (Some(text_bounds), Some(line_height), Some(rows)) =
                (s.text_bounds(), s.line_height(), s.visible_row_range())
            else {
                return;
            };
            let map = s.display_map();
            let mut runs: Vec<(Pixels, Option<usize>, bool)> = Vec::new();
            let mut last = None;
            for line in rows {
                let Some(display) = map.buffer_line_to_display_row_range(line) else {
                    continue;
                };
                let index = blamed.index(line);
                let top = text_bounds.origin.y + line_height * display.start as f32;
                if (index != last || runs.is_empty())
                    && top + line_height >= bounds.top()
                    && top <= bounds.bottom()
                {
                    runs.push((top, index, line > 0 && last.is_some() && index != last));
                }
                last = index;
            }
            (runs, line_height)
        };
        let t = cx.app_theme();
        let (font, text_size, border, subtle, muted) = (
            gpui_kit::font(t.fonts.code.clone()),
            t.fonts.code_size * 0.9,
            t.border,
            t.subtle,
            t.muted_foreground,
        );
        for (top, index, separator) in runs {
            if separator {
                window.paint_quad(fill(
                    Bounds::new(point(bounds.left(), top), size(bounds.size.width, px(1.))),
                    border,
                ));
            }
            let Some(label) = index.and_then(|i| blamed.labels.get(i)) else {
                continue;
            };
            let run = gpui_kit::TextRun {
                len: label.len(),
                font: font.clone(),
                color: if label.as_ref() == "Uncommitted" {
                    subtle
                } else {
                    muted
                },
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(label.clone(), text_size, &[run], None);
            _ = shaped.paint(
                point(bounds.left() + px(8.), top),
                line_height,
                gpui_kit::TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }

    /// The line (0-based) at window height `y`, by the editor's last layout.
    fn line_at(&self, y: Pixels, cx: &App) -> Option<usize> {
        let s = self.state.read(cx);
        let (text_bounds, line_height, rows) =
            (s.text_bounds()?, s.line_height()?, s.visible_row_range()?);
        let map = s.display_map();
        rows.into_iter().find(|&line| {
            map.buffer_line_to_display_row_range(line).is_some_and(|r| {
                let top = text_bounds.origin.y + line_height * r.start as f32;
                let text_rows = (r.len() - map.gap_rows(line)).max(1);
                y >= top && y < top + line_height * text_rows as f32
            })
        })
    }

    fn paint_bars(
        state: &Entity<EditorState>,
        bars: &[Bar],
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let s = state.read(cx);
        let (Some(text_bounds), Some(line_height)) = (s.text_bounds(), s.line_height()) else {
            return;
        };
        let input = s.input_bounds();
        let map = s.display_map();
        let t = cx.app_theme();
        let x = input.origin.x + px(1.);
        // Bars are sorted by line: walk only the visible lines', with one line to spare each side
        // (a deleted mark overhangs its line's top).
        let last_row = map.display_row_count().saturating_sub(1);
        let line_at = |y: Pixels| {
            let row = ((y - text_bounds.origin.y) / line_height).max(0.) as usize;
            map.display_row_to_buffer_line(row.min(last_row)) as u32 + 1
        };
        let (first, last) = (
            line_at(bounds.top()).saturating_sub(1),
            line_at(bounds.bottom()) + 1,
        );
        let from = bars.partition_point(|bar| bar.line < first);
        for bar in bars[from..].iter().take_while(|bar| bar.line <= last) {
            let line = bar.line.max(1) as usize - 1;
            let Some(rows) = map.buffer_line_to_display_row_range(line) else {
                continue;
            };
            let top = text_bounds.origin.y + line_height * rows.start as f32;
            if top > bounds.bottom() || top + line_height < bounds.top() {
                continue;
            }
            // The text's rows, not a block's under it.
            let tall = line_height * (rows.len() - map.gap_rows(line)).max(1) as f32;
            let (color, quad) = match bar.kind {
                BarKind::Added => (t.added, Bounds::new(point(x, top), size(px(3.), tall))),
                BarKind::Modified => (t.primary, Bounds::new(point(x, top), size(px(3.), tall))),
                BarKind::Deleted => (
                    t.removed,
                    Bounds::new(point(x, top - px(2.)), size(px(7.), px(4.))),
                ),
            };
            window.paint_quad(fill(quad, color));
        }
    }
}

impl Render for CodeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let state = self.state.clone();
        let bars = self.bars.clone();
        let map = self.map.clone();
        let goto = self.goto.as_ref().map(|(input, _)| input.clone());
        let places = self.places.as_ref().map(|p| {
            let rows = p.places.iter().enumerate().map(|(i, place)| {
                div()
                    .id(("place", i))
                    .flex()
                    .gap_2()
                    .px_3()
                    .h(px(22.))
                    .items_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover))
                    .text_size(px(12.))
                    .child(div().text_color(t.foreground).child(place.path.clone()))
                    .child(div().text_color(t.subtle).child(format!(":{}", place.line)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        let Some(place) =
                            this.places
                                .as_ref()
                                .and_then(|p| p.places.get(i))
                                .map(|p| Location {
                                    path: p.path.clone(),
                                    ..*p
                                })
                        else {
                            return;
                        };
                        this.places = None;
                        this.go_to(&place, window, cx);
                    }))
            });
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .max_h(relative(0.4))
                .flex()
                .flex_col()
                .bg(t.panel)
                .border_t_1()
                .border_color(t.border_strong)
                .key_context("CodeOverlay")
                .child(
                    div()
                        .flex()
                        .items_center()
                        .px_3()
                        .h(px(26.))
                        .text_size(px(11.5))
                        .text_color(t.muted_foreground)
                        .child(p.title.clone())
                        .child(div().flex_1())
                        .child(
                            gpui_kit::component::button::Button::new("close-places")
                                .xsmall()
                                .label("Close")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.places = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .id("places")
                        .flex_1()
                        .overflow_y_scroll()
                        .children(rows),
                )
        });
        let ligatures = gitviber_ui::Settings::get(cx).ligatures;
        let me = cx.weak_entity();
        // While the file has unsaved edits the labels would name the saved file's lines: none.
        let blame_column = self.blame_on.then(|| {
            let blamed = self.blame.clone().filter(|_| !self.dirty);
            let state = self.state.clone();
            div()
                .id("blame")
                .flex_none()
                .w(blame_width(t.fonts.code_size))
                .h_full()
                .border_r_1()
                .border_color(t.border)
                .cursor_pointer()
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, cx| {
                            if let Some(blamed) = &blamed {
                                window.with_content_mask(
                                    Some(gpui_kit::ContentMask { bounds }),
                                    |window| {
                                        CodeView::paint_blame(&state, blamed, bounds, window, cx)
                                    },
                                );
                            }
                        },
                    )
                    .size_full(),
                )
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(|this, e: &gpui_kit::MouseDownEvent, window, cx| {
                        if let Some(line) = this.line_at(e.position.y, cx) {
                            this.show_blamed(line, window, cx);
                        }
                    }),
                )
        });
        div()
            .id("code-view")
            .key_context("CodeView")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(t.code(t.background))
            .map(|el| indent_clipboard(el, &self.state, self.wide.get()))
            .on_action(cx.listener(|this, _: &file::Save, window, cx| this.save(window, cx)))
            .on_action(cx.listener(|this, _: &GoToLine, window, cx| this.open_goto(window, cx)))
            .on_action(cx.listener(|this, _: &CloseOverlay, window, cx| {
                this.places = None;
                this.goto = None;
                this.state.update(cx, |s, cx| s.focus(window, cx));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &editor::Find, _, cx| {
                this.state.update(cx, |s, cx| s.open_search(false, cx))
            }))
            .on_action(cx.listener(|this, _: &editor::GoToDefinition, window, cx| {
                // A file being edited keeps ⌘↵ (Insert Line Below); F12 still goes.
                let typed = keymap::pressed().is_some_and(|k| {
                    k.key == "enter" && (k.modifiers.platform || k.modifiers.control)
                });
                if typed && !this.readonly {
                    return cx.propagate();
                }
                this.look_up(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &editor::PeekDefinition, window, cx| {
                this.look_up(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &editor::GoToReferences, window, cx| {
                this.look_up(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &search::FindInFiles, window, cx| {
                let seed = this.selected_text(cx);
                crate::find_in_files(&this.workspace, Some(seed), window, cx);
            }))
            .on_action(cx.listener(|_, _: &editor::ToggleBlame, _, cx| {
                gitviber_ui::Settings::update(cx, |s| s.blame = !s.blame);
            }))
            .on_action(cx.listener(|_, _: &editor::ToggleWrap, _, cx| {
                gitviber_ui::Settings::update(cx, |s| s.word_wrap = !s.word_wrap);
            }))
            .on_action(
                cx.listener(|this, _: &ToggleComment, window, cx| this.toggle_comment(window, cx)),
            )
            .on_action(cx.listener(|this, _: &JumpToBracket, _, cx| this.jump_to_bracket(cx)))
            .on_action(cx.listener(|this, _: &SelectNextOccurrence, _, cx| this.select_next(cx)))
            .on_action(cx.listener(|this, _: &SelectLine, _, cx| this.select_line(cx)))
            .on_action(cx.listener(|this, _: &DeleteLine, window, cx| {
                this.edit_lines(lines::delete_lines, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLineUp, window, cx| {
                this.edit_lines(|t, s| lines::move_lines(t, s, true), window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLineDown, window, cx| {
                this.edit_lines(|t, s| lines::move_lines(t, s, false), window, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyLineUp, window, cx| {
                this.edit_lines(|t, s| lines::copy_lines(t, s, true), window, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyLineDown, window, cx| {
                this.edit_lines(|t, s| lines::copy_lines(t, s, false), window, cx)
            }))
            .on_action(cx.listener(|this, _: &InsertLineBelow, window, cx| {
                this.edit_lines(|t, s| lines::insert_line(t, s, true), window, cx)
            }))
            .on_action(cx.listener(|this, _: &InsertLineAbove, window, cx| {
                this.edit_lines(|t, s| lines::insert_line(t, s, false), window, cx)
            }))
            .on_action(cx.listener(|this, _: &ScrollPageDown, _, cx| this.page(true, cx)))
            .on_action(cx.listener(|this, _: &ScrollPageUp, _, cx| this.page(false, cx)))
            .on_action(cx.listener(|this, _: &find::Next, _, cx| this.find_step(true, cx)))
            .on_action(cx.listener(|this, _: &find::Prev, _, cx| this.find_step(false, cx)))
            .on_action(cx.listener(|this, _: &review::AddNote, _, cx| {
                if !this.notes {
                    return cx.propagate();
                }
                let lines = this.note_lines(cx);
                cx.emit(CodeEvent::AddNote { lines });
            }))
            .child(
                div()
                    .size_full()
                    .flex()
                    .when_some(blame_column, |d, column| d.child(column))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(
                                Editor::new(&self.state)
                                    .context_menu(move |menu, _, cx| match me.upgrade() {
                                        Some(this) => this.read(cx).context_menu(menu, cx),
                                        None => menu,
                                    })
                                    .size_full()
                                    .bordered(false)
                                    .appearance(false)
                                    .font_family(t.fonts.code.clone())
                                    .font_weight(t.fonts.code_weight)
                                    // Settings → ligatures: off, `->` and `!=` draw as their characters.
                                    .font_features(if ligatures {
                                        gpui_kit::FontFeatures::default()
                                    } else {
                                        gpui_kit::FontFeatures::disable_ligatures()
                                    })
                                    .text_size(t.fonts.code_size)
                                    .line_height(relative(t.fonts.code_line_height)),
                            )
                            .child(
                                canvas(
                                    |_, _, _| (),
                                    move |bounds, _, window, cx| {
                                        window.with_content_mask(
                                            Some(gpui_kit::ContentMask { bounds }),
                                            |window| {
                                                CodeView::paint_bars(
                                                    &state, &bars, bounds, window, cx,
                                                );
                                                crate::code_map::paint(
                                                    &map, &state, &bars, window, cx,
                                                );
                                            },
                                        );
                                    },
                                )
                                .absolute()
                                .inset_0(),
                            ),
                    ),
            )
            .children(goto.map(|input| {
                div()
                    .absolute()
                    .top_2()
                    .right_4()
                    .w(px(260.))
                    .p_1()
                    .rounded(px(6.))
                    .bg(t.elevated)
                    .border_1()
                    .border_color(t.border_strong)
                    .key_context("CodeOverlay")
                    .child(Input::new(&input).small())
            }))
            .children(places)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AnyView, TestAppContext, VisualTestContext};

    struct Host(Option<AnyView>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(self.0.clone())
        }
    }

    fn setup(cx: &mut TestAppContext) {
        gitviber_ui::test_support::config_dir("files-code-test");
        cx.update(|cx| {
            gitviber_ui::init(cx);
            crate::init(cx);
        });
    }

    /// A code view of `text`, drawn, its editor focused.
    fn code_view<'a>(
        cx: &'a mut TestAppContext,
        text: &str,
    ) -> (Entity<CodeView>, &'a mut VisualTestContext) {
        let (host, cx) = cx.add_window_view(|_, _| Host(None));
        let code = cx.update(|window, cx| {
            let source = CodeSource {
                path: "src/a.ts".into(),
                root: None,
            };
            let text = text.to_string();
            let code = cx.new(|cx| {
                CodeView::new(
                    WeakEntity::<Workspace>::new_invalid(),
                    source,
                    text,
                    None,
                    vec![],
                    false,
                    window,
                    cx,
                )
            });
            host.update(cx, |h, cx| {
                h.0 = Some(code.clone().into());
                cx.notify();
            });
            code
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| code.read(cx).editor_focus(cx).focus(window, cx));
        cx.run_until_parked();
        (code, cx)
    }

    fn shown_text(cx: &mut VisualTestContext, code: &Entity<CodeView>) -> String {
        cx.update(|_, cx| code.read(cx).state().read(cx).text().to_string())
    }

    fn put(cx: &mut VisualTestContext, code: &Entity<CodeView>, at: Range<usize>) {
        cx.update(|_, cx| {
            let state = code.read(cx).state().clone();
            state.update(cx, |s, cx| s.set_selected_range(at, cx));
        });
    }

    fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()))
    }

    /// A file indented 2 spaces a level shows 4 wide, as its diff does; what leaves the view
    /// (a save, Copy, Cut, a lookup) has the file's own indentation, and a paste is widened.
    #[gpui_kit::test]
    fn two_space_code_shows_four_wide_and_leaves_as_it_is(cx: &mut TestAppContext) {
        setup(cx);
        let file = "fn a() {\n  if x {\n    y();\n  }\n}\n";
        let (code, cx) = code_view(cx, file);
        let wide = "fn a() {\n    if x {\n        y();\n    }\n}\n";
        assert_eq!(shown_text(cx, &code), wide);
        assert_eq!(cx.update(|_, cx| code.read(cx).content(cx)), file);
        // A preview or a review note reads the file's lines.
        assert_eq!(cx.update(|_, cx| code.read(cx).text(cx).to_string()), file);
        // Tab steps a level as shown.
        assert_eq!(tab_size(file, 2).tab_size, indent::TAB);
        assert_eq!(tab_size(file, 0).tab_size, 2);

        // Copy: lines 2 and 3 as the file has them.
        let line2 = wide.find("    if").unwrap();
        let line4 = wide.find("    }").unwrap();
        put(cx, &code, line2..line4);
        cx.dispatch_action(edit::Copy);
        assert_eq!(clipboard(cx).as_deref(), Some("  if x {\n    y();\n"));
        assert_eq!(
            cx.update(|_, cx| code.read(cx).selected_text(cx)),
            "  if x {\n    y();\n"
        );

        // Cut: the line goes, the clipboard has it as the file does.
        let line3 = wide.find("        y").unwrap();
        put(cx, &code, line3..line4);
        cx.dispatch_action(edit::Cut);
        assert_eq!(clipboard(cx).as_deref(), Some("    y();\n"));
        assert_eq!(shown_text(cx, &code), "fn a() {\n    if x {\n    }\n}\n");

        // Paste after `{`: the new line's level is widened; saved, it's the file's 2 again.
        let end1 = "fn a() {\n    if x {".len();
        put(cx, &code, end1..end1);
        cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("\n    z();".into())));
        cx.dispatch_action(edit::Paste);
        assert_eq!(
            shown_text(cx, &code),
            "fn a() {\n    if x {\n        z();\n    }\n}\n"
        );
        assert_eq!(
            cx.update(|_, cx| code.read(cx).content(cx)),
            "fn a() {\n  if x {\n    z();\n  }\n}\n"
        );
        // Undo takes the paste back in one step.
        cx.dispatch_action(edit::Undo);
        assert_eq!(shown_text(cx, &code), "fn a() {\n    if x {\n    }\n}\n");
    }

    /// Code indented 4 a level, or with tabs, shows as it is; read again from disk with another
    /// step, the file shows by its new one.
    #[gpui_kit::test]
    fn other_indentation_shows_as_it_is(cx: &mut TestAppContext) {
        setup(cx);
        let file = "fn a() {\n    b();\n}\n";
        let (code, cx) = code_view(cx, file);
        assert_eq!(shown_text(cx, &code), file);
        put(cx, &code, 0..file.len());
        cx.dispatch_action(edit::Copy);
        assert_eq!(clipboard(cx).as_deref(), Some(file));
        let two = "fn a() {\n  b();\n}\n";
        cx.update(|window, cx| {
            code.update(cx, |c, cx| {
                c.file_changed(two.into(), None, vec![], window, cx)
            })
        });
        assert_eq!(shown_text(cx, &code), file);
        assert_eq!(cx.update(|_, cx| code.read(cx).content(cx)), two);
        assert!(!cx.update(|_, cx| code.read(cx).is_dirty()));
    }

    /// ⌘-click on an import or a `path:line` in a comment goes to that file, the whole string
    /// underlined at its columns as shown.
    #[gpui_kit::test]
    fn an_import_or_a_path_goes_to_its_file(cx: &mut TestAppContext) {
        setup(cx);
        let repo = std::env::temp_dir().join(format!("gitviber-code-links-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let file = "import { u } from \"./util\";\nif (u) {\n  // see src/util.ts:3\n}\n";
        std::fs::write(repo.join("src/a.ts"), file).unwrap();
        std::fs::write(repo.join("src/util.ts"), "export const u = 1;\n").unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        api::repo::open_repo(&core, repo.to_string_lossy().into_owned()).unwrap();
        let provider = Definitions {
            core,
            path: "src/a.ts".into(),
            root: repo.clone(),
            wide: Rc::new(std::cell::Cell::new(2)),
        };
        let (_, cx) = cx.add_window_view(|_, _| Host(None));
        let shown = indent::widen(file, 2);
        let rope = Rope::from(shown.as_str());
        let ask = |cx: &mut VisualTestContext, at: usize| {
            let task = cx.update(|window, cx| provider.definitions(&rope, at, window, cx));
            let out = Rc::new(RefCell::new(None));
            let o = out.clone();
            cx.update(|_, cx| {
                cx.spawn(async move |_| *o.borrow_mut() = Some(task.await.unwrap()))
                    .detach()
            });
            cx.run_until_parked();
            let links = out.borrow_mut().take().unwrap();
            links
                .into_iter()
                .map(|l| {
                    let path = path_of_uri(&repo, l.target_uri.as_str()).unwrap();
                    (path, l.target_range.start.line, l.origin_selection_range)
                })
                .collect::<Vec<_>>()
        };
        let import = shown.find("./util").unwrap() + 2;
        assert_eq!(
            ask(cx, import),
            [("src/util.ts".into(), 0, Some(lsp_range(1, 19, 25)))]
        );
        let comment = shown.find("src/util").unwrap() + 4;
        let at = shown[..comment].rfind('\n').unwrap() + 1;
        let start = (shown.find("src/util").unwrap() - at) as u32;
        assert_eq!(
            ask(cx, comment),
            [(
                "src/util.ts".into(),
                2,
                Some(lsp_range(3, start, start + 13))
            )]
        );
        _ = std::fs::remove_dir_all(&repo);
    }
}
