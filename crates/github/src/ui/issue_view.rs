//! An issue's tab: read it, comment, edit, label, close (completed or not planned) and reopen,
//! delete (admins), or start it in its own worktree. Its body and thread live here only.

use std::sync::Arc;
use std::time::Instant;

use gitviber_core::api;
use gitviber_core::github::IssueDetail;
use gitviber_markdown::MarkdownView;
use gitviber_ui::actions::editor;
use gitviber_ui::actions::github::PostComment;
use gitviber_ui::keymap::scroll::{self, Step};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{TabIcon, TabItem, Toast, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EntityId, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, ListAlignment, ListState, ParentElement as _, Pixels,
    Render, SharedString, Styled as _, Subscription, Task, WeakEntity, Window, div, list,
    prelude::FluentBuilder as _, px, rems,
};

use crate::cache::{Cache, MIN_AGE, Wakes};
use crate::logic::actions::issue_rights;
use crate::logic::issue_work::issue_branch_name;
use crate::model::{Account, IssueItem, LabelItem, Target, repo_of};
use crate::requests::{GithubRequest, request};
use crate::ui::common::{
    Attachments, Blocks, ago, copy, error_text, issue_icon, issue_pill, label_chip, load_failed,
    open_url, section, section_part, theme,
};
use crate::ui::dialogs::confirm;
use crate::ui::labels::LabelPicker;
use crate::ui::markdown_input::MarkdownInput;
use crate::ui::page_find::{FindHost, Key, PageFind, Parts};

/// How far past the page's view its list measures what it hasn't yet.
const PAGE_OVERDRAW: Pixels = px(600.);

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloseReason {
    Completed,
    NotPlanned,
}

impl CloseReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NotPlanned => "not_planned",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Completed => "Close as completed",
            Self::NotPlanned => "Close as not planned",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Completed => "Done, closed, fixed, resolved",
            Self::NotPlanned => "Won't fix, can't repro, duplicate, stale",
        }
    }
}

/// A write's end, and whether the comment in the box went out on the way: a close that failed
/// after its comment posted mustn't post it again on the retry.
struct Wrote {
    result: Result<(), String>,
    commented: bool,
}

impl From<Result<(), String>> for Wrote {
    fn from(result: Result<(), String>) -> Self {
        Wrote {
            result,
            commented: false,
        }
    }
}

