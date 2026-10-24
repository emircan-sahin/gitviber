//! A vault file's text in gpui-component's code editor, saved back through core with ⌘S and its
//! own line endings. What's typed is compared with the file as it was read (`base`): typing
//! undone is clean again, and a save first checks the disk still has `base`, else asks before
//! overwriting what Obsidian or a sync wrote meanwhile. Unsaved typing is kept in the vault edits
//! store when the view goes and at quit. Read-only for what can't be written back as it was
//! (lossy text, mixed line endings), as the repo's code view.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gitviber_core::{Core, api};
use gitviber_files::text;
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::{editor, file};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{SaveDone, Toast, Workspace};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Editor, EditorState, InputEvent, TabSize};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    Task, WeakEntity, Window, div, relative,
};

use crate::edits::{self, Edit};

/// Past this the text isn't parsed for colors or folds.
const MAX_HIGHLIGHT_BYTES: usize = 2_000_000;
/// Typing goes to the edits store once it pauses this long.
const KEEP_AFTER: Duration = Duration::from_millis(500);

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
    /// The file as it was read or last saved, as it holds it (its line endings on).
    base: Arc<str>,
    lossy: bool,
    readonly: bool,
    crlf: bool,
    dirty: bool,
    /// A write is on its way: another ⌘S waits for it rather than racing it.
    saving: bool,
    /// What was typed was dropped (Don't Save): it isn't kept as the view goes.
    forgotten: bool,
    focus: FocusHandle,
    _change: Subscription,
    _keep_on_exit: [Subscription; 2],
    _save: Task<()>,
    _keep: Task<()>,
    _live: Live,
}

impl EventEmitter<EditorEvent> for NoteEditor {}

fn readonly_for(lossy: bool, text: &str) -> bool {
    lossy || !text::keeps_line_endings(text)
}

