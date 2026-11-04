//! One file of a pull request: its diff from the merge base (or across the picked commits), with
//! the PR's line comments on it as GitHub threads them: each thread with its line (a click
//! scrolls the diff there) and a reply box, and C on picked lines for a new one. The picked
//! commits' lines aren't the head's, so they take no comments.

use std::sync::Arc;
use std::time::Duration;

use gitviber_core::api;
use gitviber_core::git::FileChange;
use gitviber_core::github::ReviewComment;
use gitviber_diff::{Compare, DiffEvent, DiffSource, DiffView, LinePos, LineRange, Side};
use gitviber_ui::actions::github::PostComment;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{
    PullRange, Selection, TabIcon, TabItem, Toast, Workspace, selection_key,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, Escape, InputEvent};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, rems,
};

use crate::cache::{Cache, Wakes};
use crate::model::{Target, repo_of};
use crate::ui::common::{Attachments, Blocks, ago, error_text, text_button, theme};
use crate::ui::markdown_input::MarkdownInput;

/// Comments are kept fresh a minute at a time while the file or its PR shows.
pub(crate) const AGE: Duration = Duration::from_secs(60);
/// Drafts kept aside for other threads and lines, the latest.
const DRAFTS: usize = 16;
const POST_KEY: &str = if cfg!(target_os = "macos") {
    "⌘↵"
} else {
    "Ctrl+Enter"
};

/// Where a comment box is: a new comment on lines, or a reply in a thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Draft {
    New(LineRange),
    Reply(u64),
}

struct Thread {
    root: usize,
    replies: Vec<usize>,
}

/// A PR's line comments, kept by the cache: one read serves its page (a count per file) and
/// every file tab of it.
pub(crate) fn comments_key(pull_url: &str) -> String {
    format!("pr-comments:{pull_url}")
}

pub(crate) fn load_comments(
    cx: &mut App,
    target: Target,
    number: u64,
    pull_url: &str,
    max_age: Duration,
) {
    Cache::load(cx, &comments_key(pull_url), max_age, move |core| {
        api::github::pr_review_comments(core, target.clone(), number)
    });
}

pub struct PullFileView {
    range: PullRange,
    file: Arc<FileChange>,
    target: Target,
    web: String,
    ws: WeakEntity<Workspace>,
    diff: Entity<DiffView>,
    /// The PR's comments as last read: this file's are taken from it when it changes.
    all: Option<Arc<Vec<ReviewComment>>>,
    comments: Arc<Vec<ReviewComment>>,
    /// Indexes into `comments`: this file's threads on a line now, then the outdated ones (the
    /// diff moved on past their line), shown folded.
    threads: Vec<Thread>,
    outdated: Vec<Thread>,
    outdated_open: bool,
    blocks: Blocks,
    text_stale: bool,
    attachments: Attachments,
    comments_error: Option<String>,
    draft: Option<Draft>,
    composer: Entity<MarkdownInput>,
    /// What was written for another thread or line, back when the box goes there again.
    drafts: Vec<(Draft, String)>,
    folded: bool,
    busy: bool,
    /// The box empties at the next frame, which has the window it takes.
    clear: bool,
    wakes: Wakes,
    focus: FocusHandle,
    _post: Task<()>,
    _signed: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Old => "LEFT",
        Side::New => "RIGHT",
    }
}

/// "line 4", "lines 2–4 (on 4)", "(old)" on the removed side.
fn lines_label(r: LineRange) -> String {
    let old = if r.side == Side::Old { " (old)" } else { "" };
    if r.start == r.end {
        format!("line {}{old}", r.end)
    } else {
        format!("lines {}–{} (on {}){old}", r.start, r.end, r.end)
    }
}

/// `drafts` with `text` kept for `draft` (an empty one forgotten), the latest last, bounded.
fn keep_draft(drafts: &mut Vec<(Draft, String)>, draft: Draft, text: String) {
    drafts.retain(|(d, _)| *d != draft);
    if !text.trim().is_empty() {
        drafts.push((draft, text));
    }
    let over = drafts.len().saturating_sub(DRAFTS);
    drafts.drain(..over);
}

