//! The user's review notes on a commit guide's diffs, on the commit's version, as History's commit
//! view takes them: C or a line's "+" in the diffs opens the composer; the open notes show under
//! their lines beside the agent's. A range's guide (a branch, a PR, the uncommitted changes) takes
//! none, as the old app's Open All did.

use std::collections::HashMap;
use std::rc::Rc;

use gitviber_core::api;
use gitviber_diff::{Annotation, DiffSource, LinePos, LineRange, Side};
use gitviber_ui::ActiveAppTheme as _;
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::{AppContext as _, Context, Entity, Subscription, Task, Window, px};

use super::GuideView;
use crate::model::notes::{
    Anchor, ReviewNote, Shown, anchor_at, find_note, note_lines, place_notes,
};
use crate::note_store::ReviewNotes;
use crate::notes_ui::{OnNote, composer, note_card};

/// A file's two sides, as lines; None for a side it doesn't have.
type Sides = (Option<Vec<String>>, Option<Vec<String>>);

/// A note being written on a file of the diffs.
pub(super) struct Draft {
    path: String,
    old_path: String,
    side: Side,
    anchor: Anchor,
    input: Entity<TextareaState>,
    _events: Subscription,
}

/// What the commit guide keeps for its notes: the noted files' sides as read (only theirs), and
/// the reads on their way.
#[derive(Default)]
pub(super) struct GuideNotes {
    pub(super) draft: Option<Draft>,
    lines: HashMap<String, Sides>,
    reads: HashMap<String, Task<()>>,
}

fn lines_of(text: gitviber_core::git::FileText) -> Option<Vec<String>> {
    text.exists
        .then(|| text.text.lines().map(str::to_string).collect())
}

impl GuideView {
    /// "commit 1a2b3c4" on a commit's guide: the version its notes are on. None for a range's.
    pub(super) fn notes_at(&self) -> Option<String> {
        match &self.of {
            gitviber_ui::workspace::GuideOf::Commit { commit } => {
                Some(format!("commit {}", commit.short_sha))
            }
            _ => None,
        }
    }

    /// Reads `source`'s two sides at the commit.
    fn read(source: &DiffSource, cx: &mut Context<Self>) -> Task<Option<Sides>> {
        let core = gitviber_ui::core(cx);
        let (path, old_path, sha) = (
            source.path.clone(),
            source.old_path.clone(),
            source.compare.sha().map(str::to_string),
        );
        cx.background_spawn(async move {
            let pair =
                api::files::diff_pair(&core, "commit".into(), path, old_path, sha, None, None)
                    .ok()?;
            Some((lines_of(pair.original), lines_of(pair.modified)))
        })
    }

    /// A noted file's sides, read once; its notes show when they land.
    fn read_sides(&mut self, source: &DiffSource, cx: &mut Context<Self>) {
        let read = Self::read(source, cx);
        let key = source.path.clone();
        let task = cx.spawn(async move |this, cx| {
            let sides = read.await;
            let _ = this.update(cx, |this, cx| {
                this.notes.reads.remove(&key);
                if let Some(sides) = sides {
                    this.notes.lines.insert(key, sides);
                    this.annotate(cx);
                }
            });
        });
        self.notes.reads.insert(source.path.clone(), task);
    }

    /// C on the diffs: the composer under the picked lines, once the file's side is read.
    pub(super) fn begin_note(
        &mut self,
        source: &DiffSource,
        range: LineRange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.notes_at().is_none() || self.root.as_os_str().is_empty() {
            return;
        }
        if !self.notes.lines.contains_key(&source.path) {
            let read = Self::read(source, cx);
            let key = source.path.clone();
            let source = source.clone();
            let task = cx.spawn_in(window, async move |this, cx| {
                let sides = read.await;
                let _ = this.update_in(cx, move |this, window, cx| {
                    this.notes.reads.remove(&source.path);
                    if let Some(sides) = sides {
                        this.notes.lines.insert(source.path.clone(), sides);
                        this.begin_note(&source, range, window, cx);
                    }
                });
            });
            self.notes.reads.insert(key, task);
            return;
        }
        let (old, new) = &self.notes.lines[&source.path];
        let Some(lines) = (if range.side == Side::Old { old } else { new }) else {
            return;
        };
        let end = (range.end as usize).min(lines.len()).max(1);
        let start = (range.start as usize).clamp(1, end);
        let anchor = anchor_at(lines, start, end);
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .placeholder("Review note… (⌘↵ to add, Esc to cancel)")
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        let events = cx.subscribe(&input, |this: &mut Self, _, e: &InputEvent, cx| {
            if let InputEvent::PressEnter {
                secondary: true, ..
            } = e
            {
                this.add_note(cx);
            }
        });
        self.notes.draft = Some(Draft {
            path: source.path.clone(),
            old_path: source.original_path().to_string(),
            side: range.side,
            anchor,
            input,
            _events: events,
        });
        self.annotate(cx);
    }

