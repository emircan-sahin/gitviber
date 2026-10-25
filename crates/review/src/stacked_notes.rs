//! Review notes on Open All's diffs of the files on disk: C or a line's "+" opens the composer
//! under the picked lines, and each file's open notes show under their lines once it's read.

use std::collections::HashSet;
use std::rc::Rc;

use gitviber_diff::model::SideText;
use gitviber_diff::{Annotation, DiffSource, LinePos, LineRange, Side, StackedDiff, StackedEvent};
use gitviber_ui::memory::Live;
use gitviber_ui::{ActiveAppTheme as _, AppTheme};
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::{App, AppContext as _, Context, Entity, Focusable as _, Subscription, WeakEntity};

use crate::model::notes::{
    Anchor, ReviewNote, Shown, anchor_at, find_note, note_lines, place_notes,
};
use crate::note_store::ReviewNotes;
use crate::notes_ui::{
    COMPOSER_HEIGHT, OnNote, composer, focus_soon, note_card, note_card_height, note_input,
};

fn lines_of(side: &SideText) -> Option<Vec<String>> {
    side.exists
        .then(|| (1..=side.lines).map(|n| side.line(n)).collect())
}

/// A note being written: its file, its lines as they were picked, and the box it's typed in.
struct Draft {
    source: DiffSource,
    side: Side,
    anchor: Anchor,
    input: Entity<TextareaState>,
    _events: Subscription,
}

/// The review notes of one Open All: drawn under their lines, and a new one's composer.
pub struct StackedNotes {
    stacked: WeakEntity<StackedDiff>,
    root: String,
    draft: Option<Draft>,
    /// The files with notes, by key: one left with none is cleared.
    noted: HashSet<String>,
    /// The view's reads when the notes were last placed: a file read since places them again.
    reads: u64,
    _subscriptions: Vec<Subscription>,
    _live: Live,
}

/// Review notes on `stacked`'s lines, the open worktree's files on disk. Lives as long as its
/// owner keeps it.
pub fn attach_stacked_notes(stacked: &Entity<StackedDiff>, cx: &mut App) -> Entity<StackedNotes> {
    let root = crate::code_notes::repo_root(cx).unwrap_or_default();
    stacked.update(cx, |s, cx| s.set_notes(!root.is_empty(), cx));
    cx.new(|cx| {
        let subscriptions = vec![
            cx.subscribe(
                stacked,
                |this: &mut StackedNotes, _, e: &StackedEvent, cx| {
                    if let StackedEvent::AddNote { source, range } = e {
                        this.begin(source.clone(), *range, cx);
                    }
                },
            ),
            // A file read (scrolled near, or changed on disk): its notes find their lines.
            cx.observe(stacked, |this, stacked, cx| {
                if stacked.read(cx).reads() != this.reads {
                    this.redraw(cx);
                }
            }),
            cx.observe_global::<ReviewNotes>(|this, cx| this.redraw(cx)),
        ];
        let mut notes = StackedNotes {
            stacked: stacked.downgrade(),
            root,
            draft: None,
            noted: HashSet::new(),
            reads: 0,
            _subscriptions: subscriptions,
            _live: Live::new("StackedNotes"),
        };
        notes.redraw(cx);
        notes
    })
}

impl StackedNotes {
    /// A note on `range` of `source`'s file: its composer under the range's last line. Its box
    /// needs the window, busy with the key that asked: it opens once that's done.
    fn begin(&mut self, source: DiffSource, range: LineRange, cx: &mut Context<Self>) {
        let Some(stacked) = self.stacked.upgrade() else {
            return;
        };
        let lines = stacked
            .read(cx)
            .data(&source.key())
            .and_then(|d| lines_of(d.side(range.side)));
        let (Some(lines), false) = (lines, self.root.is_empty()) else {
            return;
        };
        let end = (range.end as usize).min(lines.len()).max(1);
        let start = (range.start as usize).clamp(1, end);
        let anchor = anchor_at(&lines, start, end);
        let side = range.side;
        // The key's window; the first one when none is in front (a test's).
        let window = cx.active_window().or_else(|| cx.windows().first().copied());
        let (Some(window), this) = (window, cx.weak_entity()) else {
            return;
        };
        cx.defer(move |cx| {
            _ = window.update(cx, |_, window, cx| {
                let input = cx.new(|cx| note_input(window, cx));
                input.update(cx, |i, cx| i.focus(window, cx));
                _ = this.update(cx, |this, cx| {
                    let events =
                        cx.subscribe(&input, |this: &mut StackedNotes, _, e: &InputEvent, cx| {
                            if let InputEvent::PressEnter {
                                secondary: true, ..
                            } = e
                            {
                                this.submit(cx);
                            }
                        });
                    // One composer at a time: another file's goes.
                    this.draft = Some(Draft {
                        source,
                        side,
                        anchor,
                        input,
                        _events: events,
                    });
                    this.redraw(cx);
                });
            });
        });
    }

