//! A commit's tabs and a range's: who made the commit and when, its signature and trailers, the
//! files it changed and the diff of the one picked; or one file of it alone (a "commit"
//! selection); or every file between two commits (Compare's Open all). Closing the tab drops
//! the diff and everything it read.

use std::rc::Rc;

use gitviber_core::{api, git};
use gitviber_diff::{Compare, DiffEvent, DiffSource, DiffView};
use gitviber_refs::toast::Note;
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{
    ChangeKind, FileIcon, LineCounts, PathLabel, StatusLetter, full_date, now_unix, relative_time,
};
use gitviber_ui::workspace::{TabIcon, TabItem};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, UniformListScrollHandle,
    Window, div, prelude::FluentBuilder as _, px, rems, uniform_list,
};

use crate::commit::Commit;

/// What the tab shows.
#[derive(Clone)]
pub enum Of {
    /// One file of a commit.
    File {
        commit: Rc<Commit>,
        file: git::FileChange,
    },
    /// Every file of a commit.
    Commit(Rc<Commit>),
    /// Every file between two commits: `label` names the comparison.
    Range {
        base: String,
        head: String,
        label: String,
    },
}

pub struct CommitView {
    of: Of,
    /// The commit, for the header; a range has none.
    commit: Option<Rc<Commit>>,
    /// The commit on GitHub, when it's there.
    url: Option<String>,
    files: Option<Rc<Vec<git::FileChange>>>,
    error: Option<SharedString>,
    selected: Option<usize>,
    /// Asked for before the files came in.
    wanted: Option<String>,
    diff: Option<Entity<DiffView>>,
    details: Option<git::CommitDetails>,
    body_open: bool,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    _files: Task<()>,
    _details: Task<()>,
    _diff: Option<Subscription>,
    /// The review notes on the diff's lines, on the commit's version.
    _notes: Option<Entity<gitviber_review::DiffNotes>>,
    _live: Live,
}

impl CommitView {
    /// `url`: the commit's GitHub page, when it's there.
    pub fn new(of: Of, url: Option<String>, cx: &mut Context<Self>) -> Self {
        let core = gitviber_ui::core(cx);
        let commit = match &of {
            Of::File { commit, .. } | Of::Commit(commit) => Some(commit.clone()),
            Of::Range { .. } => None,
        };
        enum Read {
            File(git::FileChange),
            Commit(String),
            Range(String, String),
        }
        let what = match &of {
            Of::File { file, .. } => Read::File(file.clone()),
            Of::Commit(c) => Read::Commit(c.sha.clone()),
            Of::Range { base, head, .. } => Read::Range(base.clone(), head.clone()),
        };
        let read = cx.background_spawn({
            let core = core.clone();
            async move {
                match what {
                    Read::File(file) => Ok(vec![file]),
                    Read::Commit(sha) => api::history::commit_files(&core, sha),
                    Read::Range(base, head) => api::history::range_files(&core, base, head),
                }
            }
        });
        let path = match &of {
            Of::File { file, .. } => Some(file.path.clone()),
            _ => None,
        };
        let files = cx.spawn(async move |this, cx| {
            let files = read.await;
            let _ = this.update(cx, |this, cx| {
                match files {
                    Ok(f) => {
                        this.files = Some(Rc::new(f));
                        let at = this
                            .wanted
                            .take()
                            .and_then(|p| this.index_of(&p))
                            .unwrap_or(0);
                        this.select(at, cx);
                    }
                    Err(e) => this.error = Some(e.into()),
                }
                cx.notify();
            });
        });
        // Checking a signature runs gpg or ssh: once per tab, not per file shown.
        let details = match &commit {
            Some(c) => {
                let sha = c.sha.clone();
                let read =
                    cx.background_spawn(async move { api::history::commit_details(&core, sha) });
                cx.spawn(async move |this, cx| {
                    let details = read.await;
                    let _ = this.update(cx, |this, cx| {
                        this.details = details.ok();
                        cx.notify();
                    });
                })
            }
            None => Task::ready(()),
        };
        CommitView {
            of,
            commit,
            url,
            files: None,
            error: None,
            selected: None,
            wanted: path,
            diff: None,
            details: None,
            body_open: false,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            _files: files,
            _details: details,
            _diff: None,
            _notes: None,
            _live: Live::new("CommitView"),
        }
    }

    pub fn of(&self) -> &Of {
        &self.of
    }

    /// The diff of each file: a commit's against its parent, or a range's.
    fn compare(&self) -> Compare {
        match &self.of {
            Of::File { commit, .. } | Of::Commit(commit) => Compare::Commit {
                sha: commit.sha.clone(),
            },
            Of::Range { base, head, .. } => Compare::Range {
                base: base.clone(),
                head: head.clone(),
            },
        }
    }