pub struct IssueView {
    url: String,
    number: u64,
    target: Target,
    web: String,
    item: Option<IssueItem>,
    ws: WeakEntity<Workspace>,
    detail: Option<Arc<IssueDetail>>,
    detail_error: Option<String>,
    detail_at: Option<Instant>,
    loading: bool,
    /// The body (block 0) and each comment, synced at the next frame after a read.
    blocks: Blocks,
    text_stale: bool,
    attachments: Attachments,
    editing: Option<(Entity<InputState>, Entity<MarkdownInput>)>,
    /// Enter in the title being edited saves; replaced by the next edit.
    _editing: Option<Subscription>,
    comment: Entity<MarkdownInput>,
    comment_clear: bool,
    /// The labels being picked, shown in place of the issue's until they're saved.
    picking: Option<Vec<LabelItem>>,
    busy: Option<SharedString>,
    wakes: Wakes,
    generation: u64,
    focus: FocusHandle,
    /// The page as a list: the issue, then each comment, then the box to write one, so a long
    /// thread draws only what's in view.
    page: ListState,
    /// The first and last items': drawn out of view while focus is in them (an edit, the box).
    head_focus: FocusHandle,
    compose_focus: FocusHandle,
    find: PageFind,
    _detail: Task<()>,
    _action: Task<()>,
    _signed: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl IssueView {
    pub fn new(
        ws: WeakEntity<Workspace>,
        url: String,
        item: Option<IssueItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::ui::use_workspace_project(&ws, cx);
        let number = url
            .rsplit('/')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or_default();
        let repo = repo_of(&url);
        let web = format!("https://github.com/{repo}");
        let comment = cx.new(|cx| {
            MarkdownInput::new(web.clone(), url.clone(), window, cx, |t| {
                t.rows(4).placeholder("Leave a comment (markdown)")
            })
            .small()
            .signed_by(Some(repo.clone()), number)
        });
        let input = comment.read(cx).input().clone();
        let subs = vec![
            // Text in the box keeps the tab: a preview would be replaced, a hidden tab released.
            cx.subscribe(&input, |this: &mut Self, c, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) && !c.read(cx).value().trim().is_empty() {
                    this.pin(cx);
                }
                cx.notify();
            }),
            cx.observe_global::<Cache>({
                // The account decides what the page offers, and it lands without a wake.
                let mut account = Cache::stamp(cx, &[crate::account::KEY]);
                move |this: &mut Self, cx| {
                    let now = Cache::stamp(cx, &[crate::account::KEY]);
                    if Cache::wakes(cx) != this.wakes
                        || Cache::generation(cx) != this.generation
                        || now != account
                    {
                        account = now;
                        cx.notify();
                    }
                }
            }),
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    Cache::wake(cx);
                }
            }),
        ];
        let (head_focus, compose_focus) = (cx.focus_handle(), cx.focus_handle());
        let page = ListState::new(0, ListAlignment::Top, PAGE_OVERDRAW);
        page.splice_focusable(
            0..0,
            [Some(head_focus.clone()), Some(compose_focus.clone())],
        );
        let mut view = IssueView {
            web,
            target: Some(repo),
            url,
            number,
            item,
            ws,
            detail: None,
            detail_error: None,
            detail_at: None,
            loading: false,
            blocks: Blocks::default(),
            text_stale: false,
            attachments: Attachments::default(),
            editing: None,
            comment,
            comment_clear: false,
            picking: None,
            busy: None,
            wakes: Cache::wakes(cx),
            generation: Cache::generation(cx),
            focus: cx.focus_handle(),
            page,
            head_focus,
            compose_focus,
            find: PageFind::default(),
            _detail: Task::ready(()),
            _action: Task::ready(()),
            _signed: Task::ready(()),
            _editing: None,
            _subs: subs,
            _live: Live::new("IssueView"),
        };
        crate::account::load(cx, false);
        view.load(cx);
        view
    }

    fn key(&self) -> String {
        format!("issue:{}", self.url)
    }

    fn pin(&self, cx: &mut App) {
        let key = self.key();
        self.ws.update(cx, |ws, cx| ws.pin_tab(&key, cx)).ok();
    }

    fn item(&self) -> Option<IssueItem> {
        self.detail
            .as_ref()
            .map(|d| IssueItem::from(&d.issue))
            .or_else(|| self.item.clone())
    }

    fn toast(&self, toast: Toast, cx: &mut App) {
        self.ws.update(cx, |ws, cx| ws.toast(toast, cx)).ok();
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.detail_at = Some(Instant::now());
        let (core, target, number) = (crate::core(cx), self.target.clone(), self.number);
        self._detail = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { api::github::issue_detail(&core, target, number) })
                .await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match read {
                    Ok(d) => {
                        // Closed by a push or edited on GitHub: the lists still show it as it was.
                        if Cache::list_is_behind::<IssueItem>(
                            cx,
                            "issues:",
                            &d.issue.url,
                            &d.issue.updated_at,
                            |r| (r.url.as_str(), r.updated_at.as_str()),
                        ) {
                            Cache::issues_changed(cx);
                        }
                        this.detail = Some(Arc::new(d));
                        this.detail_error = None;
                        this.text_stale = true;
                        this.wants_signed(cx);
                    }
                    Err(e) => this.detail_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open(self, None, &self.focus, window, cx);
    }

    /// A scroll key, from the page or a text focused in it, moves the page.
    fn scroll_key(&mut self, step: Step, cx: &mut Context<Self>) {
        if scroll::step_list(&self.page, step) {
            cx.notify();
        }
    }

    /// Scrolls the page's list to item `ix`'s top unless some of it is in view; what's in it
    /// then asks for the line it wants. By its top: items never drawn have no height yet.
    fn reveal_item(&self, ix: usize) {
        let view = self.page.viewport_bounds();
        if !self
            .page
            .bounds_for_item(ix)
            .is_some_and(|b| b.intersects(&view))
        {
            self.page.scroll_to(gpui_kit::ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
        }
    }

    /// One item per comment, between the issue and the box: the list follows the comments read.
    fn sync_items(&self) {
        let want = 2 + self.detail.as_ref().map_or(0, |d| d.thread.len());
        let have = self.page.item_count();
        if want > have {
            self.page.splice(have - 1..have - 1, want - have);
        } else if want < have {
            self.page.splice(want - 1..have - 1, 0);
        }
    }

    /// The description and comments, as markdown views; at a frame, which has the window.
    fn sync_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.text_stale = false;
        let Some(d) = self.detail.clone() else { return };
        let (web, url) = (self.web.clone(), self.url.clone());
        let signed = self.attachments.signed.clone();
        // Block 0 (the description) is in the page's first item, comment n's block in item n + 1.
        for item in self
            .blocks
            .sync(&Self::texts(&d), &web, &url, &signed, window, cx)
        {
            self.page.remeasure_items(item..item + 1);
        }
    }

    /// The labels shown: the ones being picked until they're saved.
    fn labels(&self, i: &IssueItem) -> Vec<LabelItem> {
        self.picking.clone().unwrap_or_else(|| i.labels.clone())
    }

    /// The description's view, when it has text and isn't being edited.
    fn body_view(&self) -> Option<Entity<MarkdownView>> {
        self.detail
            .as_ref()
            .filter(|d| self.editing.is_none() && !d.body.trim().is_empty())
            .and_then(|_| self.blocks.get(0))
    }

    /// Comment `n`'s view, when it has text.
    fn comment_view(&self, d: &IssueDetail, n: usize) -> Option<Entity<MarkdownView>> {
        let c = d.thread.get(n)?;
        self.blocks.get(n + 1).filter(|_| !c.body.trim().is_empty())
    }

    fn texts(d: &IssueDetail) -> Vec<&str> {
        std::iter::once(d.body.as_str())
            .chain(d.thread.iter().map(|c| c.body.as_str()))
            .collect()
    }

    /// A private repo's attachments: signed links, asked once per new one.
    fn wants_signed(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.detail.clone() else { return };
        if !self.attachments.wants(&Self::texts(&d)) {
            return;
        }
        let (core, target, number) = (crate::core(cx), self.target.clone(), self.number);
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

    fn account(&self, cx: &App) -> Option<Arc<Account>> {
        crate::account::account(cx)
    }

    /// Runs a write under `busy`; says `done` and reads again when it went through. `after`
    /// runs either way, told whether it did.
    fn act(
        &mut self,
        busy: &'static str,
        done: String,
        run: impl FnOnce(&gitviber_core::Core) -> Wrote + Send + 'static,
        after: impl FnOnce(&mut Self, bool, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(busy.into());
        let core = crate::core(cx);
        self._action = cx.spawn(async move |this, cx| {
            let wrote = cx.background_spawn(async move { run(&core) }).await;
            this.update(cx, |this, cx| {
                this.busy = None;
                if wrote.commented {
                    this.comment_clear = true;
                }
                let ok = wrote.result.is_ok();
                match wrote.result {
                    Ok(()) => {
                        this.toast(Toast::success(done), cx);
                        Cache::issues_changed(cx);
                        this.load(cx);
                    }
                    Err(e) => {
                        let e = if wrote.commented {
                            format!("{e} (the comment was posted)")
                        } else {
                            e
                        };
                        this.toast(Toast::error(e).title(format!("{busy} failed")), cx);
                        // What the comment said is on GitHub now; read it back.
                        if wrote.commented {
                            this.load(cx);
                        }
                    }
                }
                after(this, ok, cx);
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    fn text(&self, cx: &App) -> String {
        self.comment.read(cx).value(cx).trim().to_string()
    }

    fn post(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        if text.is_empty() {
            return;
        }
        let (target, number) = (self.target.clone(), self.number);
        let run = move |core: &gitviber_core::Core| {
            let result = api::github::issue_comment(core, target, number, text);
            let commented = result.is_ok();
            Wrote { result, commented }
        };
        self.act("Comment", "Comment added".into(), run, |_, _, _| {}, cx);
    }

    /// Like GitHub's "Close with comment": the comment goes first so it reads before the close.
    fn set_open(&mut self, open: bool, reason: CloseReason, cx: &mut Context<Self>) {
        let (target, number, text) = (self.target.clone(), self.number, self.text(cx));
        let done = format!("{} #{number}", if open { "Reopened" } else { "Closed" });
        let run = move |core: &gitviber_core::Core| {
            let mut commented = false;
            if !text.is_empty() {
                if let Err(e) = api::github::issue_comment(core, target.clone(), number, text) {
                    return Wrote {
                        result: Err(e),
                        commented,
                    };
                }
                commented = true;
            }
            let result =
                api::github::issue_set_open(core, target, number, open, reason.as_str().into())
                    .map(|_| ());
            Wrote { result, commented }
        };
        self.act(
            if open { "Reopen" } else { "Close" },
            done,
            run,
            |_, _, _| {},
            cx,
        );
    }

    fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &self.detail else { return };
        let (title, body) = (d.issue.title.clone(), d.body.clone());
        let title = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Title")
                .default_value(title)
        });
        let body = cx.new(|cx| {
            MarkdownInput::new(self.web.clone(), self.url.clone(), window, cx, |t| {
                t.rows(12)
                    .placeholder("Description (markdown)")
                    .default_value(body)
            })
            .signed_by(self.target.clone(), self.number)
        });
        title.update(cx, |t, cx| t.focus(window, cx));
        self._editing = Some(cx.subscribe(&title, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.save(cx);
            }
        }));
        self.editing = Some((title, body));
        // An edit in progress keeps the tab.
        self.pin(cx);
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some((title, body)) = &self.editing else {
            return;
        };
        let (title, body) = (
            title.read(cx).value().trim().to_string(),
            body.read(cx).value(cx).to_string(),
        );
        if title.is_empty() {
            return;
        }
        let (target, number) = (self.target.clone(), self.number);
        let run = move |core: &gitviber_core::Core| {
            api::github::issue_edit(core, target, number, title, body)
                .map(|_| ())
                .into()
        };
        self.act(
            "Edit",
            format!("Updated #{number}"),
            run,
            |this, ok, _| {
                if ok {
                    this.editing = None;
                }
            },
            cx,
        );
    }

    /// Like GitHub, the picked labels are saved together when the picker closes.
    fn pick_labels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.item() else { return };
        self.picking = Some(i.labels.clone());
        let weak = cx.weak_entity();
        let closing = cx.weak_entity();
        LabelPicker::open(
            vec![self.target.clone()],
            i.labels.clone(),
            Some("Saved when closed".into()),
            std::rc::Rc::new(move |labels, _, cx| {
                weak.update(cx, |this, cx| {
                    this.picking = Some(labels);
                    cx.notify();
                })
                .ok();
            }),
            Some(std::rc::Rc::new(
                move |labels: Vec<LabelItem>, _: &mut Window, cx: &mut App| {
                    closing
                        .update(cx, |this, cx| {
                            this.save_labels(labels, i.labels.clone(), cx)
                        })
                        .ok();
                },
            )),
            window,
            cx,
        );
    }

    fn save_labels(&mut self, picked: Vec<LabelItem>, had: Vec<LabelItem>, cx: &mut Context<Self>) {
        let same = picked.len() == had.len()
            && picked.iter().all(|l| had.iter().any(|m| m.name == l.name));
        if same {
            self.picking = None;
            cx.notify();
            return;
        }
        let (target, number) = (self.target.clone(), self.number);
        let names: Vec<String> = picked.iter().map(|l| l.name.clone()).collect();
        let run = move |core: &gitviber_core::Core| {
            api::github::issue_set_labels(core, target, number, names)
                .map(|_| ())
                .into()
        };
        // Saved or not, the issue's own labels show again: a refused save isn't on it.
        self.act(
            "Labels",
            "Labels updated".into(),
            run,
            |this, _, _| this.picking = None,
            cx,
        );
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.item() else { return };
        let weak = cx.weak_entity();
        let message = format!(
            "Delete #{} \"{}\" and all its comments? This can't be undone.",
            i.number, i.title
        );
        confirm(
            "Delete issue",
            message,
            "Delete",
            window,
            cx,
            move |_, cx| {
                weak.update(cx, |this, cx| {
                    let (target, number, key) = (this.target.clone(), this.number, this.key());
                    let ws = this.ws.clone();
                    let run = move |core: &gitviber_core::Core| {
                        api::github::issue_delete(core, target, number).into()
                    };
                    // A deleted issue's tab closes: nothing to read again.
                    this.act(
                        "Delete",
                        format!("Deleted #{number}"),
                        run,
                        move |_, ok, cx| {
                            if ok {
                                ws.update(cx, |ws, cx| ws.close_tab(&key, cx)).ok();
                            }
                        },
                        cx,
                    );
                })
                .ok();
            },
        );
    }

    fn start_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(i), Some(origin)) = (
            self.item(),
            self.account(cx)
                .and_then(|a| a.origin.as_ref().map(|o| o.full_name())),
        ) else {
            return;
        };
        let branch = issue_branch_name(i.number, &i.title);
        request(
            GithubRequest::StartIssue {
                number: i.number,
                title: i.title,
                url: i.url,
                origin,
                branch,
            },
            window,
            cx,
        );
    }
}