impl PullFileView {
    pub fn new(
        ws: WeakEntity<Workspace>,
        range: PullRange,
        file: Arc<FileChange>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::ui::use_workspace_project(&ws, cx);
        let compare = Compare::Range {
            base: range.base.clone(),
            head: range.head.clone(),
        };
        let source = DiffSource::from_change(compare, &file);
        let reviewable = range.pull_url.is_some() && range.number.is_some();
        let diff = cx.new(|cx| {
            let mut d = DiffView::new(source, cx);
            d.set_notes(reviewable, cx);
            d
        });
        let target = range.pull_url.as_deref().map(repo_of);
        let web = target
            .as_ref()
            .map(|t| format!("https://github.com/{t}"))
            .unwrap_or_default();
        let page = range.pull_url.clone().unwrap_or_default();
        let composer = cx.new(|cx| {
            let input = MarkdownInput::new(web.clone(), page, window, cx, |t| {
                t.rows(3)
                    .placeholder(format!("Leave a comment (markdown; {POST_KEY} to post)"))
            })
            .small();
            match range.number {
                Some(n) => input.signed_by(target.clone(), n),
                None => input,
            }
        });
        let input = composer.read(cx).input().clone();
        let subs = vec![
            cx.subscribe_in(
                &diff,
                window,
                |this: &mut Self, _, ev: &DiffEvent, window, cx| {
                    if let DiffEvent::AddNote(lines) = ev {
                        this.start(Draft::New(*lines), window, cx);
                    }
                },
            ),
            // Text in the box keeps the tab: a preview would be replaced, a hidden tab released.
            cx.subscribe(&input, |this: &mut Self, c, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    if !c.read(cx).value().trim().is_empty() {
                        let key = this.key();
                        this.ws.update(cx, |ws, cx| ws.pin_tab(&key, cx)).ok();
                    }
                    cx.notify();
                }
            }),
            cx.observe_global::<Cache>(|this: &mut Self, cx| {
                if this.sync(cx) || Cache::wakes(cx) != this.wakes {
                    cx.notify();
                }
            }),
        ];
        let mut view = PullFileView {
            range,
            file,
            target,
            web,
            ws,
            diff,
            all: None,
            comments: Arc::new(vec![]),
            threads: vec![],
            outdated: vec![],
            outdated_open: false,
            blocks: Blocks::default(),
            text_stale: false,
            attachments: Attachments::default(),
            comments_error: None,
            draft: None,
            composer,
            drafts: vec![],
            folded: false,
            busy: false,
            clear: false,
            wakes: Cache::wakes(cx),
            focus: cx.focus_handle(),
            _post: Task::ready(()),
            _signed: Task::ready(()),
            _subs: subs,
            _live: Live::new("PullFileView"),
        };
        view.load(false, cx);
        view
    }

    pub fn range(&self) -> &PullRange {
        &self.range
    }

    fn key(&self) -> String {
        selection_key(&Selection::PrFile {
            range: self.range.clone(),
            file: self.file.clone(),
        })
    }

    /// Asks for the PR's comments unless the cache has them younger than `AGE` (`force`: now).
    fn load(&mut self, force: bool, cx: &mut Context<Self>) {
        let (Some(number), Some(url)) = (self.range.number, self.range.pull_url.clone()) else {
            return;
        };
        let age = if force { Duration::ZERO } else { AGE };
        load_comments(cx, self.target.clone(), number, &url, age);
        self.sync(cx);
    }

    /// Takes this file's threads from the cache's answer when it's a new one; true if anything
    /// shown changed.
    fn sync(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(url) = &self.range.pull_url else {
            return false;
        };
        let read = Cache::read::<Vec<ReviewComment>>(cx, &comments_key(url));
        let mut changed = read.error != self.comments_error;
        self.comments_error = read.error;
        if let Some(all) = read.data
            && !self.all.as_ref().is_some_and(|a| Arc::ptr_eq(a, &all))
        {
            self.landed(&all, cx);
            self.all = Some(all);
            changed = true;
        }
        changed
    }

    fn landed(&mut self, all: &[ReviewComment], cx: &mut Context<Self>) {
        // Only this file's: the rest of the PR's threads aren't kept.
        let mine: Vec<ReviewComment> = all
            .iter()
            .filter(|c| c.path == self.file.path)
            .cloned()
            .collect();
        let mut threads: Vec<Thread> = Vec::new();
        for (i, c) in mine.iter().enumerate() {
            if c.reply_to.is_none() {
                threads.push(Thread {
                    root: i,
                    replies: vec![],
                });
            }
        }
        for (i, c) in mine.iter().enumerate() {
            if let Some(to) = c.reply_to
                && let Some(t) = threads.iter_mut().find(|t| mine[t.root].id == to)
            {
                t.replies.push(i);
            }
        }
        let (mut threads, mut outdated): (Vec<Thread>, Vec<Thread>) = threads
            .into_iter()
            .partition(|t| mine[t.root].line.is_some());
        threads.sort_by_key(|t| mine[t.root].line);
        outdated.sort_by_key(|t| mine[t.root].original_line);
        self.comments = Arc::new(mine);
        self.threads = threads;
        self.outdated = outdated;
        self.text_stale = true;
        self.wants_signed(cx);
    }

    fn wants_signed(&mut self, cx: &mut Context<Self>) {
        let texts: Vec<&str> = self.comments.iter().map(|c| c.body.as_str()).collect();
        let Some(number) = self.range.number.filter(|_| self.attachments.wants(&texts)) else {
            return;
        };
        let (core, target) = (crate::core(cx), self.target.clone());
        self._signed = cx.spawn(async move |this, cx| {
            let Ok(map) = cx
                .background_spawn(async move { api::github::pr_attachments(&core, target, number) })
                .await
            else {
                return;
            };
            this.update(cx, |this, cx| {
                if this.attachments.add(map) {
                    this.text_stale = true;
                    cx.notify();
                }
            })
            .ok();
        });
    }

    /// Moves the box to `draft`: what was written where it was is kept for when it comes back.
    fn start(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        if self.range.pull_url.is_none() || self.draft == Some(draft) {
            return;
        }
        let text = self.composer.read(cx).value(cx).to_string();
        if let Some(was) = self.draft {
            keep_draft(&mut self.drafts, was, text);
        }
        let back = self
            .drafts
            .iter()
            .position(|(d, _)| *d == draft)
            .map(|i| self.drafts.remove(i).1)
            .unwrap_or_default();
        self.draft = Some(draft);
        self.folded = false;
        self.composer.update(cx, |c, cx| {
            c.set_value(&back, window, cx);
            c.focus(window, cx);
        });
        cx.notify();
    }

    /// Esc: the box closes and what's in it waits as that line's or thread's draft.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(was) = self.draft {
            let text = self.composer.read(cx).value(cx).to_string();
            keep_draft(&mut self.drafts, was, text);
        }
        self.cancel(window, cx);
        let diff = gpui_kit::Focusable::focus_handle(self.diff.read(cx), cx);
        window.focus(&diff, cx);
    }

    /// Cancel, clicked on purpose: what's in the box goes.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = None;
        self.composer
            .update(cx, |c, cx| c.set_value("", window, cx));
        cx.notify();
    }

    fn post(&mut self, cx: &mut Context<Self>) {
        let body = self.composer.read(cx).value(cx).trim().to_string();
        let (Some(draft), Some(number)) = (self.draft, self.range.number) else {
            return;
        };
        if body.is_empty() || self.busy {
            return;
        }
        let (side, line, reply_to) = match draft {
            // GitHub puts a comment on the last of the lines picked.
            Draft::New(r) => (side_name(r.side).to_string(), r.end as u64, None),
            Draft::Reply(root) => {
                let Some(c) = self.comments.iter().find(|c| c.id == root) else {
                    return;
                };
                (c.side.clone(), c.line.unwrap_or(0), Some(root))
            }
        };
        self.busy = true;
        let (core, target, head, path) = (
            crate::core(cx),
            self.target.clone(),
            self.range.head.clone(),
            self.file.path.clone(),
        );
        self._post = cx.spawn(async move |this, cx| {
            let done = cx
                .background_spawn(async move {
                    api::github::pr_comment_line(
                        &core, target, number, head, path, line, side, reply_to, body,
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match done {
                    Ok(_) => {
                        this.draft = None;
                        this.clear = true;
                        this.load(true, cx);
                    }
                    Err(e) => {
                        this.ws
                            .update(cx, |ws, cx| {
                                ws.toast(Toast::error(e).title("Could not comment"), cx)
                            })
                            .ok();
                    }
                }
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    fn composer(&self, label: String, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx);
        let empty = self.composer.read(cx).value(cx).trim().is_empty();
        div()
            .key_context("IssueComment")
            .on_action(cx.listener(|this, _: &PostComment, _, cx| this.post(cx)))
            // Taken before the box sees them: its own ⌘↵ would put a newline in the comment.
            .capture_action(cx.listener(|this, e: &Enter, _, cx| {
                if e.secondary {
                    cx.stop_propagation();
                    this.post(cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                cx.stop_propagation();
                this.close(window, cx);
            }))
            // The preview showing has the focus, and no binding for Esc.
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" && !e.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    this.close(window, cx);
                }
            }))
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(8.))
            .child(
                div()
                    .text_size(rems(0.69))
                    .text_color(t.subtle)
                    .child(label),
            )
            .child(self.composer.clone())
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.))
                    .child(
                        Button::new("cancel-comment")
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
                    )
                    .child(
                        Button::new("post-comment")
                            .primary()
                            .xsmall()
                            .label(if self.busy { "Posting…" } else { "Comment" })
                            .disabled(empty || self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.post(cx))),
                    ),
            )
            .into_any_element()
    }

    /// One thread: its place (a click scrolls the diff there; an outdated one has none to go
    /// to), its comments, and its reply box. `ti`: its index among all threads shown.
    fn thread_view(&self, ti: usize, th: &Thread, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx);
        let root = &self.comments[th.root];
        let side = if root.side == "LEFT" {
            Side::Old
        } else {
            Side::New
        };
        let place: AnyElement = match root.line {
            Some(line) => {
                let line = line as u32;
                let place = if side == Side::Old {
                    format!("Removed line {line}")
                } else {
                    format!("Line {line}")
                };
                text_button(("thread-line", ti), place, &t)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.diff
                            .update(cx, |d, cx| d.scroll_to_line(LinePos { side, line }, cx));
                    }))
                    .into_any_element()
            }
            None => div()
                .text_size(rems(0.69))
                .text_color(t.subtle)
                .child(match root.original_line {
                    Some(line) => format!("Outdated · was line {line}"),
                    None => "Outdated".into(),
                })
                .into_any_element(),
        };
        let mut thread = div()
            .m(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.panel)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .pt(px(6.))
                    .child(place),
            );
        for &ci in std::iter::once(&th.root).chain(&th.replies) {
            let c = &self.comments[ci];
            thread = thread.child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .text_size(rems(0.69))
                            .child(div().font_weight(t.medium()).child(c.author.clone()))
                            .child(div().text_color(t.subtle).child(ago(&c.created_at))),
                    )
                    .children(self.blocks.get(ci).map(|md| div().pt(px(4.)).child(md))),
            );
        }
        let id = root.id;
        let waiting = self.drafts.iter().any(|(d, _)| *d == Draft::Reply(id));
        if self.draft == Some(Draft::Reply(id)) {
            thread = thread.child(self.composer("Reply".into(), cx));
        } else {
            thread = thread.child(
                div().p(px(8.)).child(
                    Button::new(("reply", ti))
                        .ghost()
                        .xsmall()
                        .label(if waiting {
                            "Reply… (draft)"
                        } else {
                            "Reply…"
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start(Draft::Reply(id), window, cx)
                        })),
                ),
            );
        }
        thread.into_any_element()
    }

    fn threads_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let waiting: Vec<LineRange> = self
            .drafts
            .iter()
            .filter_map(|(d, _)| match d {
                Draft::New(r) if self.draft != Some(*d) => Some(*r),
                _ => None,
            })
            .collect();
        if self.threads.is_empty()
            && self.outdated.is_empty()
            && self.draft.is_none()
            && waiting.is_empty()
            && self.comments_error.is_none()
        {
            return None;
        }
        let t = theme(cx);
        let n: usize = self
            .threads
            .iter()
            .chain(&self.outdated)
            .map(|th| 1 + th.replies.len())
            .sum();
        let folded = self.folded;
        let head = div()
            .id("threads-head")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(px(28.))
            .px(px(12.))
            .bg(t.panel)
            .border_b_1()
            .border_color(t.border)
            .cursor_pointer()
            .text_size(rems(0.72))
            .child(
                Icon::new(if folded {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .xsmall()
                .text_color(t.subtle),
            )
            .child(
                Icon::new(IconName::MessageSquare)
                    .xsmall()
                    .text_color(t.subtle),
            )
            .child(format!(
                "{n} comment{} on this file",
                if n == 1 { "" } else { "s" }
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.folded = !this.folded;
                cx.notify();
            }));
        let mut list = div()
            .id("threads")
            .max_h(px(320.))
            .overflow_y_scroll()
            .border_b_1()
            .border_color(t.border);
        if !folded {
            for (i, r) in waiting.into_iter().enumerate() {
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .px(px(12.))
                        .py(px(6.))
                        .border_b_1()
                        .border_color(t.border)
                        .child(
                            text_button(("draft", i), format!("Draft on {}…", lines_label(r)), &t)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.start(Draft::New(r), window, cx)
                                })),
                        ),
                );
            }
            if let Some(Draft::New(r)) = self.draft {
                list = list.child(
                    div()
                        .border_b_1()
                        .border_color(t.border)
                        .child(self.composer(format!("Comment on {}", lines_label(r)), cx)),
                );
            }
            for (ti, th) in self.threads.iter().enumerate() {
                list = list.child(self.thread_view(ti, th, cx));
            }
            if !self.outdated.is_empty() {
                let open = self.outdated_open;
                let n = self.outdated.len();
                list = list.child(
                    div().flex().items_center().px(px(12.)).py(px(6.)).child(
                        text_button(
                            "outdated",
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(
                                    Icon::new(if open {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .xsmall(),
                                )
                                .child(format!(
                                    "{n} outdated thread{}",
                                    if n == 1 { "" } else { "s" }
                                )),
                            &t,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.outdated_open = !this.outdated_open;
                            cx.notify();
                        })),
                    ),
                );
                if open {
                    let at = self.threads.len();
                    for (i, th) in self.outdated.iter().enumerate() {
                        list = list.child(self.thread_view(at + i, th, cx));
                    }
                }
            }
        }
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .child(head)
                // Under the head, which a click folds: the whole of it, to select and copy.
                .when_some(self.comments_error.clone(), |d, e| {
                    d.child(
                        div()
                            .px(px(12.))
                            .py(px(6.))
                            .border_b_1()
                            .border_color(t.border)
                            .text_size(rems(0.72))
                            .child(error_text(
                                "comments-error",
                                &format!("Couldn't load the comments: {e}"),
                                t.removed,
                                cx,
                            )),
                    )
                })
                .when(!folded, |d| d.child(list))
                .into_any_element(),
        )
    }
}