    fn index_of(&self, path: &str) -> Option<usize> {
        self.files.as_ref()?.iter().position(|f| f.path == path)
    }

    /// Shows `path`'s diff; once the files are in, if they aren't yet.
    pub fn select_path(&mut self, path: &str, cx: &mut Context<Self>) {
        match self.index_of(path) {
            Some(at) => self.select(at, cx),
            None if self.files.is_none() => self.wanted = Some(path.to_string()),
            None => {}
        }
    }

    fn select(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(file) = self.files.as_ref().and_then(|f| f.get(at)).cloned() else {
            return;
        };
        self.selected = Some(at);
        self.scroll
            .scroll_to_item(at, gpui_kit::ScrollStrategy::Nearest);
        let source = DiffSource::from_change(self.compare(), &file);
        match &self.diff {
            // Refused only over unsaved edits, which a commit's diff never has.
            Some(diff) => {
                diff.update(cx, |d, cx| d.set_source(source, cx));
            }
            None => {
                let diff = cx.new(|cx| DiffView::new(source, cx));
                self._diff = Some(cx.subscribe(&diff, |_, _, e: &DiffEvent, cx| {
                    if let DiffEvent::Notice(n) = e {
                        let note = match n.level {
                            gitviber_diff::NoticeLevel::Error => Note::error(n.title.clone()),
                            gitviber_diff::NoticeLevel::Success => Note::success(n.title.clone()),
                            gitviber_diff::NoticeLevel::Info => Note::info(n.title.clone()),
                        };
                        let note = note.maybe_detail(n.detail.clone());
                        if let Some(handle) = cx.active_window() {
                            note.show_in(handle, cx);
                        }
                    }
                }));
                // A commit's version, as the note says; a range's files take none.
                if let Some(c) = &self.commit {
                    let at = format!("commit {}", c.short);
                    self._notes = Some(gitviber_review::attach_notes(&diff, Some(at), cx));
                }
                self.diff = Some(diff);
            }
        }
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.files.as_ref().map_or(0, |f| f.len());
        if n == 0 {
            return;
        }
        let at = self
            .selected
            .map_or(0, |s| (s as isize + by).clamp(0, n as isize - 1) as usize);
        self.select(at, cx);
    }