impl TabItem for IssueView {
    fn tab_title(&self, _: &App) -> SharedString {
        match self.item() {
            Some(i) => format!("#{} {}", i.number, i.title).into(),
            None => format!("#{}", self.number).into(),
        }
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::Icon(IconName::CircleDot)
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        Some(self.url.clone().into())
    }

    fn tab_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }

    /// A comment being written or an edit open: closing asks, and the tab isn't released.
    fn is_dirty(&self, cx: &App) -> bool {
        self.editing.is_some() || !self.text(cx).is_empty()
    }
}

const NO_DESCRIPTION: &str = "No description provided.";

fn number(i: &IssueItem) -> String {
    format!("#{}", i.number)
}

fn opened(i: &IssueItem) -> String {
    format!("opened {}", ago(&i.created_at))
}

fn assigned(i: &IssueItem) -> String {
    format!("assigned to {}", i.assignees.join(", "))
}

impl FindHost for IssueView {
    /// The title, the line under it and the description, or the description being edited when
    /// it's previewed; the comments; the comment being written, previewed.
    fn find_parts(&self, parts: &mut Parts, cx: &App) {
        let Some(i) = self.item() else { return };
        let d = self.detail.as_deref();
        if let Some((_, body)) = &self.editing {
            if let Some(v) = body.read(cx).shown_preview() {
                parts.view(v);
            }
        } else {
            parts.text(("title", 0), &i.title);
            parts.text(("number", 0), &number(&i));
            parts.text(("author", 0), &i.author);
            parts.text(("opened", 0), &opened(&i));
            if !i.assignees.is_empty() {
                parts.text(("assignees", 0), &assigned(&i));
            }
            for (n, l) in self.labels(&i).iter().enumerate() {
                parts.text(("label", n), &l.name);
            }
            match (self.body_view(), d) {
                (Some(v), _) => parts.view(&v),
                (None, Some(_)) => parts.text(("no-description", 0), NO_DESCRIPTION),
                (None, None) => {}
            }
        }
        for (n, c) in d.into_iter().flat_map(|d| d.thread.iter().enumerate()) {
            parts.text(("comment-author", n), &c.author);
            parts.text(("comment-time", n), &ago(&c.created_at));
            if let Some(v) = d.and_then(|d| self.comment_view(d, n)) {
                parts.view(&v);
            }
        }
        if let Some(v) = self.comment.read(cx).shown_preview() {
            parts.view(v);
        }
    }