impl TabItem for PullFileView {
    fn tab_title(&self, _: &App) -> SharedString {
        self.file
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&self.file.path)
            .to_string()
            .into()
    }

    fn tab_detail(&self, _: &App) -> Option<SharedString> {
        Some(match (self.range.number, &self.range.label) {
            (_, Some(label)) => label.clone().into(),
            (Some(n), None) => format!("#{n}").into(),
            (None, None) => self.range.label().into(),
        })
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::File(self.file.path.clone().into())
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        Some(self.file.path.clone().into())
    }

    fn tab_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(gpui_kit::Focusable::focus_handle(self.diff.read(cx), cx))
    }

    /// A comment being written, here or set aside for another thread.
    fn is_dirty(&self, cx: &App) -> bool {
        (self.draft.is_some() && !self.composer.read(cx).value(cx).trim().is_empty())
            || !self.drafts.is_empty()
    }
}

impl Render for PullFileView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let wakes = Cache::wakes(cx);
        if wakes != self.wakes {
            self.wakes = wakes;
            self.load(false, cx);
        }
        if std::mem::take(&mut self.clear) {
            self.composer
                .update(cx, |c, cx| c.set_value("", window, cx));
        }
        if std::mem::take(&mut self.text_stale) {
            let comments = self.comments.clone();
            let texts: Vec<&str> = comments.iter().map(|c| c.body.as_str()).collect();
            let pull_url = self.range.pull_url.clone().unwrap_or_default();
            let (web, signed) = (self.web.clone(), self.attachments.signed.clone());
            self.blocks
                .sync(&texts, &web, &pull_url, &signed, window, cx);
        }
        div()
            .id("pull-file")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .children(self.threads_view(cx))
            .child(div().flex_1().min_h_0().child(self.diff.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext};

    fn open_file(cx: &mut TestAppContext) -> (Entity<PullFileView>, &mut VisualTestContext) {
        crate::leak_tests::setup(cx);
        cx.add_window_view(|window, cx| {
            let ws = cx.new(Workspace::new);
            let range = PullRange {
                number: Some(1),
                pull_url: Some("https://github.com/o/r/pull/1".into()),
                label: None,
                base: "def".into(),
                head: "abc".into(),
            };
            let file = Arc::new(crate::leak_tests::file("src/zeta.rs"));
            PullFileView::new(ws.downgrade(), range, file, window, cx)
        })
    }

    fn comment(id: u64, line: Option<u64>, reply_to: Option<u64>) -> ReviewComment {
        ReviewComment {
            id,
            reply_to,
            path: "src/zeta.rs".into(),
            line,
            original_line: Some(9),
            side: "RIGHT".into(),
            author: "zeta-bot".into(),
            body: format!("note {id}"),
            created_at: "2026-01-01T00:00:00Z".into(),
            url: String::new(),
        }
    }

    #[gpui_kit::test]
    fn outdated_threads_are_kept_folded_and_the_pages_read_is_shared(cx: &mut TestAppContext) {
        let (view, cx) = open_file(cx);
        // The PR page's read lands in the cache: the file's tab takes its threads from it.
        let mut other = comment(5, Some(1), None);
        other.path = "src/other.rs".into();
        let all = vec![
            comment(1, Some(4), None),
            comment(2, None, None),
            comment(3, None, Some(2)),
            other,
        ];
        let key = comments_key("https://github.com/o/r/pull/1");
        cx.update(|_, cx| Cache::load(cx, &key, Duration::ZERO, move |_| Ok(all.clone())));
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            assert_eq!(v.comments.len(), 3);
            assert_eq!(v.threads.len(), 1);
            // The outdated root keeps its reply, folded away until asked for.
            assert_eq!(v.outdated.len(), 1);
            assert_eq!(v.outdated[0].replies.len(), 1);
            assert!(!v.outdated_open);
        });
        // Opened, it draws with its reply box like any thread.
        view.update(cx, |v, cx| {
            v.outdated_open = true;
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui_kit::test]
    fn esc_keeps_a_line_comment_and_cancel_drops_it(cx: &mut TestAppContext) {
        let (view, cx) = open_file(cx);
        let line = Draft::New(LineRange {
            side: Side::New,
            start: 4,
            end: 4,
        });
        view.update_in(cx, |v, window, cx| {
            v.start(line, window, cx);
            v.composer
                .update(cx, |c, cx| c.set_value("half a thought", window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_keystrokes("escape");
        // The box closed, the text waits as the line's draft, and the tab still asks before closing.
        view.read_with(cx, |v, cx| {
            assert_eq!(v.draft, None);
            assert_eq!(v.drafts, vec![(line, "half a thought".to_string())]);
            assert!(v.is_dirty(cx));
        });
        // Back on the line, the text is there; Cancel drops it.
        view.update_in(cx, |v, window, cx| {
            v.start(line, window, cx);
            assert_eq!(v.composer.read(cx).value(cx).to_string(), "half a thought");
            v.cancel(window, cx);
        });
        view.read_with(cx, |v, cx| {
            assert!(v.drafts.is_empty());
            assert!(!v.is_dirty(cx));
        });
    }

    #[test]
    fn a_draft_set_aside_comes_back_and_an_empty_one_is_forgotten() {
        let mut drafts = vec![];
        keep_draft(&mut drafts, Draft::Reply(1), "half a reply".into());
        keep_draft(&mut drafts, Draft::Reply(2), "  ".into());
        assert_eq!(drafts, vec![(Draft::Reply(1), "half a reply".to_string())]);
        keep_draft(&mut drafts, Draft::Reply(1), "the whole reply".into());
        assert_eq!(
            drafts,
            vec![(Draft::Reply(1), "the whole reply".to_string())]
        );
        for i in 0..40 {
            keep_draft(&mut drafts, Draft::Reply(100 + i), "x".into());
        }
        assert_eq!(drafts.len(), DRAFTS);
        assert_eq!(drafts.last().map(|d| d.0), Some(Draft::Reply(139)));
    }
}
