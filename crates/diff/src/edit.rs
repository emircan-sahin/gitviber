//! The new side of an unstaged diff, typed into and saved with ⌘S: the file on disk in a code
//! editor, its lines that differ from the index tinted as the diff tints them, worked out again
//! off the main thread a moment after typing stops.

use crate::highlight::language_for;
use crate::model::ADDED;
use crate::style::DiffStyle;
use gitviber_core::Core;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{
    Editor, EditorState, InputEvent, RangeDecoration, RangeDecorationCollection,
    RangeDecorationStyle,
};
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, Window, div,
};
use std::sync::Arc;
use std::time::Duration;

/// How long typing must pause before the tints follow.
const SETTLE: Duration = Duration::from_millis(150);

pub enum EditEvent {
    /// Something was typed.
    Changed,
    Saved,
    Failed(String),
    /// ⌘S did nothing: why.
    Refused(String),
}

pub struct DiffEditor {
    core: Arc<Core>,
    path: String,
    /// The index's text the tints compare with.
    base: Arc<str>,
    /// What was last read from or written to disk.
    saved: Arc<str>,
    state: Entity<EditorState>,
    tints: RangeDecorationCollection,
    /// A save is on its way: the next ⌘S waits for it rather than race it to the disk.
    saving: bool,
    /// Saving is off: the file on disk became something the editor can't write back as it was.
    refused: Option<String>,
    _change: Subscription,
    _tint: Task<()>,
    _save: Task<()>,
}

impl EventEmitter<EditEvent> for DiffEditor {}

impl DiffEditor {
    pub fn new(
        path: String,
        base: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let language = language_for(&path);
        let initial = text.to_string();
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .line_number(true)
                .default_value(initial)
        });
        let tints = state.update(cx, |s, cx| {
            s.create_range_decorations_collection(vec![], cx)
        });
        let change = cx.subscribe_in(
            &state,
            window,
            |this: &mut Self, _, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::Change) {
                    this.tint_soon(cx);
                    cx.emit(EditEvent::Changed);
                }
            },
        );
        let mut editor = DiffEditor {
            core: crate::core(cx),
            path,
            base: base.into(),
            saved: text.into(),
            state,
            tints,
            saving: false,
            refused: None,
            _change: change,
            _tint: Task::ready(()),
            _save: Task::ready(()),
        };
        editor.tint_soon(cx);
        editor
    }

    pub fn is_dirty(&self, cx: &gpui_kit::App) -> bool {
        *self.state.read(cx).value() != *self.saved
    }

    /// What's typed and not saved, and the file's text it was typed over.
    pub fn unsaved(&self, cx: &gpui_kit::App) -> Option<(String, String)> {
        self.is_dirty(cx).then(|| {
            (
                self.state.read(cx).value().to_string(),
                self.saved.to_string(),
            )
        })
    }

    /// The file changed on disk: shown as it is now, unless there's typing to keep.
    pub fn file_changed(
        &mut self,
        base: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.base = base.into();
        if !self.is_dirty(cx) && *self.saved != *text {
            self.saved = text.into();
            let text = text.to_string();
            self.state.update(cx, |s, cx| s.set_value(text, window, cx));
        }
        self.tint_soon(cx);
    }

    /// Replaces what's typed (tests, and the app's own edits), as typing would: set_value
    /// itself says nothing.
    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.state.update(cx, |s, cx| s.set_value(text, window, cx));
        self.tint_soon(cx);
        cx.emit(EditEvent::Changed);
    }

    /// Saving stops (Some) or goes on (None).
    pub fn refuse_saves(&mut self, why: Option<String>) {
        self.refused = why;
    }

    /// Writes what's typed, unless the file changed on disk since it was read: then it asks.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        if let Some(why) = &self.refused {
            cx.emit(EditEvent::Refused(why.clone()));
            return;
        }
        if !self.is_dirty(cx) {
            return;
        }
        self.write(false, window, cx);
    }

    fn write(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let text: Arc<str> = self.state.read(cx).value().as_ref().into();
        let expected = self.saved.clone();
        let (core, path, content) = (self.core.clone(), self.path.clone(), text.to_string());
        self.saving = true;
        self._save = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    // An agent may have written it meanwhile: its change would be lost.
                    if !force {
                        let now = gitviber_core::api::files::read_file(&core, path.clone())?;
                        if !now.exists || now.text != *expected {
                            return Ok(false);
                        }
                    }
                    gitviber_core::api::files::write_file(&core, path, content).map(|_| true)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(true) => {
                        this.saved = text;
                        cx.emit(EditEvent::Saved);
                    }
                    Ok(false) => this.ask_overwrite(window, cx),
                    Err(e) => cx.emit(EditEvent::Failed(e)),
                }
            })
            .ok();
        });
    }

    fn ask_overwrite(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let name = self
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&self.path)
            .to_string();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            alert
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .title("Changed on disk")
                .description(format!("{name} changed on disk since you started editing it. Overwrite it with your edits?"))
                .ok_text("Overwrite")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    weak.update(cx, |this, cx| this.write(true, window, cx)).ok();
                    true
                })
        });
    }

    fn tint_soon(&mut self, cx: &mut Context<Self>) {
        let base = self.base.clone();
        let text = self.state.read(cx).value();
        let color = DiffStyle::get(cx).colors.add_bg;
        self._tint = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let ranges = cx
                .background_spawn(async move { changed_lines(&base, &text) })
                .await;
            this.update(cx, |this, cx| {
                let tints = ranges
                    .into_iter()
                    .map(|r| {
                        RangeDecoration::new(r)
                            .with_style(RangeDecorationStyle::Fill)
                            .with_color(color)
                    })
                    .collect();
                this.tints.set(tints, cx);
            })
            .ok();
        });
    }
}

/// Byte ranges of `text`'s lines that `base` doesn't have as they are.
fn changed_lines(base: &str, text: &str) -> Vec<std::ops::Range<usize>> {
    let (rows, _) =
        gitviber_core::diff::rows(base, text, gitviber_core::diff::whitespace_mode(None));
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    rows.iter()
        .filter(|r| r.k == ADDED)
        .filter_map(|r| {
            let start = *starts.get(r.n as usize - 1)?;
            let end = starts.get(r.n as usize).copied().unwrap_or(text.len());
            Some(start..end)
        })
        .collect()
}

impl Render for DiffEditor {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(Editor::new(&self.state).size_full())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tints_the_lines_the_index_lacks() {
        assert_eq!(changed_lines("a\nb\nc\n", "a\nB\nc\nd\n"), vec![2..4, 6..8]);
        assert_eq!(
            changed_lines("a\n", "a\n"),
            Vec::<std::ops::Range<usize>>::new()
        );
    }
}
