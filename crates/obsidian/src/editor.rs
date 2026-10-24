//! A vault file's text in gpui-component's code editor, saved back through core with ⌘S and
//! its own line endings. Read-only for what can't be written back as it was (lossy text, mixed
//! line endings), as the repo's code view.

use std::sync::Arc;

use gitviber_core::{Core, api};
use gitviber_files::text;
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::{editor, file};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::component::input::{Editor, EditorState, InputEvent, TabSize};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    Task, WeakEntity, Window, div, relative,
};

/// Past this the text isn't parsed for colors or folds.
const MAX_HIGHLIGHT_BYTES: usize = 2_000_000;

pub enum EditorEvent {
    DirtyChanged,
    /// Written to disk: the text as it now is there.
    Saved(Arc<str>),
}

pub struct NoteEditor {
    workspace: WeakEntity<Workspace>,
    core: Arc<Core>,
    vault: String,
    path: String,
    state: Entity<EditorState>,
    readonly: bool,
    crlf: bool,
    dirty: bool,
    /// Bumped by each edit, so a save that raced typing leaves the file dirty.
    edits: u64,
    focus: FocusHandle,
    _change: Subscription,
    _save: Task<()>,
    _live: Live,
}

impl EventEmitter<EditorEvent> for NoteEditor {}

impl NoteEditor {
    /// `lossy`: the text isn't the file's own bytes, so it's never written back.
    pub fn new(
        workspace: WeakEntity<Workspace>,
        vault: String,
        path: String,
        text: String,
        lossy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let big = text.len() > MAX_HIGHLIGHT_BYTES;
        let language = if big {
            "text"
        } else {
            gitviber_files::language::language_for(&path, Some(&text))
        };
        let readonly = lossy || !text::keeps_line_endings(&text);
        let crlf = text::uses_crlf(&text);
        let unit = text::indent_unit(&text);
        let wrap = gitviber_ui::Settings::get(cx).word_wrap;
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
        if readonly {
            state.update(cx, |s, cx| s.set_readonly(true, cx));
        }
        let change = cx.subscribe(&state, |this: &mut Self, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.edits += 1;
                if !this.dirty {
                    this.dirty = true;
                    cx.emit(EditorEvent::DirtyChanged);
                }
                cx.notify();
            }
        });
        NoteEditor {
            workspace,
            core: gitviber_ui::core(cx),
            vault,
            path,
            state,
            readonly,
            crlf,
            dirty: false,
            edits: 0,
            focus: cx.focus_handle(),
            _change: change,
            _save: Task::ready(()),
            _live: Live::new("NoteEditor"),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    pub fn editor_focus(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }

    /// The text as the file would hold it (its line endings back on).
    pub fn content(&self, cx: &App) -> String {
        let text = self.state.read(cx).text().to_string();
        if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        }
    }

    /// The file changed on disk: shown as it is now, unless there's typing to keep.
    pub fn file_changed(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.dirty || self.content(cx) == text {
            return;
        }
        let text = if self.crlf {
            text.replace("\r\n", "\n")
        } else {
            text.to_string()
        };
        self.state.update(cx, |s, cx| s.set_value(text, window, cx));
        // set_value's own change event isn't typing.
        self.dirty = false;
        cx.notify();
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly || !self.dirty {
            return;
        }
        let at = self.edits;
        let content = self.content(cx);
        let saved: Arc<str> = content.as_str().into();
        let (core, vault, path) = (self.core.clone(), self.vault.clone(), self.path.clone());
        self._save = cx.spawn_in(window, async move |this, cx| {
            let written = cx
                .background_spawn(async move {
                    api::obsidian::vault_write_file(&core, vault, path, content)
                })
                .await;
            _ = this.update(cx, |this, cx| match written {
                Ok(()) => {
                    if this.edits == at {
                        this.dirty = false;
                    }
                    cx.emit(EditorEvent::Saved(saved));
                    cx.emit(EditorEvent::DirtyChanged);
                    cx.notify();
                }
                Err(e) => {
                    _ = this.workspace.update(cx, |ws, cx| {
                        ws.toast(Toast::error(e).title("Could not save"), cx)
                    });
                }
            });
        });
    }
}

impl Render for NoteEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        div()
            .id("vault-code")
            .key_context("CodeView")
            .track_focus(&self.focus)
            .size_full()
            .bg(t.code(t.background))
            .on_action(cx.listener(|this, _: &file::Save, window, cx| this.save(window, cx)))
            .on_action(cx.listener(|this, _: &editor::Find, _, cx| {
                this.state.update(cx, |s, cx| s.open_search(false, cx))
            }))
            .on_action(cx.listener(|_, _: &editor::ToggleWrap, _, cx| {
                gitviber_ui::Settings::update(cx, |s| s.word_wrap = !s.word_wrap);
            }))
            .child(
                Editor::new(&self.state)
                    .size_full()
                    .bordered(false)
                    .appearance(false)
                    .font_family(t.fonts.code.clone())
                    .text_size(t.fonts.code_size)
                    .line_height(relative(t.fonts.code_line_height)),
            )
    }
}
