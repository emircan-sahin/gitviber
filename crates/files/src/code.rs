//! A file's text in gpui-component's code editor: tree-sitter colors, line numbers, find (⌘F),
//! go to line (⌃G), Go to Definition / References through core, the change bars against HEAD in
//! the gutter, and ⌘S writing it back with its own line endings. Read-only for what can't be
//! written back as it was (outside the repo, lossy text, mixed line endings).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gitviber_core::definitions::{self, Location};
use gitviber_core::{Core, api};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::{editor, file, search};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{
    DefinitionProvider, Editor, EditorState, Input, InputEvent, InputState, Position, Rope,
    RopeExt as _, TabSize,
};
use gpui_kit::{
    App, AppContext as _, Bounds, ClickEvent, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, Window, actions,
    canvas, div, fill, point, prelude::FluentBuilder as _, px, relative, size,
};

use crate::backend;
use crate::text::{self, Bar, BarKind};
use crate::{OpenOptions, Reveal, open_file};

actions!(code_view, [GoToLine, CloseOverlay]);

/// Past this the text isn't parsed for colors or folds: a minified bundle costs more than they help.
pub const MAX_HIGHLIGHT_BYTES: usize = 2_000_000;
/// Past this the bars aren't worked out again while typing.
const MAX_LIVE_BARS_BYTES: usize = 1_500_000;
const SETTLE: Duration = Duration::from_millis(150);

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui_kit::KeyBinding::new("ctrl-g", GoToLine, Some("CodeView")),
        gpui_kit::KeyBinding::new("escape", CloseOverlay, Some("CodeOverlay")),
    ]);
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
    /// HEAD's text, for the bars once the file is typed into; None: no bars (new, outside).
    head: Option<Arc<String>>,
    bars: Arc<Vec<Bar>>,
    dirty: bool,
    /// Bumped by each edit, so a save that raced typing leaves the file dirty.
    edits: u64,
    goto: Option<(Entity<InputState>, Subscription)>,
    places: Option<Places>,
    /// Who last changed each line, while Settings asks for it (View → Blame).
    blame: Option<Arc<Blamed>>,
    blame_on: bool,
    focus: FocusHandle,
    _change: Subscription,
    _settings: Subscription,
    _blame: Task<()>,
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
}