    // For tests.
    #[doc(hidden)]
    pub fn has_draft(&self) -> bool {
        self.draft.is_some()
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.redraw(cx);
        self.refocus(cx);
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.draft else { return };
        let body = d.input.read(cx).value().trim().to_string();
        if body.is_empty() {
            return;
        }
        let old = d.side == Side::Old;
        let note = ReviewNote {
            path: if old {
                d.source.original_path().to_string()
            } else {
                d.source.path.clone()
            },
            body,
            anchor: d.anchor.clone(),
            old,
            ..Default::default()
        };
        self.draft = None;
        ReviewNotes::add(cx, &self.root.clone(), note);
        self.redraw(cx);
        self.refocus(cx);
    }

    fn refocus(&self, cx: &mut Context<Self>) {
        if let Some(stacked) = self.stacked.upgrade() {
            focus_soon(stacked.read(cx).focus_handle(cx), cx);
        }
    }

    /// Each noted file's open notes under their lines, where its versions have them, and the
    /// composer. A file not read keeps what it was given last.
    fn redraw(&mut self, cx: &mut Context<Self>) {
        let Some(stacked) = self.stacked.upgrade() else {
            return;
        };
        let notes: Vec<ReviewNote> = if self.root.is_empty() {
            vec![]
        } else {
            ReviewNotes::get(cx, &self.root)
                .into_iter()
                .filter(|n| !n.resolved && n.at.is_none())
                .collect()
        };
        let t = cx.app_theme().clone();
        let paths: HashSet<&str> = notes.iter().map(|n| n.path.as_str()).collect();
        let mut noted = HashSet::new();
        let mut placed: Vec<(String, Vec<Annotation>)> = vec![];
        let s = stacked.read(cx);
        self.reads = s.reads();
        for file in s.files() {
            let source = &file.source;
            let (path, old_path) = (source.path.as_str(), source.original_path());
            let key = source.key();
            let draft = self.draft.as_ref().filter(|d| d.source.key() == key);
            if draft.is_none() && !paths.contains(path) && !paths.contains(old_path) {
                continue;
            }
            noted.insert(key.clone());
            let Some(data) = s.data(&key) else { continue };
            let (old_lines, new_lines) = (lines_of(&data.old), lines_of(&data.new));
            let mine: Vec<ReviewNote> = notes
                .iter()
                .filter(|n| n.path == path || n.path == old_path)
                .cloned()
                .collect();
            let shown = Shown {
                path,
                old_path,
                old_lines: old_lines.as_deref(),
                new_lines: new_lines.as_deref(),
            };
            let mut out: Vec<Annotation> = place_notes(&mine, &shown)
                .into_iter()
                .map(|p| {
                    let side = if p.old { Side::Old } else { Side::New };
                    let (note, root, t) = (p.note, self.root.clone(), t.clone());
                    let pos = LinePos {
                        side,
                        line: p.end as u32,
                    };
                    let height = note_card_height(&note, &t, cx);
                    Annotation::new(note.id.clone(), pos, height, move |_, _| {
                        note_card(&note, &root, &t)
                    })
                })
                .collect();
            if let Some(d) = draft {
                let lines = if d.side == Side::Old {
                    &old_lines
                } else {
                    &new_lines
                };
                out.push(self.composer(d, lines.as_deref(), &t, cx.weak_entity()));
            }
            placed.push((key, out));
        }
        let gone: Vec<String> = self.noted.difference(&noted).cloned().collect();
        self.noted = noted;
        if placed.is_empty() && gone.is_empty() {
            return;
        }
        stacked.update(cx, |s, cx| {
            for (key, notes) in placed {
                s.set_annotations(&key, notes, cx);
            }
            for key in gone {
                s.set_annotations(&key, vec![], cx);
            }
        });
    }

    /// The draft's box under its lines where the file has them now; where they were, if they're
    /// gone.
    fn composer(
        &self,
        d: &Draft,
        lines: Option<&[String]>,
        t: &AppTheme,
        this: WeakEntity<Self>,
    ) -> Annotation {
        let end = lines
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
        let (cancel, add) = (this.clone(), this);
        let on_cancel: OnNote = Rc::new(move |cx| {
            _ = cancel.update(cx, |this, cx| this.cancel(cx));
        });
        let on_add: OnNote = Rc::new(move |cx| {
            _ = add.update(cx, |this, cx| this.submit(cx));
        });
        let (input, t) = (d.input.clone(), t.clone());
        let pos = LinePos {
            side: d.side,
            line: end as u32,
        };
        Annotation::new("draft", pos, COMPOSER_HEIGHT, move |_, _| {
            composer(&label, &input, on_cancel.clone(), on_add.clone(), &t)
        })
    }
}
