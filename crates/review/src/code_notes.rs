//! Review notes on a repo file's own view: its open notes on the file on disk drawn under their
//! lines as blocks of the code editor, and Add Review Note…'s composer there. Typing carries the
//! blocks with their lines; a save moves the notes where it left them.

use std::collections::HashMap;
use std::rc::Rc;

use gitviber_files::{CodeBlock, CodeEvent, CodeView};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::Toast;
use gpui_kit::component::input::{InputEvent, RopeExt as _, TextareaState};
use gpui_kit::{App, AppContext as _, Context, Entity, EntityId, Subscription, WeakEntity};

use crate::model::notes::{Anchor, ReviewNote, anchor_at, find_note, is_live, note_lines};
use crate::note_store::ReviewNotes;
use crate::notes_ui::{COMPOSER_HEIGHT, OnNote, composer, note_card, note_card_height, note_input};

/// The composer's block id; notes' blocks go by their own.
const DRAFT: &str = "draft";

/// A note being written: its lines as they were picked, and the box it's typed in.
struct Draft {
    anchor: Anchor,
    input: Entity<TextareaState>,
    _events: Subscription,
}

/// The blocks as they were set, in their order: the notes', then the composer's.
#[derive(Default, PartialEq)]
struct Shown {
    notes: Vec<(ReviewNote, u32)>,
    /// Its line, label and box.
    draft: Option<(u32, String, EntityId)>,
}

impl Shown {
    fn ids(&self) -> impl Iterator<Item = &str> {
        self.notes
            .iter()
            .map(|(n, _)| n.id.as_str())
            .chain(self.draft.as_ref().map(|_| DRAFT))
    }
}

/// The review notes of one code view: drawn under their lines, and a new one's composer.
pub struct CodeNotes {
    code: WeakEntity<CodeView>,
    /// In the order `CodeView::block_lines` answers in.
    shown: Shown,
    draft: Option<Draft>,
    _subscriptions: Vec<Subscription>,
    _live: Live,
}

/// The open worktree's root, whose notes these are; None without a repository.
pub(crate) fn repo_root(cx: &App) -> Option<String> {
    gitviber_ui::has_core(cx)
        .then(|| gitviber_ui::core(cx).repo().ok())
        .flatten()
        .map(|r| r.to_string_lossy().into_owned())
        .filter(|r| !r.is_empty())
}

fn lines_of(code: &Entity<CodeView>, cx: &App) -> Vec<String> {
    code.read(cx)
        .text(cx)
        .iter_lines()
        .map(|l| l.to_string())
        .collect()
}

/// Notes on `code`'s lines, the file's in the open worktree. Lives as long as the code view.
pub(crate) fn attach(code: &Entity<CodeView>, cx: &mut App) -> Option<Entity<CodeNotes>> {
    repo_root(cx)?;
    code.update(cx, |c, _| c.set_notes(true));
    Some(cx.new(|cx| {
        let subscriptions = vec![
            cx.subscribe(code, |this: &mut CodeNotes, _, e: &CodeEvent, cx| match e {
                CodeEvent::AddNote { lines } => this.begin(*lines, cx),
                CodeEvent::Saved => this.saved(cx),
                CodeEvent::Reloaded => this.redraw(None, cx),
                CodeEvent::DirtyChanged => {}
            }),
            cx.observe_global::<ReviewNotes>(|this, cx| this.redraw(None, cx)),
        ];
        let mut notes = CodeNotes {
            code: code.downgrade(),
            shown: Shown::default(),
            draft: None,
            _subscriptions: subscriptions,
            _live: Live::new("CodeNotes"),
        };
        notes.redraw(None, cx);
        notes
    }))
}

impl CodeNotes {
    /// Each block's line now, by id: typing moves them with their lines.
    fn carried(&self, code: &Entity<CodeView>, cx: &App) -> HashMap<String, u32> {
        self.shown
            .ids()
            .map(str::to_string)
            .zip(code.read(cx).block_lines(cx))
            .collect()
    }