/// The 0-based UTF-16 column of `offset` in its line, as core counts columns.
fn utf16_column(text: &Rope, offset: usize) -> (u32, u32) {
    let p = text.offset_to_point(offset);
    let start = text.line_start_offset(p.row);
    let line: String = text.slice(start..offset).to_string();
    (p.row as u32 + 1, line.encode_utf16().count() as u32)
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
        let (core, path, root, text) = (
            self.core.clone(),
            self.path.clone(),
            self.root.clone(),
            text.clone(),
        );
        cx.background_spawn(async move {
            let (line, column) = utf16_column(&text, offset);
            let request = definitions::Request {
                path,
                text: text.to_string(),
                line,
                column,
                rev: None,
            };
            let found = api::files::definitions(&core, request)
                .map_err(gpui_kit::private::anyhow::Error::msg)?;
            Ok(found
                .into_iter()
                .filter_map(|l| {
                    Some(lsp_types::LocationLink {
                        origin_selection_range: None,
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
        let language = if text.len() > MAX_HIGHLIGHT_BYTES {
            "text"
        } else {
            crate::language::language_for(&source.path, Some(&text))
        };
        let crlf = text::uses_crlf(&text);
        // Past this the bars aren't worked out again, so HEAD's text isn't kept either.
        let head = head.filter(|_| text.len() <= MAX_LIVE_BARS_BYTES);
        let unit = text::indent_unit(&text);
        let wrap = gitviber_ui::Settings::get(cx).word_wrap;
        let big = text.len() > MAX_HIGHLIGHT_BYTES;
        // The editor joins lines with \n; the file's \r\n goes back on at save.
        let text = if crlf {
            text.replace("\r\n", "\n")
        } else {
            text
        };
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .line_number(true)
                .folding(!big)
                .soft_wrap(wrap)
                .tab_size(TabSize {
                    tab_size: if unit == 0 { 4 } else { unit },
                    hard_tabs: false,
                })
                .default_value(text)
        });
        let core = backend::core(cx);
        if let Some(root) = source.root.clone() {
            let provider = Definitions {
                core: core.clone(),
                path: source.path.clone(),
                root: root.clone(),
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
            head: head.map(Arc::new),
            bars: Arc::new(bars),
            dirty: false,
            edits: 0,
            goto: None,
            places: None,
            blame: None,
            blame_on: false,
            focus: cx.focus_handle(),
            _change: change,
            _settings: settings,
            _blame: Task::ready(()),
            _bars: Task::ready(()),
            _save: Task::ready(()),
            _lookup: Task::ready(()),
            _live: Live::new("CodeView"),
        };
        this.blame_on = gitviber_ui::Settings::get(cx).blame;
        this.load_blame(cx);
        this
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
    pub(crate) fn state(&self) -> &Entity<EditorState> {
        &self.state
    }

    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    pub fn editor_focus(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }

    pub fn text<'a>(&self, cx: &'a App) -> &'a Rope {
        self.state.read(cx).text()
    }

    /// The file changed on disk: shown as it is now, unless there's typing to keep.
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
        if self.dirty {
            self.bars_soon(cx);
            return;
        }
        self.bars = Arc::new(bars);
        let text = if self.crlf {
            text.replace("\r\n", "\n")
        } else {
            text
        };
        let same = {
            let shown = self.state.read(cx).text();
            shown.len() == text.len() && *shown == text.as_str()
        };
        if !same {
            self.state.update(cx, |s, cx| s.set_value(text, window, cx));
            // set_value's own change event isn't typing.
            self.dirty = false;
            self.load_blame(cx);
        }
        cx.notify();
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
        self._bars = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let bars = cx
                .background_spawn(async move {
                    let now = rope.to_string();
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

    /// `save`, answered once it's on disk (true) or failed; None where nothing can be saved.
    pub fn save_task(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Task<bool>> {
        if self.readonly || self.source.root.is_none() {
            return None;
        }
        let (done, answer) = gitviber_ui::workspace::save_signal(cx);
        let at = self.edits;
        let mut content = self.state.read(cx).text().to_string();
        if self.crlf {
            content = content.replace('\n', "\r\n");
        }
        let (core, path) = (self.core.clone(), self.source.path.clone());
        self._save = cx.spawn_in(window, async move |this, cx| {
            let written = cx
                .background_spawn(async move { api::files::write_file(&core, path, content) })
                .await;
            _ = this.update(cx, |this, cx| match written {
                Ok(()) => {
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
            });
        });
        Some(answer)
    }

    /// Shows a line: a link's at its column, or a search result's with its match selected.
    pub fn reveal(&mut self, reveal: Reveal, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| {
            let text = s.text().clone();
            match reveal {
                Reveal::Line { line, column } => {
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
        let (core, path) = (self.core.clone(), self.source.path.clone());
        self._lookup = cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let (line, column) = utf16_column(&text, offset);
                    let request = definitions::Request {
                        path,
                        text: text.to_string(),
                        line,
                        column,
                        rev: None,
                    };
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
        s.text().slice(range).to_string()
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
                y >= top && y < top + line_height * r.len().max(1) as f32
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
        for bar in bars {
            let line = bar.line.max(1) as usize - 1;
            let Some(rows) = map.buffer_line_to_display_row_range(line) else {
                continue;
            };
            let top = text_bounds.origin.y + line_height * rows.start as f32;
            if top > bounds.bottom() || top + line_height < bounds.top() {
                continue;
            }
            let (color, quad) = match bar.kind {
                BarKind::Added => (
                    t.added,
                    Bounds::new(
                        point(x, top),
                        size(px(3.), line_height * rows.len().max(1) as f32),
                    ),
                ),
                BarKind::Modified => (
                    t.primary,
                    Bounds::new(
                        point(x, top),
                        size(px(3.), line_height * rows.len().max(1) as f32),
                    ),
                ),
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
                                    .size_full()
                                    .bordered(false)
                                    .appearance(false)
                                    .font_family(t.fonts.code.clone())
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
                                                )
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
