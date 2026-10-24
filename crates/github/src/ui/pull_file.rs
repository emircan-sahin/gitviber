//! One file of a pull request: its diff from the merge base (or across the picked commits), with
//! the PR's line comments on it as GitHub threads them: each thread with its line (a click
//! scrolls the diff there) and a reply box, and C on picked lines for a new one. The picked
//! commits' lines aren't the head's, so they take no comments.

use std::sync::Arc;
use std::time::{Duration, Instant};

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
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, rems,
};

use crate::cache::{Cache, Wakes};
use crate::model::{Target, repo_of};
use crate::ui::common::{Attachments, Blocks, ago, text_button, theme};
use crate::ui::markdown_input::MarkdownInput;

/// Comments are kept fresh a minute at a time while the file shows.
const AGE: Duration = Duration::from_secs(60);
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

pub struct PullFileView {
    range: PullRange,
    file: Arc<FileChange>,
    target: Target,
    web: String,
    ws: WeakEntity<Workspace>,
    diff: Entity<DiffView>,
    comments: Arc<Vec<ReviewComment>>,
    /// Indexes into `comments`: this file's threads, outdated ones (no line) left out.
    threads: Vec<Thread>,
    blocks: Blocks,
    text_stale: bool,
    attachments: Attachments,
    comments_at: Option<Instant>,
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
    _load: Task<()>,
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
            MarkdownInput::new(web.clone(), page, window, cx, |t| {
                t.rows(3)
                    .placeholder(format!("Leave a comment (markdown; {POST_KEY} to post)"))
            })
            .small()
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
                if Cache::wakes(cx) != this.wakes {
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
            comments: Arc::new(vec![]),
            threads: vec![],
            blocks: Blocks::default(),
            text_stale: false,
            attachments: Attachments::default(),
            comments_at: None,
            comments_error: None,
            draft: None,
            composer,
            drafts: vec![],
            folded: false,
            busy: false,
            clear: false,
            wakes: Cache::wakes(cx),
            focus: cx.focus_handle(),
            _load: Task::ready(()),
            _post: Task::ready(()),
            _signed: Task::ready(()),
            _subs: subs,
            _live: Live::new("PullFileView"),
        };
        view.load(cx);
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

    fn load(&mut self, cx: &mut Context<Self>) {
        let (Some(number), Some(_)) = (self.range.number, self.range.pull_url.as_ref()) else {
            return;
        };
        self.comments_at = Some(Instant::now());
        let (core, target) = (crate::core(cx), self.target.clone());
        self._load = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(
                    async move { api::github::pr_review_comments(&core, target, number) },
                )
                .await;
            this.update(cx, |this, cx| {
                match read {
                    Ok(all) => this.landed(all, cx),
                    Err(e) => this.comments_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        });
    }

    fn landed(&mut self, all: Vec<ReviewComment>, cx: &mut Context<Self>) {
        // Only this file's: the rest of the PR's threads aren't kept.
        let mine: Vec<ReviewComment> = all
            .into_iter()
            .filter(|c| c.path == self.file.path)
            .collect();
        let mut threads: Vec<Thread> = Vec::new();
        for (i, c) in mine.iter().enumerate() {
            if c.reply_to.is_none() && c.line.is_some() {
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
        threads.sort_by_key(|t| mine[t.root].line);
        self.comments = Arc::new(mine);
        self.threads = threads;
        self.comments_error = None;
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
                        this.load(cx);
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
                this.cancel(window, cx);
            }))
            // The preview showing has the focus, and no binding for Esc.
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" && !e.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    this.cancel(window, cx);
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

    fn threads_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.threads.is_empty() && self.draft.is_none() && self.comments_error.is_none() {
            return None;
        }
        let t = theme(cx);
        let n: usize = self.threads.iter().map(|th| 1 + th.replies.len()).sum();
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
            .when_some(self.comments_error.clone(), |d, e| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(t.removed)
                        .child(format!("· {e}")),
                )
            })
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
            if let Some(Draft::New(r)) = self.draft {
                let old = if r.side == Side::Old { " (old)" } else { "" };
                let lines = if r.start == r.end {
                    format!("line {}", r.end)
                } else {
                    format!("lines {}–{} (on {})", r.start, r.end, r.end)
                };
                list = list.child(
                    div()
                        .border_b_1()
                        .border_color(t.border)
                        .child(self.composer(format!("Comment on {lines}{old}"), cx)),
                );
            }
            for (ti, th) in self.threads.iter().enumerate() {
                let root = &self.comments[th.root];
                let (line, side) = (
                    root.line.unwrap_or(0) as u32,
                    if root.side == "LEFT" {
                        Side::Old
                    } else {
                        Side::New
                    },
                );
                let place = if side == Side::Old {
                    format!("Removed line {line}")
                } else {
                    format!("Line {line}")
                };
                let mut thread = div()
                    .m(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.panel)
                    .overflow_hidden()
                    .child(div().flex().items_center().px(px(10.)).pt(px(6.)).child(
                        text_button(("thread-line", ti), place, &t).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.diff.update(cx, |d, cx| {
                                    d.scroll_to_line(LinePos { side, line }, cx)
                                });
                            },
                        )),
                    ));
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
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(c.author.clone()),
                                    )
                                    .child(div().text_color(t.subtle).child(ago(&c.created_at))),
                            )
                            .children(self.blocks.get(ci).map(|md| div().pt(px(4.)).child(md))),
                    );
                }
                let id = root.id;
                let waiting = self.drafts.iter().any(|(d, _)| *d == Draft::Reply(id));
                thread = if self.draft == Some(Draft::Reply(id)) {
                    thread.child(self.composer("Reply".into(), cx))
                } else {
                    thread.child(
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
                    )
                };
                list = list.child(thread);
            }
        }
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .child(head)
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
            if self.comments_at.is_some_and(|at| at.elapsed() > AGE) {
                self.load(cx);
            }
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