    /// A note on `lines`: its composer under the last. Its box needs the window, busy with the
    /// key that asked: it opens once that's done.
    fn begin(&mut self, (start, end): (u32, u32), cx: &mut Context<Self>) {
        let Some(code) = self.code.upgrade() else {
            return;
        };
        // The key's window; the first one when none is in front (a test's).
        let Some(window) = cx.active_window().or_else(|| cx.windows().first().copied()) else {
            return;
        };
        // A note is on the file's lines on disk, which typing has left behind.
        if code.read(cx).is_dirty() {
            let toast =
                Toast::info("A note goes on the saved file's lines.").title("Save the file first");
            cx.defer(move |cx| {
                _ = window.update(cx, |_, window, cx| {
                    crate::notes_ui::toast(toast, window, cx)
                });
            });
            return;
        }
        let lines = lines_of(&code, cx);
        let end = (end as usize).min(lines.len()).max(1);
        let start = (start as usize).clamp(1, end);
        let anchor = anchor_at(&lines, start, end);
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            _ = window.update(cx, |_, window, cx| {
                let input = cx.new(|cx| note_input(window, cx));
                input.update(cx, |i, cx| i.focus(window, cx));
                _ = this.update(cx, |this, cx| {
                    let events =
                        cx.subscribe(&input, |this: &mut CodeNotes, _, e: &InputEvent, cx| {
                            if let InputEvent::PressEnter {
                                secondary: true, ..
                            } = e
                            {
                                this.submit(cx);
                            }
                        });
                    this.draft = Some(Draft {
                        anchor,
                        input,
                        _events: events,
                    });
                    this.redraw(None, cx);
                });
            });
        });
    }

    // For tests.
    #[cfg(test)]
    pub(crate) fn has_draft(&self) -> bool {
        self.draft.is_some()
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.redraw(None, cx);
        self.refocus(cx);
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let (Some(code), Some(root)) = (self.code.upgrade(), repo_root(cx)) else {
            return;
        };
        let Some(d) = &self.draft else { return };
        let body = d.input.read(cx).value().trim().to_string();
        if body.is_empty() {
            return;
        }
        let note = ReviewNote {
            path: code.read(cx).source().path.clone(),
            body,
            anchor: d.anchor.clone(),
            ..Default::default()
        };
        // Typing since it opened may have carried the composer: the note takes its place.
        let line = self.carried(&code, cx).get(DRAFT).copied();
        self.draft = None;
        let id = ReviewNotes::add(cx, &root, note);
        self.redraw(line.map(|l| (id, l)), cx);
        self.refocus(cx);
    }

    /// The keys back to the code once the composer goes.
    fn refocus(&self, cx: &mut Context<Self>) {
        if let Some(code) = self.code.upgrade() {
            crate::notes_ui::focus_soon(code.read(cx).editor_focus(cx), cx);
        }
    }

    /// The text typed into is on disk now: the notes (and the composer) take the lines the
    /// typing carried them to.
    fn saved(&mut self, cx: &mut Context<Self>) {
        let Some(code) = self.code.upgrade() else {
            return;
        };
        // Typing raced the save: what's shown isn't what was written.
        if code.read(cx).is_dirty() {
            return;
        }
        let carried = self.carried(&code, cx);
        if carried.is_empty() {
            return;
        }
        let lines = lines_of(&code, cx);
        if let (Some(d), Some(&end)) = (&mut self.draft, carried.get(DRAFT)) {
            let end = (end as usize).min(lines.len()).max(1);
            let start = (end + d.anchor.start).saturating_sub(d.anchor.end).max(1);
            d.anchor = anchor_at(&lines, start, end);
        }
        if let Some(root) = repo_root(cx) {
            let path = code.read(cx).source().path.clone();
            let at = carried
                .into_iter()
                .filter(|(id, _)| id != DRAFT)
                .map(|(id, l)| (id, l as usize))
                .collect();
            ReviewNotes::follow(cx, &root, &path, &lines, &at);
        }
        self.redraw(None, cx);
    }

    /// The open notes on the file on disk under their lines, and the composer. `added`: a note
    /// just written, and the line its composer was carried to.
    fn redraw(&mut self, added: Option<(String, u32)>, cx: &mut Context<Self>) {
        let Some(code) = self.code.upgrade() else {
            return;
        };
        let root = repo_root(cx).unwrap_or_default();
        let (path, dirty) = {
            let c = code.read(cx);
            (c.source().path.clone(), c.is_dirty())
        };
        let notes: Vec<ReviewNote> = if root.is_empty() {
            vec![]
        } else {
            ReviewNotes::get(cx, &root)
                .into_iter()
                .filter(|n| is_live(n) && !n.resolved && n.path == path)
                .collect()
        };
        let mut next = Shown::default();
        if !notes.is_empty() || self.draft.is_some() {
            // Typed into, the text isn't the file's: the blocks drawn stay where typing took them.
            let mut carried = if dirty {
                self.carried(&code, cx)
            } else {
                HashMap::new()
            };
            carried.extend(added);
            let lines = lines_of(&code, cx);
            next.notes = notes
                .into_iter()
                .filter_map(|n| {
                    let line = carried.get(&n.id).copied().or_else(|| {
                        find_note(&lines, &n.anchor, None)
                            .map(|s| (s + n.anchor.end - n.anchor.start) as u32)
                    })?;
                    Some((n, line))
                })
                .collect();
            next.draft = self.draft.as_ref().map(|d| {
                // Its lines where the file has them now; where they were, if they're gone.
                let line = carried.get(DRAFT).copied().unwrap_or_else(|| {
                    find_note(&lines, &d.anchor, None)
                        .map_or(d.anchor.end, |s| s + d.anchor.end - d.anchor.start)
                        as u32
                });
                let label = format!(
                    "Note on {}",
                    note_lines(&ReviewNote {
                        anchor: d.anchor.clone(),
                        ..Default::default()
                    })
                );
                (line, label, d.input.entity_id())
            });
        }
        if next == self.shown {
            return;
        }
        let t = cx.app_theme();
        let mut blocks: Vec<CodeBlock> = next
            .notes
            .iter()
            .map(|(n, line)| {
                let (note, root) = (n.clone(), root.clone());
                CodeBlock::new(
                    n.id.clone(),
                    *line,
                    note_card_height(n, t, cx),
                    move |_, cx| note_card(&note, &root, cx.app_theme()),
                )
            })
            .collect();
        if let (Some(d), Some((line, label, _))) = (&self.draft, &next.draft) {
            let (input, label, this) = (d.input.clone(), label.clone(), cx.weak_entity());
            let (cancel, add) = (this.clone(), this);
            let on_cancel: OnNote = Rc::new(move |cx| {
                _ = cancel.update(cx, |this, cx| this.cancel(cx));
            });
            let on_add: OnNote = Rc::new(move |cx| {
                _ = add.update(cx, |this, cx| this.submit(cx));
            });
            blocks.push(CodeBlock::new(
                DRAFT,
                *line,
                COMPOSER_HEIGHT,
                move |_, cx| {
                    composer(
                        &label,
                        &input,
                        on_cancel.clone(),
                        on_add.clone(),
                        cx.app_theme(),
                    )
                },
            ));
        }
        self.shown = next;
        code.update(cx, |c, cx| c.set_blocks(blocks, cx));
    }
}

#[cfg(test)]
mod tests;