    /// The page's list draws the item the match is in: a comment's, else the issue's.
    fn show_part(&mut self, (what, ix): Key, cx: &mut Context<Self>) {
        let item = match what {
            "comment-author" | "comment-time" => ix + 1,
            _ => 0,
        };
        self.reveal_item(item);
        cx.notify();
    }

    /// The description is in the first item, comment n's view in item n + 1, the comment being
    /// written in the last; the description being edited is in the first.
    fn show_view(&mut self, view: EntityId, cx: &mut Context<Self>) {
        let writing = self
            .comment
            .read(cx)
            .shown_preview()
            .is_some_and(|v| v.entity_id() == view);
        let item = match self.blocks.position(view) {
            Some(item) => item,
            None if writing => self.page.item_count() - 1,
            None => 0,
        };
        self.reveal_item(item);
        cx.notify();
    }
}

impl IssueView {
    /// The page's first item: the title and the line under it, the actions, the description.
    fn head(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(i) = self.item() else {
            return div().into_any_element();
        };
        let t = theme(cx);
        let marks = self.find.marks();
        let account = self.account(cx);
        let d = self.detail.clone();
        let rights = issue_rights(
            &i,
            account.as_deref(),
            d.as_ref().and_then(|d| d.closed_by.as_deref()),
        );
        let busy = self.busy.is_some();
        let text = self.text(cx);
        let labels = self.labels(&i);

        let header: AnyElement = if let Some((title, body)) = &self.editing {
            let empty = title.read(cx).value().trim().is_empty();
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(Input::new(title))
                .child(body.clone())
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .child(
                            Button::new("cancel-edit")
                                .ghost()
                                .small()
                                .label("Cancel")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.editing = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("save-edit")
                                .primary()
                                .small()
                                .label("Save")
                                .disabled(busy || empty)
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        ),
                )
                .into_any_element()
        } else {
            let mut meta = div()
                .mt(px(6.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .text_size(rems(0.75))
                .text_color(t.muted_foreground)
                .child(issue_pill(&i, &t))
                .child(
                    div()
                        .text_color(t.foreground.opacity(0.85))
                        .child(marks.text(("author", 0), i.author.clone(), cx)),
                )
                .child(marks.text(("opened", 0), opened(&i), cx));
            if !i.assignees.is_empty() {
                meta = meta
                    .child(div().text_color(t.subtle).child("·"))
                    .child(marks.text(("assignees", 0), assigned(&i), cx));
            }
            for (n, l) in labels.iter().enumerate() {
                meta = meta.child(label_chip(
                    l,
                    marks.text(("label", n), l.name.clone(), cx),
                    &t,
                ));
            }
            if rights.label {
                meta = meta.child(
                    Button::new("edit-labels")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Tag)
                        .label(if labels.is_empty() {
                            "Add labels"
                        } else {
                            "Edit labels"
                        })
                        .disabled(busy || d.is_none())
                        .on_click(cx.listener(|this, _, window, cx| this.pick_labels(window, cx))),
                );
            }
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(div().mt(px(6.)).child(issue_icon(&i, &t)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_x(px(6.))
                                .text_size(rems(1.06))
                                .font_weight(t.semibold())
                                .child(marks.text(("title", 0), i.title.clone(), cx))
                                .child(
                                    div()
                                        .text_color(t.subtle)
                                        .font_weight(FontWeight::NORMAL)
                                        .child(marks.text(("number", 0), number(&i), cx)),
                                ),
                        )
                        .child(meta),
                )
                .into_any_element()
        };

        let (url, copy_url) = (i.url.clone(), i.url.clone());
        let weak = cx.weak_entity();
        let close_label = if text.is_empty() {
            "Close issue"
        } else {
            "Close with comment"
        };
        let actions = div()
            .mt(px(16.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .when(self.editing.is_none() && rights.edit, |a| {
                a.child(
                    Button::new("edit")
                        .small()
                        .icon(IconName::Pencil)
                        .label("Edit")
                        .disabled(busy || d.is_none())
                        .on_click(cx.listener(|this, _, window, cx| this.edit(window, cx))),
                )
            })
            .when(i.open && rights.close, |a| {
                a.child(
                    Button::new("close")
                        .small()
                        .icon(IconName::CircleCheck)
                        .label(close_label)
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_open(false, CloseReason::Completed, cx)
                        })),
                )
                .child(
                    Button::new("close-how")
                        .small()
                        .icon(IconName::ChevronDown)
                        .disabled(busy)
                        .dropdown_menu(move |menu, _, _| {
                            let mut menu = menu;
                            for r in [CloseReason::Completed, CloseReason::NotPlanned] {
                                let weak = weak.clone();
                                let icon = if r == CloseReason::Completed {
                                    IconName::CircleCheck
                                } else {
                                    IconName::CircleSlash
                                };
                                let item = PopupMenuItem::element(move |_, _| {
                                    div().flex().flex_col().child(r.label()).child(
                                        // Faded, not muted: the row's ink turns light when highlighted.
                                        div().text_size(rems(0.75)).opacity(0.7).child(r.note()),
                                    )
                                });
                                menu = menu.item(item.icon(icon).on_click(move |_, _, cx| {
                                    weak.update(cx, |this, cx| this.set_open(false, r, cx)).ok();
                                }));
                            }
                            menu
                        }),
                )
            })
            .when(!i.open && rights.reopen, |a| {
                a.child(
                    Button::new("reopen")
                        .small()
                        .icon(IconName::CircleDot)
                        .label(if text.is_empty() {
                            "Reopen"
                        } else {
                            "Reopen with comment"
                        })
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_open(true, CloseReason::Completed, cx)
                        })),
                )
            })
            .when(
                i.open && account.as_ref().is_some_and(|a| a.origin.is_some()),
                |a| {
                    a.child(
                        Button::new("start")
                            .small()
                            .icon(IconName::FolderGit2)
                            .label("Start in a worktree…")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.start_worktree(window, cx)),
                            ),
                    )
                },
            )
            .child(
                Button::new("on-github")
                    .small()
                    .icon(IconName::ExternalLink)
                    .label("Open on GitHub")
                    .on_click(move |_, _, cx| open_url(&url, cx)),
            )
            .child(
                Button::new("copy-link")
                    .ghost()
                    .small()
                    .icon(IconName::Link)
                    .tooltip("Copy link")
                    .on_click(move |_, window, cx| copy(&copy_url, "Link copied", window, cx)),
            )
            .child(
                Button::new("reload")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .tooltip("Refresh")
                    .loading(self.loading)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            )
            .when_some(self.busy.clone(), |a, b| {
                a.child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(t.muted_foreground)
                        .child(format!("{b}…")),
                )
            })
            // GitHub lets repository admins alone delete issues.
            .when(rights.delete, |a| {
                a.child(div().flex_1()).child(
                    Button::new("delete")
                        .ghost()
                        .small()
                        .danger()
                        .icon(IconName::Trash)
                        .label("Delete")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                )
            });

        let mut page = div()
            .track_focus(&self.head_focus)
            .child(header)
            .child(actions)
            // Cached data stays on screen; a failed refresh (a rate limit, say) is noted under it.
            .when_some(
                self.detail_error.clone().filter(|_| !busy && d.is_some()),
                |page, e| {
                    page.child(
                        div()
                            .mt(px(8.))
                            .text_size(rems(0.75))
                            .debug_selector(|| "refresh-error".into())
                            .child(error_text(
                                "refresh-error",
                                &format!("Couldn't refresh: {e}"),
                                t.removed,
                                cx,
                            )),
                    )
                },
            );
        if self.editing.is_none() {
            let desc = match (self.body_view(), &d) {
                (Some(md), _) => div().px(px(12.)).py(px(10.)).child(md).into_any_element(),
                (None, Some(_)) => div()
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(rems(0.75))
                    .text_color(t.subtle)
                    .italic()
                    .child(marks.text(("no-description", 0), NO_DESCRIPTION, cx))
                    .into_any_element(),
                (None, None) => div().h(px(8.)).into_any_element(),
            };
            page = page.child(section("Description", None, &t).child(desc));
        }
        page.into_any_element()
    }

    /// The page's last item: the box to write a comment in.
    fn composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx);
        let busy = self.busy.is_some();
        let text = self.text(cx);
        let can_post = !text.is_empty() && !busy;
        section("Add a comment", None, &t)
            .track_focus(&self.compose_focus)
            .child(
                div()
                    .p(px(8.))
                    .key_context("IssueComment")
                    .on_action(cx.listener(move |this, _: &PostComment, _, cx| {
                        if can_post {
                            this.post(cx);
                        }
                    }))
                    // The box's own ⌘↵ wins over PostComment's binding and adds a newline:
                    // taken before it sees it.
                    .capture_action(cx.listener(move |this, e: &Enter, _, cx| {
                        if e.secondary {
                            cx.stop_propagation();
                            if can_post {
                                this.post(cx);
                            }
                        }
                    }))
                    .child(self.comment.clone())
                    .child(
                        div().mt(px(8.)).flex().justify_end().child(
                            Button::new("post")
                                .primary()
                                .small()
                                .label("Comment")
                                .disabled(!can_post)
                                .on_click(cx.listener(|this, _, _, cx| this.post(cx))),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// Comment `n`, as its item of the page's list: a slice of the Conversation section.
    fn comment(&self, n: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(d) = self.detail.clone() else {
            return div().into_any_element();
        };
        let Some(c) = d.thread.get(n) else {
            return div().into_any_element();
        };
        let t = theme(cx);
        let marks = self.find.marks();
        let md = self.comment_view(&d, n);
        let title = (n == 0).then(|| ("Conversation", Some(d.thread.len().to_string().into())));
        section_part(title, n + 1 == d.thread.len(), &t)
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .text_size(rems(0.72))
                            .child(div().font_weight(t.semibold()).child(marks.text(
                                ("comment-author", n),
                                c.author.clone(),
                                cx,
                            )))
                            .child(div().text_color(t.subtle).child(marks.text(
                                ("comment-time", n),
                                ago(&c.created_at),
                                cx,
                            ))),
                    )
                    .children(md.map(|md| div().mt(px(6.)).child(md))),
            )
            .into_any_element()
    }

    /// Item `ix` of the page's list: the issue, a comment each, then the box.
    fn page_item(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let comments = self.detail.as_ref().map_or(0, |d| d.thread.len());
        let item = match ix {
            0 => self.head(cx),
            n if n <= comments => self.comment(n - 1, cx),
            _ => self.composer(cx),
        };
        // The page's column, centered in the list's width.
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(900.))
                    .px(px(24.))
                    .when(ix == 0, |d| d.pt(px(20.)))
                    .when(ix > comments, |d| d.pb(px(20.)))
                    .child(item),
            )
            .into_any_element()
    }
}