    fn render_range_bar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let t = cx.app_theme().clone();
        let Of::Range { base, head, label } = &self.of else {
            return div().into_any_element();
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(t.border)
            .bg(t.panel)
            .px_3()
            .py_2()
            .child(
                Icon::new(IconName::GitCompare)
                    .small()
                    .text_color(t.primary),
            )
            .child(
                div()
                    .text_size(rems(0.78125))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("All Changes · {label}")),
            )
            .child(
                div()
                    .ml_auto()
                    .font_family(t.fonts.code.clone())
                    .text_size(rems(0.71875))
                    .text_color(t.muted_foreground)
                    .child(format!(
                        "{}..{}",
                        &base[..base.len().min(7)],
                        &head[..head.len().min(7)]
                    )),
            )
            .into_any_element()
    }

    fn render_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(c) = self.commit.clone() else {
            return self.render_range_bar(cx);
        };
        // Only commits GitHub has have checks.
        let checks = self
            .url
            .is_some()
            .then(|| crate::commit_checks(&c.sha, true, window, cx))
            .flatten();
        let t = cx.app_theme().clone();
        let now = now_unix();
        let written = relative_time(c.timestamp, now);
        let committed = relative_time(c.committed_at, now);
        let sig = self.details.as_ref().and_then(|d| signature(d, &t));
        let trailers: Vec<(String, String)> = self
            .details
            .as_ref()
            .map(|d| d.trailers.clone())
            .unwrap_or_default();
        let sha = c.sha.clone();
        let url = self.url.clone();
        div()
            .flex_none()
            .border_b_1()
            .border_color(t.border)
            .bg(t.panel)
            .px_3()
            .py_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        Icon::new(IconName::GitCommitHorizontal)
                            .small()
                            .text_color(t.primary),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(rems(0.78125))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(c.subject.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap_2()
                            .text_size(rems(0.71875))
                            .text_color(t.muted_foreground)
                            .child(c.author.clone())
                            .child(div().text_color(t.subtle).child("·"))
                            .child({
                                let tip: SharedString = full_date(c.timestamp).into();
                                div()
                                    .id("written")
                                    .child(written.clone())
                                    .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
                            })
                            // Rebased or cherry-picked: it landed later than it was written, maybe by someone else.
                            .when(committed != written, |d| {
                                let by = if c.committer != c.author {
                                    format!(" by {}", c.committer)
                                } else {
                                    String::new()
                                };
                                let tip: SharedString = full_date(c.committed_at).into();
                                d.child(div().text_color(t.subtle).child("·")).child(
                                    div()
                                        .id("committed")
                                        .child(format!("committed {committed}{by}"))
                                        .tooltip(move |w, cx| {
                                            Tooltip::new(tip.clone()).build(w, cx)
                                        }),
                                )
                            })
                            .children(checks)
                            .when_some(sig, |d, s| d.child(s))
                            .child(
                                div()
                                    .id("sha")
                                    .px(px(6.))
                                    .rounded(px(3.))
                                    .bg(t.elevated)
                                    .font_family(t.fonts.code.clone())
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(t.foreground))
                                    .child(c.short.clone())
                                    .tooltip(|w, cx| {
                                        Tooltip::new("Copy the commit's SHA").build(w, cx)
                                    })
                                    .on_click(move |_, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            sha.clone(),
                                        ));
                                        Note::success("Commit SHA copied").show(window, cx);
                                    }),
                            )
                            .when(!c.body.trim().is_empty(), |d| {
                                d.child(
                                    div()
                                        .id("more")
                                        .text_color(t.primary)
                                        .cursor_pointer()
                                        .child(if self.body_open { "less" } else { "more" })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.body_open = !this.body_open;
                                            cx.notify();
                                        })),
                                )
                            })
                            .when_some(url, |d, url| {
                                d.child(
                                    Button::new("github")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::ExternalLink)
                                        .tooltip("Open on GitHub")
                                        .on_click(move |_, _, cx| cx.open_url(&url)),
                                )
                            }),
                    ),
            )
            .when(!trailers.is_empty(), |d| {
                d.child(
                    div()
                        .mt_1p5()
                        .pl(px(22.))
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .children(trailers.into_iter().enumerate().map(|(i, (key, value))| {
                            // The name, not the email.
                            let shown = value
                                .rsplit_once(" <")
                                .map_or(value.as_str(), |(n, _)| n)
                                .to_string();
                            let tip: SharedString = format!("{key}: {value}").into();
                            div()
                                .id(("trailer", i))
                                .flex()
                                .gap_1()
                                .h(px(20.))
                                .items_center()
                                .px(px(6.))
                                .rounded(px(3.))
                                .bg(t.elevated)
                                .text_size(rems(0.6875))
                                .child(div().text_color(t.subtle).child(key))
                                .child(div().truncate().text_color(t.muted_foreground).child(shown))
                                .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
                        })),
                )
            })
            .when(self.body_open, |d| {
                d.child(
                    div()
                        .id("body")
                        .mt_2()
                        .pl(px(22.))
                        .max_h(px(192.))
                        .overflow_y_scroll()
                        .text_size(rems(0.75))
                        .text_color(t.muted_foreground)
                        .child(c.body.clone()),
                )
            })
            .into_any_element()
    }

    fn render_files(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let Some(files) = self.files.clone() else {
            let text: SharedString = self.error.clone().unwrap_or_else(|| "Loading…".into());
            return div()
                .p_3()
                .text_size(rems(0.71875))
                .text_color(t.subtle)
                .child(text)
                .into_any_element();
        };
        let (add, del) = files.iter().fold((0, 0), |(a, d), f| {
            (a + f.additions.unwrap_or(0), d + f.deletions.unwrap_or(0))
        });
        let selected = self.selected;
        let list = uniform_list(
            "commit-files",
            files.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                let t = cx.app_theme().clone();
                range
                    .map(|i| {
                        let f = &files[i];
                        let active = selected == Some(i);
                        div()
                            .id(("f", i))
                            .relative()
                            .w_full()
                            .overflow_hidden()
                            .flex()
                            .h(px(26.))
                            .items_center()
                            .gap_2()
                            .px_2()
                            .text_size(rems(0.75))
                            .cursor_pointer()
                            .when(active, |d| {
                                d.bg(t.primary.opacity(0.15)).child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .left_0()
                                        .w(px(2.))
                                        .bg(t.primary),
                                )
                            })
                            .when(!active, |d| d.hover(|s| s.bg(t.hover)))
                            .child(FileIcon::file(&f.path))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(PathLabel(f.path.clone().into())),
                            )
                            .child(LineCounts::new(f.additions, f.deletions))
                            .child(StatusLetter(ChangeKind::from_letter(
                                f.status.chars().next().unwrap_or('M'),
                            )))
                            .on_click(cx.listener(move |this, _, _, cx| this.select(i, cx)))
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .w_full()
        .flex_1();
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .h(px(28.))
                    .items_center()
                    .gap_2()
                    .px_2()
                    .text_size(rems(0.65625))
                    .text_color(t.subtle)
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("CHANGED FILES"),
                    )
                    .child(
                        div()
                            .font_family(t.fonts.code.clone())
                            .text_color(t.muted_foreground)
                            .child(self.files.as_ref().map_or(0, |f| f.len()).to_string()),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .flex()
                            .gap_1()
                            .font_family(t.fonts.code.clone())
                            .child(div().text_color(t.added).child(format!("+{add}")))
                            .child(div().text_color(t.removed).child(format!("-{del}"))),
                    ),
            )
            .child(list)
            .into_any_element()
    }
}

