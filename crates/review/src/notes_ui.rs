//! Review notes on screen: on a diff's lines (`attach_notes`: the composer C or a line's "+"
//! opens, and the open notes under their lines), and the worktree's list, a section of the
//! Changes panel (`NotesSection`), with what hands them to the agent.

use std::path::PathBuf;

use gitviber_diff::{Annotation, DiffEvent, DiffView, LinePos, LineRange, Side};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{PanelContext, ProjectEvent, Selection, Toast, Workspace};
use gitviber_ui::{ActiveAppTheme as _, AppTheme};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    rems,
};

use crate::model::notes::{
    Anchor, ReviewNote, Shown, anchor_at, for_terminal, is_live, note_lines, note_location,
    notes_prompt, place_notes,
};
use crate::note_store::ReviewNotes;

/// A side's lines as the diff holds them, for anchoring and placing notes.
fn lines_of(diff: &DiffView, side: Side) -> Option<Vec<String>> {
    let text = diff.side_text(side)?;
    Some((1..=text.lines).map(|n| text.line(n)).collect())
}

/// A note being written: its lines as they were picked, and the box it's typed in.
struct Draft {
    side: Side,
    anchor: Anchor,
    input: Entity<TextareaState>,
    _events: Subscription,
}

/// The review notes of one diff: drawn under their lines, and a new one's composer.
pub struct DiffNotes {
    diff: WeakEntity<DiffView>,
    root: String,
    /// "commit 1a2b3c4" for a commit's version; None for the files on disk.
    at: Option<String>,
    draft: Option<Draft>,
    /// Some notes were drawn: an update with none clears them.
    drawn: bool,
    _subscriptions: Vec<Subscription>,
    _live: Live,
}

/// Review notes on `diff`'s lines, in the open repository's worktree: C or a line's "+" writes
/// one. `at`: a commit's version ("commit 1a2b3c4"), whose lines never change.
pub fn attach_notes(
    diff: &Entity<DiffView>,
    at: Option<String>,
    cx: &mut App,
) -> Entity<DiffNotes> {
    // No repository open (or no core, in a view's own test): no notes.
    let root = gitviber_ui::has_core(cx)
        .then(|| gitviber_ui::core(cx).repo().ok())
        .flatten()
        .map(|r| r.to_string_lossy().into_owned())
        .unwrap_or_default();
    diff.update(cx, |d, cx| d.set_notes(!root.is_empty(), cx));
    cx.new(|cx| {
        let subscriptions = vec![
            cx.subscribe(diff, |this: &mut DiffNotes, _, e: &DiffEvent, cx| match e {
                DiffEvent::AddNote(range) => this.begin(*range, cx),
                DiffEvent::Loaded { .. } | DiffEvent::Edited => this.redraw(cx),
                _ => {}
            }),
            cx.observe_global::<ReviewNotes>(|this, cx| this.redraw(cx)),
        ];
        let mut notes = DiffNotes {
            diff: diff.downgrade(),
            root,
            at,
            draft: None,
            drawn: false,
            _subscriptions: subscriptions,
            _live: Live::new("DiffNotes"),
        };
        notes.redraw(cx);
        notes
    })
}

