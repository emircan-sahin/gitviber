//! The left sidebar's Issues: origin's (and a fork's original's beside them), narrowed by the
//! chips and labels. The list holds GitHub's 50 most recent; it draws only the rows on screen.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gitviber_core::api;
use gitviber_ui::actions::editor;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::CountBadge;
use gitviber_ui::workspace::{Panel, PanelContext, ProjectEvent};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    rems, uniform_list,
};

use crate::cache::{Cache, MIN_AGE, Wakes};
use crate::logic::narrow::{Filter, ListKind, Narrow, compact_count, empty_text};
use crate::model::{Account, Counts, IssueItem, LabelItem, Target, is_not_connected};
use crate::ui::common::{ago, empty_note, issue_icon, label_dot, list_error, signed_in_as, theme};
use crate::ui::connect::{ConnectView, gate_view};
use crate::ui::dialogs::CreateIssueDialog;
use crate::ui::labels::LabelPicker;
use crate::ui::list::{
    ListControls, ListPanel, filter_tabs, find_box, narrow_bar, selected_labels,
};
use crate::ui::pulls_panel::{main_of, use_project};
use crate::ui::rows::{ROW, RowKey, Rows, focus_page, row_key};

pub struct IssuesPanel {
    ctx: PanelContext,
    _root: PathBuf,
    controls: ListControls,
    asked: Option<(String, Wakes, Option<String>)>,
    stamp: u64,
    connect: Option<Entity<ConnectView>>,
    focus: FocusHandle,
    /// Each pane's rows and their keyboard: origin's, and a fork's original's.
    rows: [Rows; 2],
    _origin: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl ListPanel for IssuesPanel {
    fn controls(&mut self) -> &mut ListControls {
        &mut self.controls
    }

    fn narrowed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.controls.save_picked(cx);
        self.ask(false, cx);
    }

    fn focus_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rows[0].focus.focus(window, cx);
    }
}

impl Panel for IssuesPanel {
    fn panel_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.rows[0].focus.clone())
    }
}