    fn cancel_note(&mut self, cx: &mut Context<Self>) {
        self.notes.draft = None;
        self.annotate(cx);
    }

    fn add_note(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.notes.draft else { return };
        let body = d.input.read(cx).value().trim().to_string();
        if body.is_empty() {
            return;
        }
        let old = d.side == Side::Old;
        let note = ReviewNote {
            path: if old {
                d.old_path.clone()
            } else {
                d.path.clone()
            },
            body,
            anchor: d.anchor.clone(),
            old,
            at: self.notes_at(),
            ..Default::default()
        };
        self.notes.draft = None;
        let root = self.root.to_string_lossy().into_owned();
        ReviewNotes::add(cx, &root, note);
        self.annotate(cx);
    }

    /// The user's open notes on `path` and the composer, where its sides have their lines; a
    /// noted file's sides are read the first time.
    pub(super) fn note_annotations(
        &mut self,
        path: &str,
        source: Option<&DiffSource>,
        cx: &mut Context<Self>,
    ) -> Vec<Annotation> {
        let Some(at) = self.notes_at() else {
            return vec![];
        };
        let root = self.root.to_string_lossy().into_owned();
        let old_path = source.map_or(path.to_string(), |s| s.original_path().to_string());
        let notes: Vec<ReviewNote> = ReviewNotes::get(cx, &root)
            .into_iter()
            .filter(|n| !n.resolved && n.at.as_deref() == Some(at.as_str()))
            .filter(|n| n.path == path || n.path == old_path)
            .collect();
        let drafting = self.notes.draft.as_ref().is_some_and(|d| d.path == path);
        if notes.is_empty() && !drafting {
            return vec![];
        }
        let Some((old_lines, new_lines)) = self.notes.lines.get(path).cloned() else {
            if let Some(source) = source
                && !self.notes.reads.contains_key(path)
            {
                self.read_sides(source, cx);
            }
            return vec![];
        };
        let t = cx.app_theme().clone();
        let shown = Shown {
            path,
            old_path: &old_path,
            old_lines: old_lines.as_deref(),
            new_lines: new_lines.as_deref(),
        };
        let mut out: Vec<Annotation> = place_notes(&notes, &shown)
            .into_iter()
            .map(|p| {
                let side = if p.old { Side::Old } else { Side::New };
                let (note, root, t) = (p.note, root.clone(), t.clone());
                let pos = LinePos {
                    side,
                    line: p.end as u32,
                };
                Annotation::new(format!("note-{}", note.id), pos, px(64.), move |_, _| {
                    note_card(&note, &root, &t)
                })
            })
            .collect();
        if let Some(d) = self.notes.draft.as_ref().filter(|d| d.path == path) {
            let lines = if d.side == Side::Old {
                &old_lines
            } else {
                &new_lines
            };
            let end = lines
                .as_deref()
                .and_then(|l| find_note(l, &d.anchor, None))
                .map_or(d.anchor.end, |s| s + d.anchor.end - d.anchor.start);
            let label = format!(
                "Note on {}{}",
                note_lines(&ReviewNote {
                    anchor: d.anchor.clone(),
                    ..Default::default()
                }),
                if d.side == Side::Old { " (old)" } else { "" }
            );
            let input = d.input.clone();
            let (cancel, add) = (cx.weak_entity(), cx.weak_entity());
            let on_cancel: OnNote = Rc::new(move |cx| {
                let _ = cancel.update(cx, |this, cx| this.cancel_note(cx));
            });
            let on_add: OnNote = Rc::new(move |cx| {
                let _ = add.update(cx, |this, cx| this.add_note(cx));
            });
            let pos = LinePos {
                side: d.side,
                line: end as u32,
            };
            out.push(Annotation::new("note-draft", pos, px(120.), move |_, _| {
                composer(&label, &input, on_cancel.clone(), on_add.clone(), &t)
            }));
        }
        out
    }
}