/// Nothing for an unsigned commit, unless commit.gpgSign says it should have been signed.
fn signature(d: &git::CommitDetails, t: &gitviber_ui::AppTheme) -> Option<impl IntoElement> {
    let signer = if d.signer.is_empty() {
        "an unknown key".to_string()
    } else {
        d.signer.clone()
    };
    let (label, color, icon, tip) = match d.signature.as_str() {
        "G" => (
            "Verified",
            t.added,
            IconName::ShieldCheck,
            format!("Good signature from {signer}"),
        ),
        "U" => (
            "Signed",
            t.added,
            IconName::ShieldCheck,
            format!("Good signature from {signer}, whose key isn't marked as trusted"),
        ),
        "X" => (
            "Signed",
            t.modified,
            IconName::ShieldAlert,
            format!("Good signature from {signer}, but the signature has expired"),
        ),
        "Y" => (
            "Signed",
            t.modified,
            IconName::ShieldAlert,
            format!("Good signature from {signer}, made with a key that has since expired"),
        ),
        "R" => (
            "Revoked key",
            t.removed,
            IconName::ShieldX,
            format!("Signed by {signer} with a key that has been revoked"),
        ),
        "B" => (
            "Bad signature",
            t.removed,
            IconName::ShieldX,
            "The signature doesn't match this commit".to_string(),
        ),
        "E" => (
            "Signed",
            t.subtle,
            IconName::ShieldAlert,
            "Signed, but git couldn't check it: the key is missing or verification isn't set up"
                .to_string(),
        ),
        _ if d.sign_expected => (
            "Unsigned",
            t.modified,
            IconName::ShieldAlert,
            "commit.gpgSign is on, but this commit has no signature".to_string(),
        ),
        _ => return None,
    };
    let tip: SharedString = tip.into();
    Some(
        div()
            .id("signature")
            .flex()
            .items_center()
            .gap_1()
            .text_color(color)
            .child(Icon::new(icon).small())
            .child(label)
            .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx)),
    )
}

impl Focusable for CommitView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TabItem for CommitView {
    fn tab_title(&self, _: &App) -> SharedString {
        match &self.of {
            Of::File { file, .. } => file
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&file.path)
                .to_string()
                .into(),
            Of::Commit(c) => format!("Commit {}", c.short).into(),
            Of::Range { label, .. } => format!("All Changes · {label}").into(),
        }
    }

    fn tab_detail(&self, _: &App) -> Option<SharedString> {
        match &self.of {
            Of::File { commit, .. } => Some(commit.short.clone()),
            _ => {
                let f = self.files.as_ref()?.get(self.selected?)?;
                Some(
                    f.path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&f.path)
                        .to_string()
                        .into(),
                )
            }
        }
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        match &self.of {
            Of::File { file, .. } => TabIcon::File(file.path.clone().into()),
            Of::Commit(_) => TabIcon::Icon(IconName::GitCommitHorizontal),
            Of::Range { .. } => TabIcon::Icon(IconName::GitCompare),
        }
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        match &self.of {
            Of::File { commit, file } => Some(format!("{} · {}", file.path, commit.subject).into()),
            Of::Commit(c) => Some(c.subject.clone()),
            Of::Range { base, head, .. } => Some(format!("{base}..{head}").into()),
        }
    }

    fn tab_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(
            self.diff
                .as_ref()
                .map_or_else(|| self.focus.clone(), |d| d.focus_handle(cx)),
        )
    }
}

impl Render for CommitView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &gpui_kit::KeyDownEvent, _, cx| {
                match e.keystroke.key.as_str() {
                    // ⌥↑/⌥↓ walk the files, as the arrows walk a list.
                    "down" if e.keystroke.modifiers.alt => this.step(1, cx),
                    "up" if e.keystroke.modifiers.alt => this.step(-1, cx),
                    _ => {}
                }
            }))
            .child(self.render_bar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(!matches!(self.of, Of::File { .. }), |d| {
                        d.child(
                            div()
                                .w(px(280.))
                                .flex_none()
                                .h_full()
                                .border_r_1()
                                .border_color(t.border)
                                .bg(t.sidebar)
                                .child(self.render_files(cx)),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .children(self.diff.clone())
                            .when(
                                self.diff.is_none()
                                    && self.files.as_ref().is_some_and(|f| f.is_empty()),
                                |d| {
                                    d.flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(t.subtle)
                                        .child("This commit changes no files.")
                                },
                            ),
                    ),
            )
    }
}