impl IssuesPanel {
    pub fn new(ctx: &PanelContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let root = ctx
            .project
            .as_ref()
            .map(|p| p.read(cx).root().to_path_buf())
            .unwrap_or_default();
        use_project(&root, cx);
        let controls = ListControls::new(ListKind::Issues, main_of(&root), window, cx);
        let mut subs = vec![
            cx.observe_global::<Cache>(|this: &mut Self, cx| {
                let stamp = this.stamp_now(cx);
                if stamp != this.stamp || this.asked.as_ref().map(|a| a.1) != Some(Cache::wakes(cx))
                {
                    this.stamp = stamp;
                    cx.notify();
                }
            }),
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    Cache::wake(cx);
                }
            }),
        ];
        if let Some(project) = &ctx.project {
            subs.push(
                cx.subscribe(project, |this: &mut Self, _, ev: &ProjectEvent, cx| {
                    if matches!(ev, ProjectEvent::RepoChanged(c) if c.git) {
                        this.read_origin(cx);
                    }
                }),
            );
        }
        let mut panel = IssuesPanel {
            ctx: ctx.clone(),
            _root: root,
            controls,
            asked: None,
            stamp: 0,
            connect: None,
            focus: cx.focus_handle(),
            rows: [Rows::new(cx), Rows::new(cx)],
            _origin: Task::ready(()),
            _subs: subs,
            _live: Live::new("IssuesPanel"),
        };
        panel.read_origin(cx);
        panel
    }

    /// Origin's URL, as the PRs panel reads it with the status: a new one is another repository
    /// (its lists read again), and the sign-in screen names it. One `git remote -v`.
    fn read_origin(&mut self, cx: &mut Context<Self>) {
        let root = self._root.clone();
        self._origin = cx.spawn(async move |_, cx| {
            let remotes = cx
                .background_spawn(async move { gitviber_core::git::remote_urls(&root) })
                .await;
            // Git couldn't say: that isn't "no origin".
            let Ok(remotes) = remotes else { return };
            let origin = remotes.into_iter().find(|(n, _)| n == "origin");
            cx.update(|cx| Cache::set_origin(cx, origin.and_then(|(_, url)| url)));
        });
    }

    fn account(&self, cx: &App) -> Option<Arc<Account>> {
        crate::account::account(cx)
    }

    /// owner/name of a fork's original, when it has issues on.
    fn upstream(&self, cx: &App) -> Option<String> {
        self.account(cx).and_then(|a| {
            a.parent
                .as_ref()
                .filter(|p| p.issues)
                .map(|p| p.full_name())
        })
    }

    fn me_usable(&self, cx: &App) -> bool {
        let read = crate::account::current_account(cx);
        read.error.is_none() || read.data.is_some()
    }

    fn narrow(&self, cx: &App) -> Narrow {
        self.controls.narrow(self.me_usable(cx))
    }

    fn list_key(name: &str, filter: Filter, narrow: &Narrow) -> String {
        format!("issues:{name}:{}:{}", filter.as_str(), narrow.key())
    }

    fn counts_key(name: &str, narrow: &Narrow) -> String {
        format!("issues:counts:{name}:{}", narrow.key())
    }

    fn stamp_now(&self, cx: &App) -> u64 {
        let narrow = self.narrow(cx);
        let mut keys = vec![
            crate::account::KEY.to_string(),
            Self::list_key("origin", self.controls.filter, &narrow),
            Self::counts_key("origin", &narrow),
        ];
        if let Some(up) = self.upstream(cx) {
            keys.push(Self::list_key(&up, self.controls.filter, &narrow));
            keys.push(Self::counts_key(&up, &narrow));
        }
        Cache::stamp(cx, &keys.iter().map(String::as_str).collect::<Vec<_>>())
    }

    fn signature(&self, cx: &App) -> String {
        format!(
            "{}|{}",
            self.narrow(cx).key(),
            self.controls.filter.as_str()
        )
    }

    fn ask(&mut self, force: bool, cx: &mut Context<Self>) {
        let age = if force { Duration::ZERO } else { MIN_AGE };
        crate::account::load(cx, force);
        let (narrow, filter, upstream) = (self.narrow(cx), self.controls.filter, self.upstream(cx));
        for (target, name) in std::iter::once((None, "origin".to_string()))
            .chain(upstream.clone().map(|u| (Some(u.clone()), u)))
        {
            let (t, n) = (target.clone(), narrow.clone());
            Cache::load(
                cx,
                &Self::list_key(&name, filter, &narrow),
                age,
                move |core| {
                    api::github::issue_list(core, t.clone(), filter.as_str().into(), n.to_core())
                        .map(|is| is.iter().map(IssueItem::from).collect::<Vec<_>>())
                },
            );
            let (t, n) = (target, narrow.clone());
            Cache::load(cx, &Self::counts_key(&name, &narrow), age, move |core| {
                api::github::issue_counts(core, t.clone(), n.to_core()).map(|c| Counts::from(&c))
            });
        }
        self.asked = Some((self.signature(cx), Cache::wakes(cx), upstream));
    }

    fn open(&self, item: IssueItem, preview: bool, window: &mut Window, cx: &mut App) {
        crate::ui::open_issue(&self.ctx.workspace, item, preview, window, cx);
    }

    fn create(&self, target: Target, repo: String, window: &mut Window, cx: &mut App) {
        CreateIssueDialog::open(self.ctx.workspace.clone(), target, repo, window, cx);
    }

    fn pick_labels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repos = std::iter::once(None)
            .chain(self.upstream(cx).map(Some))
            .collect();
        let weak = cx.weak_entity();
        let hint = (self.controls.labels.len() > 1).then(|| "Issues with all of them".into());
        LabelPicker::open(
            repos,
            self.controls.labels.clone(),
            hint,
            std::rc::Rc::new(move |labels, window, cx| {
                weak.update(cx, |this, cx| {
                    this.controls.labels = labels;
                    this.narrowed(window, cx);
                    cx.notify();
                })
                .ok();
            }),
            None,
            window,
            cx,
        );
    }

    /// A key on a pane's rows (see `rows`).
    fn row_key(
        &mut self,
        pane: usize,
        rows: &[IssueItem],
        ev: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let urls: Vec<&str> = rows.iter().map(|i| i.url.as_str()).collect();
        let active = self.ctx.workspace.upgrade().and_then(|w| {
            w.read(cx)
                .tabs()
                .active_key()
                .and_then(|k| k.strip_prefix("issue:").map(str::to_string))
        });
        let nav = &mut self.rows[pane];
        let at = nav.at(&urls, active.as_deref());
        let Some(key) = row_key(ev, at, urls.len(), false) else {
            return;
        };
        cx.stop_propagation();
        let at = at.unwrap_or(0);
        match key {
            RowKey::Move(to) => {
                nav.put(&urls, to);
                self.open(rows[to].clone(), true, window, cx);
            }
            RowKey::Keep => self.open(rows[at].clone(), false, window, cx),
            RowKey::Preview => self.open(rows[at].clone(), true, window, cx),
            RowKey::Page => {
                if active.as_deref() != Some(urls[at]) {
                    self.open(rows[at].clone(), true, window, cx);
                }
                focus_page(&self.ctx.workspace, window, cx);
            }
            RowKey::More => {}
            RowKey::Menu => {
                nav.put(&urls, at);
                nav.open_menu(at, window, cx);
            }
        }
        cx.notify();
    }

    fn rows_view(
        &self,
        pane: usize,
        name: &str,
        roomy: bool,
        failure: Option<String>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        let narrow = self.narrow(cx);
        let read =
            Cache::read::<Vec<IssueItem>>(cx, &Self::list_key(name, self.controls.filter, &narrow));
        let error = failure.or(read.error);
        let loaded = read.data;
        let rows: Arc<Vec<IssueItem>> = match &loaded {
            Some(l) if !self.controls.needle.is_empty() => Arc::new(
                l.iter()
                    .filter(|i| {
                        let mut fields: Vec<&str> = vec![&i.title, &i.author];
                        let n = format!("#{}", i.number);
                        fields.push(&n);
                        fields.extend(i.labels.iter().map(|l| l.name.as_str()));
                        self.controls.matches(&fields)
                    })
                    .cloned()
                    .collect(),
            ),
            Some(l) => l.clone(),
            None => Arc::new(vec![]),
        };
        let mut col = div().flex().flex_col().size_full().min_h_0();
        if let Some(e) = &error {
            col = col.child(list_error(
                ("list-error", pane),
                e,
                loaded.is_some(),
                &t,
                cx,
            ));
        }
        if loaded.is_some() && rows.is_empty() {
            let any = loaded.as_ref().is_some_and(|l| !l.is_empty());
            let text = if !self.controls.needle.is_empty() && any {
                format!(
                    "None of the {} loaded issues match.",
                    loaded.as_ref().map_or(0, |l| l.len())
                )
            } else {
                empty_text(ListKind::Issues, self.controls.filter, &narrow)
            };
            col = col.child(empty_note(text, roomy, &t));
            if narrow.is_narrowed() && !(any && !self.controls.needle.is_empty()) {
                col = col.child(
                    div().flex().justify_center().child(
                        Button::new(("clear-narrow", pane))
                            .ghost()
                            .xsmall()
                            .label("Clear filter")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.controls.clear(cx);
                                this.narrowed(window, cx);
                            })),
                    ),
                );
            }
        }
        let active = self
            .ctx
            .workspace
            .upgrade()
            .and_then(|w| w.read(cx).tabs().active_key().cloned());
        let weak = cx.weak_entity();
        let count = rows.len();
        let nav = &self.rows[pane];
        let cursor = nav.cursor.clone().filter(|_| nav.is_focused(window));
        let key_rows = rows.clone();
        col.child(
            div()
                .id(("issue-rows", pane))
                .track_focus(&nav.focus)
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .on_key_down(
                    cx.listener(move |this, ev: &gpui::KeyDownEvent, window, cx| {
                        this.row_key(pane, &key_rows, ev, window, cx)
                    }),
                )
                .child(
                    uniform_list(("issues", pane), count, move |range, _, cx| {
                        let t = theme(cx);
                        range
                            .map(|ix| {
                                issue_row(
                                    pane,
                                    ix,
                                    &rows[ix],
                                    active.as_ref(),
                                    cursor.as_deref(),
                                    weak.clone(),
                                    &t,
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .track_scroll(&nav.scroll)
                    .flex_1()
                    .min_h_0(),
                ),
        )
        .into_any_element()
    }
}

fn issue_row(
    pane: usize,
    ix: usize,
    i: &IssueItem,
    active: Option<&SharedString>,
    cursor: Option<&str>,
    panel: WeakEntity<IssuesPanel>,
    t: &gitviber_ui::AppTheme,
) -> AnyElement {
    let on = active.is_some_and(|a| a.as_ref() == format!("issue:{}", i.url));
    let keys_here = cursor == Some(i.url.as_str());
    let (author, author_panel, underline) = (i.author.clone(), panel.clone(), t.foreground);
    let hover = t.hover;
    let item = i.clone();
    let mut meta = div()
        .mt(px(2.))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(rems(0.656))
        .text_color(t.subtle)
        .child(
            div()
                .flex_none()
                .font_family(t.fonts.code.clone())
                .child(format!("#{}", i.number)),
        )
        .child("·")
        // A click on the author narrows the list to theirs, as GitHub's lists do.
        .child(
            div()
                .id(("author", pane * 100_000 + ix))
                .flex_none()
                .cursor_pointer()
                .hover(move |s| s.text_color(underline).underline())
                .child(i.author.clone())
                .on_click(move |ev, window, cx| {
                    let m = ev.modifiers();
                    if m.platform || m.control || m.shift || m.alt {
                        return;
                    }
                    cx.stop_propagation();
                    let author = author.clone();
                    author_panel
                        .update(cx, |this, cx| {
                            this.controls.set_author(Some(author), cx);
                            this.narrowed(window, cx);
                            cx.notify();
                        })
                        .ok();
                }),
        );
    if i.comments > 0 {
        meta = meta.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(2.))
                .child(Icon::new(IconName::MessageSquare).xsmall())
                .child(i.comments.to_string()),
        );
    }
    let mut chips = div()
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .gap(px(4.));
    for (li, l) in i.labels.iter().enumerate() {
        let (label, panel) = (l.clone(), panel.clone());
        chips = chips.child(
            div()
                .id(("label", ix * 64 + li))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(3.))
                .max_w(px(120.))
                .px(px(5.))
                .rounded_full()
                .border_1()
                .border_color(t.border)
                .cursor_pointer()
                .child(label_dot(l, t))
                .child(div().truncate().child(l.name.clone()))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    let label = label.clone();
                    panel
                        .update(cx, |this, cx| this.filter_label(label, window, cx))
                        .ok();
                }),
        );
    }
    meta = meta
        .child(chips)
        .child(div().flex_none().child(ago(&i.updated_at)));
    let (menu_item, menu_panel) = (i.clone(), panel.clone());
    div()
        .id(("issue", pane * 100_000 + ix))
        .relative()
        .w_full()
        .h(px(ROW))
        .flex()
        .gap(px(10.))
        .py(px(6.))
        .pl(px(12.))
        .pr(px(8.))
        .cursor_pointer()
        .when(on, |d| {
            d.bg(t.primary.opacity(0.15)).child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(2.))
                    .bg(t.primary),
            )
        })
        .when(!on, |d| d.hover(move |s| s.bg(hover)))
        .when(keys_here && !on, |d| d.bg(hover))
        .when(keys_here, |d| d.border_1().border_color(t.ring))
        .child(div().mt(px(2.)).child(issue_icon(i, t)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(rems(0.75))
                        .text_color(t.foreground.opacity(0.9))
                        .child(i.title.clone()),
                )
                .child(meta),
        )
        .on_click(move |ev, window, cx| {
            let pin = ev.click_count() > 1;
            let item = item.clone();
            panel
                .update(cx, |this, cx| {
                    this.rows[pane].cursor = Some(item.url.clone());
                    this.open(item, !pin, window, cx)
                })
                .ok();
        })
        .context_menu(move |menu, _, _| {
            let i = menu_item.clone();
            let (author, panel) = (i.author.clone(), menu_panel.clone());
            let mut menu = menu.item(
                PopupMenuItem::new(format!("Filter by author {author}"))
                    .icon(IconName::ListFilter)
                    .on_click(move |_, window, cx| {
                        let author = author.clone();
                        panel
                            .update(cx, |this, cx| {
                                this.controls.set_author(Some(author), cx);
                                this.narrowed(window, cx);
                                cx.notify();
                            })
                            .ok();
                    }),
            );
            for l in i.labels.clone() {
                let panel = menu_panel.clone();
                menu = menu.item(
                    PopupMenuItem::new(format!("Filter by label {}", l.name))
                        .icon(IconName::ListFilter)
                        .on_click(move |_, window, cx| {
                            let l = l.clone();
                            panel
                                .update(cx, |this, cx| this.filter_label(l, window, cx))
                                .ok();
                        }),
                );
            }
            let (copy_url, open_url) = (i.url.clone(), i.url.clone());
            menu.separator()
                .item(
                    PopupMenuItem::new("Copy link")
                        .icon(IconName::Link)
                        .on_click(move |_, window, cx| {
                            crate::ui::common::copy(&copy_url, "Link copied", window, cx)
                        }),
                )
                .item(
                    PopupMenuItem::new("Open on GitHub")
                        .icon(IconName::ExternalLink)
                        .on_click(move |_, _, cx| crate::ui::common::open_url(&open_url, cx)),
                )
        })
        .into_any_element()
}