impl NoteEditor {
    /// `text`: the file as read; `lossy`: that isn't the file's own bytes, so it's never written
    /// back. Typing kept from before (edits store) comes back over it.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace: WeakEntity<Workspace>,
        vault: String,
        path: String,
        text: String,
        lossy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let kept = (!readonly_for(lossy, &text))
            .then(|| edits::get(&vault, &path, cx))
            .flatten()
            .filter(|e| text::keeps_line_endings(&e.text));
        let (shown, base) = match kept {
            Some(Edit { text: typed, base }) => (typed, base),
            None => (text.clone(), text),
        };
        let big = shown.len() > MAX_HIGHLIGHT_BYTES;
        let language = if big {
            "text"
        } else {
            gitviber_files::language::language_for(&path, Some(&shown))
        };
        let readonly = readonly_for(lossy, &shown);
        let crlf = text::uses_crlf(&shown);
        let unit = text::indent_unit(&shown);
        let wrap = gitviber_ui::Settings::get(cx).word_wrap;
        let dirty = shown != base;
        // The editor joins lines with \n; the file's \r\n goes back on at save.
        let value = if crlf {
            shown.replace("\r\n", "\n")
        } else {
            shown
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
                .default_value(value)
        });
        if readonly {
            state.update(cx, |s, cx| s.set_readonly(true, cx));
        }
        let change = cx.subscribe(&state, |this: &mut Self, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.typed(cx);
            }
        });
        // What's typed and not saved outlives the view: its tab closing, a repo switch, a quit.
        let keep_on_exit = [
            cx.on_release(|this, cx| this.keep(usize::MAX, cx)),
            cx.on_app_quit(|this, cx| {
                this.keep(usize::MAX, cx);
                gitviber_ui::store::flush(cx);
                async {}
            }),
        ];
        NoteEditor {
            workspace,
            core: gitviber_ui::core(cx),
            vault,
            path,
            state,
            base: base.into(),
            lossy,
            readonly,
            crlf,
            dirty,
            saving: false,
            forgotten: false,
            focus: cx.focus_handle(),
            _change: change,
            _keep_on_exit: keep_on_exit,
            _save: Task::ready(()),
            _keep: Task::ready(()),
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

    /// Something was typed (or undone): dirty while it differs from the file as read.
    fn typed(&mut self, cx: &mut Context<Self>) {
        self.forgotten = false;
        let dirty = self.content(cx) != *self.base;
        if dirty != self.dirty {
            self.dirty = dirty;
            cx.emit(EditorEvent::DirtyChanged);
        }
        self._keep = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(KEEP_AFTER).await;
            _ = this.update(cx, |this, cx| this.keep(edits::MAX_PAUSE_BYTES, cx));
        });
        cx.notify();
    }

    /// Types `text` over everything inside the caller's update, for tests that need typing and
    /// saves without the executor running between them.
    #[cfg(test)]
    pub(crate) fn type_now(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.state.update(cx, |s, cx| s.set_value(text, window, cx));
        self.typed(cx);
    }

    /// Puts what's typed (or that nothing is) in the edits store, unless it's past `max` bytes.
    fn keep(&self, max: usize, cx: &mut App) {
        if self.readonly || self.forgotten {
            return;
        }
        let text = self.content(cx);
        if text.len() + self.base.len() > max {
            return;
        }
        let edit = (text != *self.base).then(|| Edit {
            text,
            base: self.base.to_string(),
        });
        edits::set(&self.vault, &self.path, edit, cx);
    }

    /// The file was read again (it changed on disk): shown as it is now unless there's typing,
    /// which stays (a save then asks). `lossy` is the new read's.
    pub fn file_changed(
        &mut self,
        text: &str,
        lossy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.lossy = lossy;
        if !self.dirty {
            if self.content(cx) != text {
                self.crlf = text::uses_crlf(text);
                let value = if self.crlf {
                    text.replace("\r\n", "\n")
                } else {
                    text.to_string()
                };
                // set_value says nothing of its own: it isn't typing.
                self.state
                    .update(cx, |s, cx| s.set_value(value, window, cx));
            }
            self.base = text.into();
        }
        let readonly = readonly_for(self.lossy, &self.content(cx));
        if readonly != self.readonly {
            self.readonly = readonly;
            self.state.update(cx, |s, cx| s.set_readonly(readonly, cx));
        }
        cx.notify();
    }

    /// Drops what's typed for good (Don't Save): out of the edits store, and not kept again as
    /// the view goes. Typing after it is kept as any is.
    pub fn forget(&mut self, cx: &mut Context<Self>) {
        self.forgotten = true;
        self.dirty = false;
        self._keep = Task::ready(());
        edits::set(&self.vault, &self.path, None, cx);
        cx.emit(EditorEvent::DirtyChanged);
        cx.notify();
    }

    /// ⌘S: writes what's typed, unless the file changed on disk since it was read (then it asks).
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_then(None, window, cx);
    }

    /// `save`, answering `done` (true once written) for the ask before edits would be lost.
    pub fn save_then(
        &mut self,
        done: Option<SaveDone>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.readonly || !self.dirty {
            if let Some(done) = done {
                done.done(!self.dirty);
            }
            return;
        }
        self.write(false, done, window, cx);
    }

    fn write(
        &mut self,
        force: bool,
        done: Option<SaveDone>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = self.content(cx);
        let saved: Arc<str> = content.as_str().into();
        let expected = self.base.clone();
        let (core, vault, path) = (self.core.clone(), self.vault.clone(), self.path.clone());
        self.saving = true;
        self._save = cx.spawn_in(window, async move |this, cx| {
            let written = cx
                .background_spawn(async move {
                    // Obsidian or a sync may have written it meanwhile: its change would be lost.
                    if !force {
                        let now =
                            api::obsidian::vault_read_file(&core, vault.clone(), path.clone())?;
                        if !now.exists || now.lossy || now.text != *expected {
                            return Ok(false);
                        }
                    }
                    api::obsidian::vault_write_file(&core, vault, path, content).map(|_| true)
                })
                .await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match written {
                    Ok(true) => {
                        this.base = saved.clone();
                        // Typing during the write stays unsaved.
                        this.dirty = this.content(cx) != *this.base;
                        this.keep(usize::MAX, cx);
                        cx.emit(EditorEvent::Saved(saved));
                        cx.emit(EditorEvent::DirtyChanged);
                        cx.notify();
                        if let Some(done) = done {
                            done.done(true);
                        }
                    }
                    Ok(false) => this.ask_overwrite(done, window, cx),
                    Err(e) => {
                        _ = this.workspace.update(cx, |ws, cx| {
                            ws.toast(Toast::error(e).title("Could not save"), cx)
                        });
                    }
                }
            });
        });
    }

    fn ask_overwrite(
        &mut self,
        done: Option<SaveDone>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        let name = self
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&self.path)
            .to_string();
        // Answered by the button chosen; a dialog closed otherwise drops it, which is a no.
        let done = Rc::new(RefCell::new(done));
        let no = done.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (weak, done, no) = (weak.clone(), done.clone(), no.clone());
            alert
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .title("Changed on disk")
                .description(format!(
                    "{name} changed on disk since you started editing it. Overwrite it with your edits?"
                ))
                .ok_text("Overwrite")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let done = done.borrow_mut().take();
                    weak.update(cx, |this, cx| this.write(true, done, window, cx)).ok();
                    true
                })
                .on_cancel(move |_, _, _| {
                    if let Some(done) = no.borrow_mut().take() {
                        done.done(false);
                    }
                    true
                })
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