impl Render for IssueView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let (wakes, generation) = (Cache::wakes(cx), Cache::generation(cx));
        // Another repo or account (a restored tab reads before its project opened): read again now.
        let reset = generation != self.generation;
        if wakes != self.wakes || reset {
            self.wakes = wakes;
            self.generation = generation;
            let old = self.detail_at.is_none_or(|at| at.elapsed() > MIN_AGE);
            if (reset || old || self.detail.is_none()) && !self.loading {
                self.load(cx);
            }
        }
        if std::mem::take(&mut self.comment_clear) {
            self.comment.update(cx, |c, cx| c.set_value("", window, cx));
        }
        self.sync_items();
        if self.text_stale {
            self.sync_text(window, cx);
        }
        self.find.sync(self, cx);
        // The first read failed (offline, GitHub down): the list's copy alone isn't the page.
        if let (None, Some(e)) = (&self.detail, &self.detail_error) {
            let retry = cx.listener(|this, _, _, cx| this.load(cx));
            return div()
                .id("issue")
                .track_focus(&self.focus)
                .size_full()
                .child(load_failed("issue", e, self.loading, retry, &t, cx))
                .into_any_element();
        }
        if self.item().is_none() {
            return div()
                .id("issue")
                .track_focus(&self.focus)
                .size_full()
                .p(px(32.))
                .text_center()
                .text_size(rems(0.78))
                .text_color(t.muted_foreground)
                .child("Loading the issue…")
                .into_any_element();
        }
        let weak = cx.weak_entity();
        let page = list(self.page.clone(), move |ix, _, cx| {
            weak.update(cx, |this, cx| this.page_item(ix, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full();
        div()
            .relative()
            .size_full()
            .on_action(cx.listener(|this, _: &editor::Find, window, cx| this.open_find(window, cx)))
            .child(
                div()
                    .id("issue")
                    .key_context("IssueView")
                    .track_focus(&self.focus)
                    .size_full()
                    .map(|el| scroll::on_keys(el, Self::scroll_key, cx))
                    .child(page),
            )
            .children(self.find.bar(cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gitviber_core::github::{Comment, Label};
    use gpui_kit::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::leak_tests::{issue, setup};

    /// The workspace, which the page holds weakly, and the page.
    struct Host(Entity<Workspace>, Entity<IssueView>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(900.)).h(px(600.)).child(self.1.clone())
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// Fills the page as a read would: `n` comments of a few paragraphs, a list and code.
    fn fill_long(v: &mut IssueView, n: usize) {
        let body = "A paragraph long enough to wrap once or twice at the page's width, about \
                    what changed and why it matters for the issue.\n\n- one point\n- another \
                    point\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n";
        v.detail = Some(Arc::new(IssueDetail {
            issue: crate::ui::core_issue(&issue(1)),
            body: "The body".into(),
            thread: (0..n)
                .map(|i| Comment {
                    author: format!("user{i}"),
                    body: format!("Comment {i}. {body}"),
                    created_at: "2026-01-01T00:00:00Z".into(),
                    review: None,
                })
                .collect(),
            closed_by: None,
        }));
        v.detail_error = None;
        v.detail_at = Some(Instant::now());
        v.text_stale = true;
    }

    fn open_long(n: usize, cx: &mut TestAppContext) -> (Entity<IssueView>, &mut VisualTestContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            let i = issue(1);
            let view =
                cx.new(|cx| IssueView::new(ws.downgrade(), i.url.clone(), Some(i), window, cx));
            Host(ws, view)
        });
        cx.run_until_parked();
        let view = host.read_with(cx, |h, _| h.1.clone());
        view.update(cx, |v, cx| {
            fill_long(v, n);
            cx.notify();
        });
        draw(cx);
        draw(cx);
        (view, cx)
    }

    /// Frame times on a long thread, scrolling top to bottom: `--run-ignored only`.
    #[gpui_kit::test]
    #[ignore = "a measurement, not a check"]
    fn long_page_frame_times(cx: &mut TestAppContext) {
        for n in [0, 300] {
            let (view, cx) = open_long(n, cx);
            let mut frames = vec![];
            for _ in 0..60 {
                view.update(cx, |v, cx| {
                    v.page.scroll_by(px(400.));
                    cx.notify();
                });
                let at = Instant::now();
                draw(cx);
                frames.push(at.elapsed());
            }
            frames.sort();
            eprintln!(
                "{n} comments: median {:?}, p90 {:?}, max {:?}",
                frames[frames.len() / 2],
                frames[frames.len() * 9 / 10],
                frames[frames.len() - 1]
            );
        }
    }

    fn shown(view: &Entity<IssueView>, item: usize, cx: &mut VisualTestContext) -> bool {
        view.read_with(cx, |v, _| {
            let view = v.page.viewport_bounds();
            v.page
                .bounds_for_item(item)
                .is_some_and(|b| b.intersects(&view))
        })
    }

    #[gpui_kit::test]
    fn a_long_thread_builds_only_what_is_in_view(cx: &mut TestAppContext) {
        let (view, cx) = open_long(300, cx);
        view.read_with(cx, |v, _| {
            // The issue, 300 comments, the box; the last ones never built.
            assert_eq!(v.page.item_count(), 302);
            assert!(v.page.bounds_for_item(300).is_none());
        });
        // A comment read since goes in above the box.
        view.update(cx, |v, cx| {
            fill_long(v, 301);
            cx.notify();
        });
        draw(cx);
        assert_eq!(view.read_with(cx, |v, _| v.page.item_count()), 303);
        view.update(cx, |v, cx| {
            v.reveal_item(302);
            cx.notify();
        });
        draw(cx);
        assert!(shown(&view, 302, cx));
    }

    #[gpui_kit::test]
    fn the_box_takes_keys_while_the_thread_is_scrolled_away(cx: &mut TestAppContext) {
        let (view, cx) = open_long(300, cx);
        let input = view.read_with(cx, |v, cx| v.comment.read(cx).input().clone());
        view.update(cx, |v, cx| {
            v.reveal_item(301);
            cx.notify();
        });
        draw(cx);
        cx.update(|window, cx| input.update(cx, |i, cx| i.focus(window, cx)));
        draw(cx);
        view.update(cx, |v, cx| {
            v.page.scroll_to(gpui_kit::ListOffset::default());
            cx.notify();
        });
        draw(cx);
        assert!(!shown(&view, 301, cx));
        cx.simulate_input("still here");
        draw(cx);
        assert_eq!(view.read_with(cx, |v, cx| v.text(cx)), "still here");
    }

    #[gpui_kit::test]
    fn find_reaches_a_comment_far_down_a_long_thread(cx: &mut TestAppContext) {
        let (view, cx) = open_long(300, cx);
        view.update(cx, |v, cx| {
            let d = Arc::get_mut(v.detail.as_mut().unwrap()).unwrap();
            d.thread[250].body = "the needle is here".into();
            d.thread[280].author = "needle-bot".into();
            v.text_stale = true;
            cx.notify();
        });
        draw(cx);
        cx.update(|window, cx| view.update(cx, |v, cx| v.open_find(window, cx)));
        cx.simulate_input("needle");
        draw(cx);
        draw(cx);
        let finder = view.read_with(cx, |v, _| v.find.finder()).expect("open");
        assert_eq!(finder.read_with(cx, |f, _| f.state().0), Some(0));
        // The first: in comment 250's text, which its view scrolled to.
        assert!(shown(&view, 251, cx));
        // The next: comment 280's author, drawn as the list brought it in.
        cx.simulate_keystrokes("enter");
        draw(cx);
        draw(cx);
        assert!(shown(&view, 281, cx));
        assert_eq!(finder.read_with(cx, |f, _| f.state()), (Some(1), 2, None));
    }

    #[gpui_kit::test]
    fn a_first_read_that_fails_says_why_and_offers_retry(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            let i = issue(1);
            let view =
                cx.new(|cx| IssueView::new(ws.downgrade(), i.url.clone(), Some(i), window, cx));
            Host(ws, view)
        });
        // The read finds no repository, as it would find no network.
        cx.run_until_parked();
        draw(cx);
        let view = host.read_with(cx, |h, _| h.1.clone());
        let retry = cx.debug_bounds("load-retry").expect("Retry shown");
        let before = view.read_with(cx, |v, _| v.detail_at);
        cx.simulate_click(retry.center(), gpui_kit::Modifiers::none());
        // Read again (and failed again: there's still no repository).
        assert!(view.read_with(cx, |v, _| v.detail_at) > before);
        cx.run_until_parked();
        draw(cx);
        assert!(view.read_with(cx, |v, _| v.detail_error.is_some()));
        assert!(cx.debug_bounds("load-retry").is_some());
    }

    #[gpui_kit::test]
    fn find_reaches_the_labels_assignees_and_comments(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            let i = issue(1);
            let view =
                cx.new(|cx| IssueView::new(ws.downgrade(), i.url.clone(), Some(i), window, cx));
            Host(ws, view)
        });
        cx.run_until_parked();
        let (_ws, view) = host.read_with(cx, |h, _| (h.0.clone(), h.1.clone()));
        view.update(cx, |v, cx| {
            let mut i = crate::ui::core_issue(&issue(1));
            i.title = "zeta crash".into();
            i.assignees = vec!["zeta-x".into()];
            i.labels = ["zeta-bug", "other"]
                .map(|name| Label {
                    name: name.into(),
                    color: "ff0000".into(),
                    description: String::new(),
                })
                .into();
            v.detail = Some(Arc::new(IssueDetail {
                issue: i,
                body: "The zeta body".into(),
                thread: vec![Comment {
                    author: "Bob".into(),
                    body: "zeta again".into(),
                    created_at: "2026-01-01T00:00:00Z".into(),
                    review: None,
                }],
                closed_by: None,
            }));
            v.detail_error = None;
            v.detail_at = Some(Instant::now());
            v.text_stale = true;
            cx.notify();
        });
        draw(cx);
        cx.update(|window, cx| view.update(cx, |v, cx| v.open_find(window, cx)));
        cx.simulate_input("zeta");
        draw(cx);
        let finder = view.read_with(cx, |v, _| v.find.finder()).expect("open");
        // The title, "assigned to zeta-x", the label, the description and the comment.
        assert_eq!(finder.read_with(cx, |f, _| f.state()), (Some(0), 5, None));
        for n in 1..=5 {
            cx.simulate_keystrokes("enter");
            draw(cx);
            let (current, _, waiting) = finder.read_with(cx, |f, _| f.state());
            assert_eq!(current, Some(n % 5));
            assert_eq!(waiting, None, "match {n} was never drawn");
        }
    }

    /// How far the page is scrolled, how far it can be, the view's height, and item `last` in
    /// view, drawn.
    fn scrolled(
        view: &Entity<IssueView>,
        last: usize,
        cx: &mut VisualTestContext,
    ) -> (f32, f32, f32, bool) {
        draw(cx);
        draw(cx);
        let at = view.read_with(cx, |v, _| {
            (
                f32::from(-v.page.scroll_px_offset_for_scrollbar().y),
                f32::from(v.page.max_offset_for_scrollbar().y),
                f32::from(v.page.viewport_bounds().size.height),
            )
        });
        (at.0, at.1, at.2, shown(view, last, cx))
    }

    #[gpui_kit::test]
    fn a_long_thread_scrolls_from_the_keyboard_but_not_from_the_box(cx: &mut TestAppContext) {
        let (view, cx) = open_long(300, cx);
        let focus = view.read_with(cx, |v, _| v.focus.clone());
        cx.update(|window, cx| focus.focus(window, cx));
        let (_, _, height, _) = scrolled(&view, 301, cx);
        cx.simulate_keystrokes("pagedown");
        let (top, ..) = scrolled(&view, 301, cx);
        assert!((top - height * scroll::PAGE).abs() < 0.5, "{top} {height}");
        // To the box at the end, which was never drawn.
        cx.simulate_keystrokes("end");
        let (top, max, _, last) = scrolled(&view, 301, cx);
        assert!(
            last && top > height && (top - max).abs() < 0.5,
            "{top} of {max}"
        );
        let input = view.read_with(cx, |v, cx| v.comment.read(cx).input().clone());
        cx.update(|window, cx| input.update(cx, |i, cx| i.focus(window, cx)));
        cx.simulate_keystrokes("home pageup up");
        assert_eq!(scrolled(&view, 301, cx).0, top);
        cx.update(|window, cx| focus.focus(window, cx));
        cx.simulate_keystrokes("home");
        assert_eq!(scrolled(&view, 301, cx).0, 0.);
    }
}