impl IssuesPanel {
    fn filter_label(&mut self, label: LabelItem, window: &mut Window, cx: &mut Context<Self>) {
        self.controls.add_label(label);
        self.narrowed(window, cx);
        cx.notify();
    }
}

fn pane_header(
    title: &'static str,
    detail: String,
    count: Option<u64>,
    new: Option<AnyElement>,
    t: &gitviber_ui::AppTheme,
) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(10.))
        .bg(t.panel)
        .border_b_1()
        .border_color(t.border)
        .text_size(rems(0.72))
        .child(div().font_weight(t.semibold()).child(title))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(t.subtle)
                .child(detail),
        )
        .when_some(count, |d, n| d.child(CountBadge::new(compact_count(n))))
        .child(div().flex_1())
        .children(new)
        .into_any_element()
}

impl Render for IssuesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let account = self.account(cx);
        let parent = account.as_ref().and_then(|a| a.parent.clone());
        let upstream = self.upstream(cx);
        let signature = self.signature(cx);
        if self
            .asked
            .as_ref()
            .is_none_or(|(s, w, u)| *s != signature || *w != Cache::wakes(cx) || *u != upstream)
        {
            self.ask(false, cx);
        }
        let acct = crate::account::current_account(cx);
        let narrow = self.narrow(cx);
        let own = Cache::read::<Vec<IssueItem>>(
            cx,
            &Self::list_key("origin", self.controls.filter, &narrow),
        );
        let failure = acct.error.clone().or(own.error.clone());
        if let Some(gate) = gate_view(
            &mut self.connect,
            "issues",
            &acct,
            own.error.as_deref(),
            self.ctx.workspace.clone(),
            &t,
            cx,
        ) {
            return div()
                .id("issues-panel")
                .track_focus(&self.focus)
                .size_full()
                .child(gate)
                .into_any_element();
        }
        let loading = acct.loading || own.loading;
        let origin = account.as_ref().and_then(|a| a.origin.clone());
        let counts = |name: &str, cx: &App| {
            Cache::read::<Counts>(cx, &Self::counts_key(name, &narrow))
                .data
                .map(|c| *c)
        };
        let own_counts = counts("origin", cx);
        let filter = self.controls.filter;
        let pick = |c: Option<Counts>| {
            c.map(|c| match filter {
                Filter::Open => c.open,
                Filter::Closed => c.closed,
                Filter::All => c.open + c.closed,
            })
        };
        let header_counts = if account.is_none()
            || parent.is_some()
            || !origin.as_ref().is_some_and(|o| o.issues)
        {
            None
        } else {
            own_counts
        };
        let me_off = (!self.me_usable(cx)).then(|| {
            if acct.error.as_deref().is_some_and(is_not_connected) {
                "Sign in to GitHub to filter by you"
            } else {
                "Filters by you need your GitHub account, which couldn't be read"
            }
        });
        let origin_name = origin.as_ref().map(|o| o.full_name()).unwrap_or_default();
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(32.))
            .px(px(8.))
            .border_b_1()
            .border_color(t.border)
            .child(filter_tabs(&self.controls, header_counts, cx))
            .child(div().flex_1())
            .child(
                Button::new("pick-labels")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Tag)
                    .label(if self.controls.labels.is_empty() {
                        "Label".to_string()
                    } else {
                        format!("Label {}", self.controls.labels.len())
                    })
                    .tooltip("Filter by label")
                    .on_click(cx.listener(|this, _, window, cx| this.pick_labels(window, cx))),
            )
            .child(
                Button::new("refresh")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RefreshCw)
                    .loading(loading)
                    .tooltip("Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.ask(true, cx))),
            )
            .when(parent.is_none(), |d| {
                let repo = origin_name.clone();
                d.child(
                    Button::new("new-issue")
                        .small()
                        .icon(IconName::Plus)
                        .label("New")
                        .tooltip("New issue")
                        .disabled(origin.is_none())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.create(None, repo.clone(), window, cx)
                        })),
                )
            });
        let off = |repo: &str| {
            div()
                .px(px(16.))
                .py(px(12.))
                .text_center()
                .text_size(rems(0.75))
                .text_color(t.subtle)
                .child(format!("Issues are turned off in {repo}."))
                .into_any_element()
        };
        let body: AnyElement = match &parent {
            Some(parent) => {
                let up = parent.full_name();
                let own_on = origin.as_ref().is_some_and(|o| o.issues);
                let new =
                    |id: &'static str, target: Target, repo: String, cx: &mut Context<Self>| {
                        Button::new(id)
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("New issue")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.create(target.clone(), repo.clone(), window, cx)
                            }))
                            .into_any_element()
                    };
                let own_view = if own_on {
                    self.rows_view(0, "origin", false, failure.clone(), window, cx)
                } else {
                    off(&origin_name)
                };
                let up_view = if parent.issues {
                    self.rows_view(1, &up, false, None, window, cx)
                } else {
                    off(&up)
                };
                let new_own = own_on.then(|| new("new-own", None, origin_name.clone(), cx));
                let new_up = parent
                    .issues
                    .then(|| new("new-up", Some(up.clone()), up.clone(), cx));
                let up_counts = if parent.issues {
                    pick(counts(&up, cx))
                } else {
                    None
                };
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(pane_header(
                        "Your fork",
                        origin_name.clone(),
                        if own_on { pick(own_counts) } else { None },
                        new_own,
                        &t,
                    ))
                    .child(div().flex_1().min_h_0().child(own_view))
                    .child(pane_header("Original", up.clone(), up_counts, new_up, &t))
                    .child(div().flex_1().min_h_0().child(up_view))
                    .into_any_element()
            }
            None => {
                let view = self.rows_view(0, "origin", true, failure.clone(), window, cx);
                div()
                    .flex_1()
                    .min_h_0()
                    .pt(px(4.))
                    .child(view)
                    .into_any_element()
            }
        };
        div()
            .id("issues-panel")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &editor::Find, window, cx| {
                this.controls.open_find(window, cx);
                cx.notify();
            }))
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(narrow_bar(&self.controls, me_off, cx))
            .children(selected_labels(&self.controls.labels, cx))
            .children(find_box(&self.controls, cx))
            .child(body)
            .children(account.map(|a| signed_in_as(&a.login, &a.source, &t)))
            .into_any_element()
    }
}