impl DiffNotes {
    /// A note on `range`: its composer under the range's last line. Its box needs the window,
    /// busy with the key that asked: it opens once that's done.
    fn begin(&mut self, range: LineRange, cx: &mut Context<Self>) {
        let Some(diff) = self.diff.upgrade() else {
            return;
        };
        let d = diff.read(cx);
        // Lines being typed into aren't the ones on disk.
        if d.is_editing() || self.root.is_empty() {
            return;
        }
        let Some(lines) = lines_of(d, range.side) else {
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
            let _ = window.update(cx, |_, window, cx| {
                let input = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .rows(3)
                        .placeholder("Review note… (⌘↵ to add, Esc to cancel)")
                });
                input.update(cx, |i, cx| i.focus(window, cx));
                let _ = this.update(cx, |this, cx| {
                    let events =
                        cx.subscribe(&input, |this: &mut DiffNotes, _, e: &InputEvent, cx| {
                            if let InputEvent::PressEnter {
                                secondary: true, ..
                            } = e
                            {
                                this.submit(cx);
                            }
                        });
                    this.draft = Some(Draft {
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

    #[doc(hidden)]
    pub fn drawn(&self) -> bool {
        self.drawn
    }

    #[doc(hidden)]
    pub fn write_draft(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = &self.draft {
            d.input
                .update(cx, |i, cx| i.set_value(text.to_string(), window, cx));
        }
        self.submit(cx);
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.redraw(cx);
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(diff) = self.diff.upgrade() else {
            return;
        };
        let Some(d) = &self.draft else { return };
        let body = d.input.read(cx).value().trim().to_string();
        if body.is_empty() {
            return;
        }
        let source = diff.read(cx).source().clone();
        let old = d.side == Side::Old;
        let note = ReviewNote {
            id: String::new(),
            path: if old {
                source.original_path().to_string()
            } else {
                source.path.clone()
            },
            body,
            anchor: d.anchor.clone(),
            old,
            at: self.at.clone(),
            outdated: false,
            resolved: false,
        };
        self.draft = None;
        ReviewNotes::add(cx, &self.root.clone(), note);
        self.redraw(cx);
    }

    /// The open notes under their lines, where the diff's versions have them, and the composer.
    fn redraw(&mut self, cx: &mut Context<Self>) {
        let Some(diff) = self.diff.upgrade() else {
            return;
        };
        let (path, old_path) = {
            let s = diff.read(cx).source();
            (s.path.clone(), s.original_path().to_string())
        };
        let notes: Vec<ReviewNote> = ReviewNotes::get(cx, &self.root)
            .into_iter()
            .filter(|n| !n.resolved && (n.path == path || n.path == old_path))
            // A commit's notes on its version, the disk's on the disk's.
            .filter(|n| n.at == self.at || (n.at.is_none() && self.at.is_none()))
            .collect();
        if notes.is_empty() && self.draft.is_none() {
            if self.drawn {
                self.drawn = false;
                diff.update(cx, |d, cx| d.set_annotations(vec![], cx));
            }
            return;
        }
        let t = cx.app_theme().clone();
        let (old_lines, new_lines) = {
            let d = diff.read(cx);
            (lines_of(d, Side::Old), lines_of(d, Side::New))
        };
        let shown = Shown {
            path: &path,
            old_path: &old_path,
            old_lines: old_lines.as_deref(),
            new_lines: new_lines.as_deref(),
        };
        let mut out: Vec<Annotation> = place_notes(&notes, &shown)
            .into_iter()
            .map(|p| {
                let side = if p.old { Side::Old } else { Side::New };
                let (note, root, t) = (p.note, self.root.clone(), t.clone());
                let pos = LinePos {
                    side,
                    line: p.end as u32,
                };
                Annotation::new(note.id.clone(), pos, px(64.), move |_, _| {
                    note_card(&note, &root, &t)
                })
            })
            .collect();
        if let Some(d) = &self.draft {
            let lines = if d.side == Side::Old {
                &old_lines
            } else {
                &new_lines
            };
            // Its lines where the file has them now; where they were, if they're gone.
            let end = lines
                .as_deref()
                .and_then(|l| crate::model::notes::find_note(l, &d.anchor, None))
                .map_or(d.anchor.end, |s| s + d.anchor.end - d.anchor.start);
            let (input, this, t) = (d.input.clone(), cx.weak_entity(), t.clone());
            let label = format!(
                "Note on {}{}",
                note_lines(&ReviewNote {
                    anchor: d.anchor.clone(),
                    ..Default::default()
                }),
                if d.side == Side::Old { " (old)" } else { "" }
            );
            let pos = LinePos {
                side: d.side,
                line: end as u32,
            };
            out.push(Annotation::new("draft", pos, px(120.), move |_, _| {
                composer(&label, &input, &this, &t)
            }));
        }
        self.drawn = true;
        diff.update(cx, |d, cx| d.set_annotations(out, cx));
    }
}

fn card_button(id: SharedString, icon: IconName, tip: &'static str) -> Button {
    Button::new(id).ghost().xsmall().icon(icon).tooltip(tip)
}

/// A note as it shows under its lines.
fn note_card(note: &ReviewNote, root: &str, t: &AppTheme) -> AnyElement {
    let (resolve, remove) = (
        (root.to_string(), note.id.clone()),
        (root.to_string(), note.id.clone()),
    );
    div()
        .px(px(12.))
        .py(px(4.))
        .child(
            div()
                .max_w(px(760.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.panel)
                .text_color(t.foreground)
                .font_family(t.fonts.ui.clone())
                .text_size(rems(0.75))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .pl(px(10.))
                        .pr(px(4.))
                        .pt(px(4.))
                        .child(
                            gpui_kit::component::Icon::new(IconName::MessageSquareText)
                                .xsmall()
                                .text_color(t.primary),
                        )
                        .child(
                            div()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child("Review note"),
                        )
                        .child(div().text_color(t.subtle).child(format!(
                            "on {}{}",
                            note_lines(note),
                            if note.old { " (old)" } else { "" }
                        )))
                        .child(div().flex_1())
                        .child(
                            card_button(
                                format!("resolve-{}", note.id).into(),
                                IconName::Check,
                                "Resolve note",
                            )
                            .on_click(move |_, _, cx| {
                                ReviewNotes::update(cx, &resolve.0, &resolve.1, |n| {
                                    n.resolved = true
                                })
                            }),
                        )
                        .child(
                            card_button(
                                format!("delete-{}", note.id).into(),
                                IconName::Trash,
                                "Delete note",
                            )
                            .on_click(move |_, _, cx| {
                                ReviewNotes::remove(cx, &remove.0, &remove.1)
                            }),
                        ),
                )
                .child(div().px(px(10.)).pb(px(8.)).child(note.body.clone())),
        )
        .into_any_element()
}

fn composer(
    label: &str,
    input: &Entity<TextareaState>,
    this: &WeakEntity<DiffNotes>,
    t: &AppTheme,
) -> AnyElement {
    let (cancel, add, escape) = (this.clone(), this.clone(), this.clone());
    div()
        .px(px(12.))
        .py(px(4.))
        .on_key_down(move |e: &gpui_kit::KeyDownEvent, _, cx| {
            if e.keystroke.key == "escape" {
                let _ = escape.update(cx, |this, cx| this.cancel(cx));
                cx.stop_propagation();
            }
        })
        .child(
            div()
                .max_w(px(760.))
                .p(px(8.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.panel)
                .font_family(t.fonts.ui.clone())
                .text_size(rems(0.75))
                .child(
                    div()
                        .text_color(t.muted_foreground)
                        .child(label.to_string()),
                )
                .child(Textarea::new(input).small())
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .child(
                            Button::new("note-cancel")
                                .small()
                                .ghost()
                                .label("Cancel")
                                .on_click(move |_, _, cx| {
                                    let _ = cancel.update(cx, |this, cx| this.cancel(cx));
                                }),
                        )
                        .child(
                            Button::new("note-add")
                                .small()
                                .primary()
                                .label("Add Note")
                                .on_click(move |_, _, cx| {
                                    let _ = add.update(cx, |this, cx| this.submit(cx));
                                }),
                        ),
                ),
        )
        .into_any_element()
}

fn toast(toast: Toast, window: &mut Window, cx: &mut App) {
    match Workspace::for_window(window, cx) {
        Some(ws) => ws.update(cx, |ws, cx| ws.toast(toast, cx)),
        None => toast.show(window, cx),
    }
}

/// The worktree open in `window`, whose notes its commands act on.
fn front_root(window: &Window, cx: &App) -> Option<String> {
    let ws = Workspace::for_window(window, cx)?;
    let project = ws.read(cx).project()?.clone();
    Some(project.read(cx).root().to_string_lossy().into_owned())
}

fn no_notes(window: &mut Window, cx: &mut App) {
    toast(
        Toast::info("Add one from a diff with its key (C) or a line's +.")
            .title("No open review notes"),
        window,
        cx,
    );
}

/// The notes as a prompt, on the clipboard.
pub fn copy_notes(notes: &[ReviewNote], window: &mut Window, cx: &mut App) {
    if notes.is_empty() {
        return no_notes(window, cx);
    }
    cx.write_to_clipboard(ClipboardItem::new_string(notes_prompt(notes)));
    let title = if notes.len() == 1 {
        "Note copied for the agent".to_string()
    } else {
        format!("{} notes copied for the agent", notes.len())
    };
    toast(
        Toast::success("Paste it into the agent's terminal.").title(title),
        window,
        cx,
    );
}

/// Into this worktree's terminal, where the agent runs; Enter is left to you.
pub fn send_notes(root: &str, notes: &[ReviewNote], window: &mut Window, cx: &mut App) {
    if notes.is_empty() {
        return no_notes(window, cx);
    }
    let text = for_terminal(&notes_prompt(notes));
    if !crate::agent::paste_to_terminal(&PathBuf::from(root), text, window, cx) {
        toast(
            Toast::info("Open one and start the agent in it, or copy the notes instead.")
                .title("No terminal in this worktree"),
            window,
            cx,
        );
    }
}

fn pending(root: &str, cx: &mut App) -> Vec<ReviewNote> {
    ReviewNotes::ordered(cx, root)
        .into_iter()
        .filter(|n| !n.resolved)
        .collect()
}

/// Copy Review Notes as Prompt, in the window in front.
pub fn copy_pending(window: &mut Window, cx: &mut App) {
    let notes = front_root(window, cx)
        .map(|r| pending(&r, cx))
        .unwrap_or_default();
    copy_notes(&notes, window, cx);
}

/// Send Review Notes to Terminal, in the window in front.
pub fn send_pending(window: &mut Window, cx: &mut App) {
    let Some(root) = front_root(window, cx) else {
        return;
    };
    let notes = pending(&root, cx);
    send_notes(&root, &notes, window, cx);
}

/// The worktree's review notes, a section of Changes: open ones first, each file's in line order,
/// with what hands them to the agent. Nothing shows while there are none.
pub struct NotesSection {
    workspace: WeakEntity<Workspace>,
    root: String,
    _check: Task<()>,
    _subscriptions: Vec<Subscription>,
    _live: Live,
}

impl NotesSection {
    pub fn new(ctx: &PanelContext, cx: &mut Context<Self>) -> Self {
        let root = ctx
            .project
            .as_ref()
            .map(|p| p.read(cx).root().to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut subscriptions = vec![cx.observe_global::<ReviewNotes>(|_, cx| cx.notify())];
        if let Some(project) = &ctx.project {
            subscriptions.push(cx.subscribe(
                project,
                |this, project, e: &ProjectEvent, cx| match e {
                    // On every change on disk, the notes on the files there move with their lines.
                    ProjectEvent::RepoChanged(_) => {
                        this._check = ReviewNotes::check(cx, &this.root);
                    }
                    ProjectEvent::Opened | ProjectEvent::Switched { .. } => {
                        this.root = project.read(cx).root().to_string_lossy().into_owned();
                        this._check = ReviewNotes::check(cx, &this.root);
                        cx.notify();
                    }
                    ProjectEvent::Failed(_) => {}
                },
            ));
        }
        let check = if root.is_empty() {
            Task::ready(())
        } else {
            ReviewNotes::check(cx, &root)
        };
        NotesSection {
            workspace: ctx.workspace.clone(),
            root,
            _check: check,
            _subscriptions: subscriptions,
            _live: Live::new("NotesSection"),
        }
    }

    /// Its lines in the file on disk; else the file, which has the version it was written on.
    fn go_to(&self, note: &ReviewNote, window: &mut Window, cx: &mut App) {
        let Some(ws) = self.workspace.upgrade() else {
            return;
        };
        if !is_live(note) && note.at.is_some() {
            return toast(
                Toast::info(format!(
                    "It's on {}: open that commit in History.",
                    note.at.clone().unwrap_or_default()
                ))
                .title(note_location(note)),
                window,
                cx,
            );
        }
        let path = note.path.clone();
        ws.update(cx, |ws, cx| {
            ws.open(Selection::File { path }, false, window, cx)
        });
    }
}

fn badge(text: impl Into<SharedString>, color: gpui_kit::Hsla, t: &AppTheme) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(4.))
        .rounded(px(3.))
        .bg(t.elevated)
        .font_family(t.fonts.code.clone())
        .text_size(rems(0.62))
        .text_color(color)
        .child(text.into())
}

impl Render for NotesSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notes = if self.root.is_empty() {
            vec![]
        } else {
            ReviewNotes::ordered(cx, &self.root)
        };
        if notes.is_empty() {
            return div().into_any_element();
        }
        let t = cx.app_theme().clone();
        let open = notes.iter().filter(|n| !n.resolved).count();
        let resolved = notes.len() - open;
        let root = self.root.clone();
        let header = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(12.))
            .h(px(28.))
            .text_size(rems(0.72))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .text_color(t.muted_foreground)
            .child("REVIEW NOTES")
            .child(div().text_color(t.subtle).child(open.to_string()))
            .child(div().flex_1())
            .when(open > 0, |d| {
                let (r1, r2) = (root.clone(), root.clone());
                d.child(
                    Button::new("notes-copy")
                        .ghost()
                        .xsmall()
                        .label("Copy as Prompt")
                        .on_click(move |_, window, cx| {
                            let notes = pending(&r1, cx);
                            copy_notes(&notes, window, cx)
                        }),
                )
                .child(
                    Button::new("notes-send")
                        .ghost()
                        .xsmall()
                        .label("Send to Terminal")
                        .on_click(move |_, window, cx| {
                            let notes = pending(&r2, cx);
                            send_notes(&r2, &notes, window, cx)
                        }),
                )
            })
            .when(resolved > 0, |d| {
                let r = root.clone();
                d.child(
                    Button::new("notes-clear")
                        .ghost()
                        .xsmall()
                        .label("Clear Resolved")
                        .on_click(move |_, _, cx| ReviewNotes::clear_resolved(cx, &r)),
                )
            });
        let rows = notes.into_iter().map(|n| {
            let (id, resolved) = (n.id.clone(), n.resolved);
            let (r_copy, r_res, r_del) = (n.clone(), root.clone(), root.clone());
            let (id_res, id_del) = (id.clone(), id.clone());
            let go = n.clone();
            div()
                .id(SharedString::from(format!("note-{id}")))
                .group("note-row")
                .flex()
                .items_start()
                .gap(px(8.))
                .px(px(12.))
                .py(px(4.))
                .text_size(rems(0.75))
                .hover(|s| s.bg(t.hover))
                .child(
                    div().pt(px(2.)).child(
                        gpui_kit::component::Icon::new(IconName::MessageSquareText)
                            .xsmall()
                            .text_color(if resolved { t.subtle } else { t.primary }),
                    ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("note-go-{id}")))
                        .flex_1()
                        .min_w_0()
                        .cursor_pointer()
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.go_to(&go, window, cx)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .font_family(t.fonts.code.clone())
                                        .text_size(rems(0.68))
                                        .text_color(t.muted_foreground)
                                        .child(note_location(&n)),
                                )
                                .when(n.old, |d| d.child(badge("old", t.subtle, &t)))
                                .when_some(n.at.clone(), |d, at| d.child(badge(at, t.subtle, &t)))
                                .when(n.outdated && !resolved, |d| {
                                    d.child(badge("outdated", t.modified, &t))
                                }),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_color(if resolved { t.subtle } else { t.foreground })
                                .when(resolved, |d| d.line_through())
                                .child(n.body.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .invisible()
                        .group_hover("note-row", |s| s.visible())
                        .when(!resolved, |d| {
                            d.child(
                                card_button(
                                    format!("note-copy-{id}").into(),
                                    IconName::Copy,
                                    "Copy as prompt",
                                )
                                .on_click(move |_, window, cx| {
                                    copy_notes(std::slice::from_ref(&r_copy), window, cx)
                                }),
                            )
                        })
                        .child(
                            card_button(
                                format!("note-resolve-{id}").into(),
                                if resolved {
                                    IconName::Undo2
                                } else {
                                    IconName::Check
                                },
                                if resolved {
                                    "Reopen note"
                                } else {
                                    "Resolve note"
                                },
                            )
                            .on_click(move |_, _, cx| {
                                ReviewNotes::update(cx, &r_res, &id_res, |n| n.resolved = !resolved)
                            }),
                        )
                        .child(
                            card_button(
                                format!("note-delete-{id}").into(),
                                IconName::Trash,
                                "Delete note",
                            )
                            .on_click(move |_, _, cx| ReviewNotes::remove(cx, &r_del, &id_del)),
                        ),
                )
        });
        div()
            .id("review-notes")
            .flex_none()
            .max_h(px(220.))
            .overflow_y_scroll()
            .border_t_1()
            .border_color(t.border)
            .child(header)
            .children(rows)
            .into_any_element()
    }
}
