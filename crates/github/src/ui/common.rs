//! Pieces the GitHub views share: state icons and pills, CI and label chips, sections, notes, a
//! page's markdown blocks and their attachments, and opening or copying a link.

use std::collections::{HashMap, HashSet};

use gitviber_markdown::{Callouts, MarkdownBase, MarkdownView};
use gitviber_ui::widgets::{now_unix, relative_time};
use gitviber_ui::workspace::{Slot, Toast, ToastRun, Workspace};
use gitviber_ui::{ActiveAppTheme as _, AppTheme};
use gpui_kit::assets::IconName;
use gpui_kit::base::text::{TextView, TextViewStyle};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Context, Div, ElementId, Entity,
    Hsla, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
    rems,
};

use crate::logic::text::{attachment_id, error_markdown, github_markdown};
use crate::model::{
    CiState, IssueItem, LabelItem, PullState, is_expired, is_not_connected, unix,
    unreachable_detail,
};

pub fn theme(cx: &App) -> AppTheme {
    cx.app_theme().clone()
}

/// "3h ago" for a GitHub time.
pub fn ago(iso: &str) -> SharedString {
    unix(iso)
        .map(|t| relative_time(t, now_unix()))
        .unwrap_or_default()
        .into()
}

pub fn pull_icon(state: PullState, draft: bool, t: &AppTheme) -> Icon {
    let (name, color) = match state {
        PullState::Merged => (IconName::GitMerge, t.renamed),
        PullState::Closed => (IconName::GitPullRequestClosed, t.removed),
        PullState::Open if draft => (IconName::GitPullRequestDraft, t.subtle),
        PullState::Open => (IconName::GitPullRequest, t.added),
    };
    Icon::new(name).small().text_color(color)
}

pub fn issue_icon(i: &IssueItem, t: &AppTheme) -> Icon {
    let (name, color) = if i.open {
        (IconName::CircleDot, t.added)
    } else if i.not_planned() {
        (IconName::CircleSlash, t.subtle)
    } else {
        (IconName::CircleCheck, t.renamed)
    };
    Icon::new(name).small().text_color(color)
}

fn pill(
    label: &'static str,
    bg: gpui_kit::Hsla,
    fg: gpui_kit::Hsla,
    border: Option<gpui_kit::Hsla>,
    t: &AppTheme,
) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(3.))
        .bg(bg)
        .text_color(fg)
        .text_size(rems(0.656))
        .font_weight(t.semibold())
        .when_some(border, |d, b| d.border_1().border_color(b))
        .child(label)
}

pub fn pull_pill(state: PullState, draft: bool, t: &AppTheme) -> Div {
    match state {
        PullState::Merged => pill("Merged", t.renamed_fill, t.on_status, None, t),
        PullState::Closed => pill("Closed", t.removed_fill, t.on_status, None, t),
        PullState::Open if draft => pill(
            "Draft",
            t.elevated,
            t.muted_foreground,
            Some(t.border_strong),
            t,
        ),
        PullState::Open => pill("Open", t.added_fill, t.on_status, None, t),
    }
}

pub fn issue_pill(i: &IssueItem, t: &AppTheme) -> Div {
    if i.open {
        pill("Open", t.added_fill, t.on_status, None, t)
    } else if i.not_planned() {
        pill(
            "Closed as not planned",
            t.elevated,
            t.muted_foreground,
            Some(t.border_strong),
            t,
        )
    } else {
        pill("Closed", t.renamed_fill, t.on_status, None, t)
    }
}

/// CI's verdict on a commit, as a small icon with its words in a tooltip; nothing without checks.
pub fn ci_badge(
    id: impl Into<ElementId>,
    state: Option<CiState>,
    t: &AppTheme,
) -> Option<AnyElement> {
    let state = state?;
    let (name, color) = match state {
        CiState::Success => (IconName::CircleCheck, t.added),
        CiState::Failure => (IconName::CircleX, t.removed),
        CiState::Pending => (IconName::CircleDot, t.modified),
    };
    Some(
        div()
            .id(id.into())
            .flex_none()
            .child(Icon::new(name).xsmall().text_color(color))
            .tooltip(move |window, cx| Tooltip::new(state.label()).build(window, cx))
            .into_any_element(),
    )
}

pub fn label_dot(label: &LabelItem, t: &AppTheme) -> Div {
    let color = label
        .rgb()
        .map(|c| gpui_kit::rgb(c).into())
        .unwrap_or(t.subtle);
    div().flex_none().size(px(7.)).rounded_full().bg(color)
}

/// A label as a small chip; `name`: its name as drawn.
pub fn label_chip(label: &LabelItem, name: impl IntoElement, t: &AppTheme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .max_w(px(160.))
        .px(px(6.))
        .rounded_full()
        .border_1()
        .border_color(t.border)
        .text_size(rems(0.656))
        .text_color(t.muted_foreground)
        .child(label_dot(label, t))
        .child(div().truncate().child(name))
}

/// A boxed part of a PR or issue page with its uppercase title.
pub fn section(title: &'static str, aside: Option<SharedString>, t: &AppTheme) -> Div {
    div()
        .mt(px(18.))
        .rounded(px(6.))
        .border_1()
        .border_color(t.border)
        .overflow_hidden()
        .child(section_title(title, aside, t))
}

fn section_title(title: &'static str, aside: Option<SharedString>, t: &AppTheme) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(30.))
        .px(px(12.))
        .bg(t.panel)
        .border_b_1()
        .border_color(t.border)
        .text_size(rems(0.656))
        .font_weight(t.semibold())
        .text_color(t.subtle)
        .child(title.to_uppercase())
        .when_some(aside, |d, a| {
            d.child(div().ml_auto().font_family(t.fonts.code.clone()).child(a))
        })
}

/// One entry of a section drawn as a list's item (a conversation's comment): the first carries
/// the section's top and title, the last its bottom; together they look like one `section`.
pub fn section_part(
    title: Option<(&'static str, Option<SharedString>)>,
    last: bool,
    t: &AppTheme,
) -> Div {
    div()
        .border_x_1()
        .border_b_1()
        .border_color(t.border)
        .overflow_hidden()
        .when_some(title, |d, (title, aside)| {
            d.mt(px(18.))
                .border_t_1()
                .rounded_t(px(6.))
                .child(section_title(title, aside, t))
        })
        .when(last, |d| d.rounded_b(px(6.)))
}

/// A GitHub error as text that can be selected and copied, its links (where to authorize a
/// token for SAML sign-on) opening in the browser.
pub fn error_text(id: impl Into<ElementId>, error: &str, color: Hsla, cx: &App) -> TextView {
    let style =
        TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx)).with_foreground(color);
    TextView::markdown(id, error_markdown(error))
        .style(style)
        .selectable(true)
}

/// Why a GitHub call failed, as the user can act on it: from `github_failure`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubFailure {
    pub kind: FailureKind,
    /// What to tell the user, never core's internal strings.
    pub message: String,
    /// The transport's own words, for Details.
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// No GitHub login found: "Sign in…".
    SignedOut,
    /// GitHub refused the login found (expired or revoked): "Sign in…".
    Expired,
    /// GitHub couldn't be reached: "Retry".
    Offline,
    /// GitHub's own answer (a 404, a rate limit, a 422), in its words.
    Other,
}

/// P4, for every failed GitHub call: `github_failure(&e).toast("Merge failed", retry)` for an
/// action; `load_failed`, `refresh_error` and `list_error` for a page or list call it already.
pub fn github_failure(error: &str) -> GithubFailure {
    let (kind, message, detail) = if is_expired(error) {
        (
            FailureKind::Expired,
            "GitHub rejected your login (expired or revoked). Sign in again.".to_string(),
            None,
        )
    } else if is_not_connected(error) {
        (
            FailureKind::SignedOut,
            "You're signed out of GitHub.".to_string(),
            None,
        )
    } else if let Some(detail) = unreachable_detail(error) {
        (
            FailureKind::Offline,
            gitviber_core::github::UNREACHABLE.to_string(),
            Some(detail.to_string()).filter(|d| !d.is_empty()),
        )
    } else {
        (FailureKind::Other, error.to_string(), None)
    };
    GithubFailure {
        kind,
        message,
        detail,
    }
}

impl GithubFailure {
    /// Signed out or expired: what fixes it is signing in, not trying again.
    pub fn needs_sign_in(&self) -> bool {
        matches!(self.kind, FailureKind::SignedOut | FailureKind::Expired)
    }

    /// An error toast titled `title`, with the button that fixes it: "Sign in…" when signed out
    /// or expired, else `retry` as "Retry" when GitHub couldn't be reached.
    pub fn toast(&self, title: impl Into<SharedString>, retry: Option<ToastRun>) -> Toast {
        let toast = Toast::error(self.message.clone()).title(title);
        let toast = match &self.detail {
            Some(d) => toast.detail(d.clone()),
            None => toast,
        };
        match retry {
            _ if self.needs_sign_in() => toast.action("Sign in…", sign_in),
            Some(retry) if self.kind == FailureKind::Offline => {
                toast.action("Retry", move |window, cx| retry(window, cx))
            }
            _ => toast,
        }
    }
}

/// Shows the PRs panel, whose sign-in screen finds a login, and reads the account again so the
/// screen shows even when the account was read before the login went.
pub fn sign_in(window: &mut Window, cx: &mut App) {
    crate::account::load(cx, true);
    if let Some(ws) = Workspace::for_window(window, cx) {
        ws.update(cx, |ws, cx| {
            ws.show_panel("pulls", cx);
            ws.focus_slot(Slot::Left, window, cx);
        });
    }
}

/// The button that fixes `failure` on a page or list: "Sign in…", else `retry` as "Retry".
fn fix_button(
    id: impl Into<ElementId>,
    failure: &GithubFailure,
    loading: bool,
    retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    let button = Button::new(id).small();
    if failure.needs_sign_in() {
        return button
            .icon(IconName::LogIn)
            .label("Sign in…")
            .on_click(|_, window, cx| sign_in(window, cx));
    }
    button
        .icon(IconName::RefreshCw)
        .label("Retry")
        .loading(loading)
        .disabled(loading)
        .on_click(retry)
}

/// A PR's or an issue's page whose first read failed (offline, GitHub down, signed out): what
/// went wrong, and the button that fixes it.
pub fn load_failed(
    what: &'static str,
    error: &str,
    loading: bool,
    retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    t: &AppTheme,
    cx: &App,
) -> Div {
    let failure = github_failure(error);
    div().size_full().px(px(24.)).pt(px(48.)).child(
        div()
            .mx_auto()
            .max_w(px(560.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .text_center()
            .text_size(rems(0.78))
            .child(
                div()
                    .text_size(rems(0.875))
                    .font_weight(t.semibold())
                    .child(format!("Couldn't load the {what}")),
            )
            .child(error_text(
                "load-error",
                &failure.message,
                t.muted_foreground,
                cx,
            ))
            .when_some(failure.detail.clone(), |d, detail| {
                d.child(div().text_size(rems(0.69)).child(error_text(
                    "load-detail",
                    &detail,
                    t.subtle,
                    cx,
                )))
            })
            .child(
                div()
                    .mt(px(4.))
                    .debug_selector(|| "load-retry".into())
                    .child(fix_button("retry", &failure, loading, retry)),
            ),
    )
}

/// A page's failed refresh, noted under the data it still shows (a rate limit, say); signed
/// out or expired, with "Sign in…".
pub fn refresh_error(error: &str, t: &AppTheme, cx: &App) -> Div {
    let failure = github_failure(error);
    div()
        .mt(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(rems(0.75))
        .debug_selector(|| "refresh-error".into())
        .child(error_text(
            "refresh-error",
            &format!("Couldn't refresh: {}", failure.message),
            t.removed,
            cx,
        ))
        .when(failure.needs_sign_in(), |d| {
            d.child(
                text_button("refresh-sign-in", "Sign in…", t)
                    .on_click(|_, window, cx| sign_in(window, cx)),
            )
        })
}

/// An empty list's words; `roomy`: the whole panel, so it sits lower.
pub fn empty_note(text: impl Into<SharedString>, roomy: bool, t: &AppTheme) -> Div {
    div()
        .px(px(16.))
        .when(roomy, |d| d.pt(px(56.)))
        .when(!roomy, |d| d.py(px(12.)))
        .text_center()
        .text_size(rems(0.75))
        .text_color(t.subtle)
        .child(text.into())
}

/// A failed load, with the button that fixes it. With a list on screen, a note above it, not a
/// blank panel. `id` is a pair, so the text and the button get ids of their own.
pub fn list_error(
    id: (&'static str, usize),
    error: &str,
    loaded: bool,
    loading: bool,
    retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    t: &AppTheme,
    cx: &App,
) -> Div {
    let failure = github_failure(error);
    let text = |color| {
        let detail = failure.detail.as_ref().map(|d| format!("\n{d}"));
        let words = format!("{}{}", failure.message, detail.unwrap_or_default());
        error_text(id, &words, color, cx)
    };
    let button = fix_button(("list-fix", id.1), &failure, loading, retry).xsmall();
    if loaded {
        div()
            .mx(px(8.))
            .mb(px(4.))
            .px(px(8.))
            .py(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(3.))
            .bg(t.removed.opacity(0.1))
            .text_size(rems(0.72))
            .child(div().flex_1().min_w_0().child(text(t.removed)))
            .child(button)
    } else {
        div()
            .px(px(16.))
            .py(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .text_center()
            .text_size(rems(0.75))
            .child(text(t.muted_foreground))
            .child(button)
    }
}

/// Opens a page in the browser.
pub fn open_url(url: &str, cx: &mut App) {
    let (core, url) = (crate::core(cx), url.to_string());
    cx.background_spawn(async move {
        let _ = gitviber_core::api::app::open_url(&core, url);
    })
    .detach();
}

pub fn copy(text: &str, said: &str, window: &mut Window, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    crate::requests::toast(Toast::success(said.to_string()), window, cx);
}

/// A page's markdown (a PR's or issue's body, each comment) as inline markdown views, in the
/// page's scroll. A block whose text didn't change keeps its view, and the images it decoded.
#[derive(Default)]
pub struct Blocks {
    items: Vec<(String, Entity<MarkdownView>)>,
}

impl Blocks {
    /// `repo`: https://github.com/owner/name; `page`: the PR's or issue's, which relative links
    /// resolve against. Returns the blocks whose text changed, for a list to measure again.
    pub fn sync<V: 'static>(
        &mut self,
        texts: &[&str],
        repo: &str,
        page: &str,
        signed: &HashMap<String, String>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Vec<usize> {
        let sign = |id: &str| signed.get(id).cloned();
        let mut changed = vec![];
        for (i, text) in texts.iter().enumerate() {
            let source = github_markdown(text, repo, page, &sign);
            match self.items.get_mut(i) {
                Some((old, _)) if *old == source => {}
                Some((old, view)) => {
                    view.update(cx, |v, cx| v.set_source(&source, cx));
                    *old = source;
                    changed.push(i);
                }
                None => {
                    let base = MarkdownBase {
                        root: None,
                        file: None,
                        callouts: Callouts::GitHub,
                    };
                    let view = cx.new(|cx| MarkdownView::inline(&source, base, window, cx));
                    self.items.push((source, view));
                }
            }
        }
        self.items.truncate(texts.len());
        changed
    }

    /// Which block view `id` is.
    pub fn position(&self, id: gpui_kit::EntityId) -> Option<usize> {
        self.items.iter().position(|(_, v)| v.entity_id() == id)
    }

    /// Block `i`'s view, once synced.
    pub fn get(&self, i: usize) -> Option<Entity<MarkdownView>> {
        self.items.get(i).map(|(_, v)| v.clone())
    }
}

/// Signed links for a private repo's attachments (`github.com/user-attachments/...` needs a
/// github.com login the app doesn't have), asked once per id a page shows: the first link of
/// each is kept for the page's life, so a reload asks nothing and redraws nothing.
#[derive(Default)]
pub struct Attachments {
    pub signed: HashMap<String, String>,
    asked: HashSet<String>,
}

/// The ids of the attachments `text` links to.
pub fn attachment_ids(text: &str) -> impl Iterator<Item = &str> {
    text.split(['(', ')', '"', '\'', ' ', '\n', '<', '>'])
        .filter_map(attachment_id)
}

impl Attachments {
    /// Whether `texts` hold attachments not asked for yet; marks them asked.
    pub fn wants(&mut self, texts: &[&str]) -> bool {
        let mut new = false;
        for id in texts.iter().flat_map(|t| attachment_ids(t)) {
            new |= self.asked.insert(id.to_string());
        }
        new
    }

    /// Whether any link was new.
    pub fn add(&mut self, map: HashMap<String, String>) -> bool {
        let mut changed = false;
        for (id, url) in map {
            if let std::collections::hash_map::Entry::Vacant(e) = self.signed.entry(id) {
                e.insert(url);
                changed = true;
            }
        }
        changed
    }
}

/// A button-like text link in a row ("Show failure", "Details").
pub fn text_button(
    id: impl Into<ElementId>,
    label: impl IntoElement,
    t: &AppTheme,
) -> gpui_kit::Stateful<Div> {
    let hover = t.foreground;
    div()
        .id(id.into())
        .flex_none()
        .cursor_pointer()
        .text_size(rems(0.69))
        .text_color(t.subtle)
        .hover(move |s| s.text_color(hover))
        .child(label)
}

/// The "Signed in as" line under a list.
pub fn signed_in_as(login: &str, source: &str, t: &AppTheme) -> Div {
    div()
        .flex_none()
        .border_t_1()
        .border_color(t.border)
        .px(px(12.))
        .py(px(6.))
        .text_size(rems(0.656))
        .text_color(t.subtle)
        .child(format!(
            "Signed in as {login} via {}",
            if source == "gh" {
                "GitHub CLI"
            } else {
                "git credentials"
            }
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitviber_core::github::{EXPIRED, NOT_CONNECTED, UNREACHABLE};

    #[test]
    fn failures_say_what_fixes_them_never_core_strings() {
        let out = github_failure(NOT_CONNECTED);
        assert_eq!(out.kind, FailureKind::SignedOut);
        assert_eq!(out.message, "You're signed out of GitHub.");
        assert!(out.needs_sign_in());

        let expired = github_failure(EXPIRED);
        assert_eq!(expired.kind, FailureKind::Expired);
        assert!(expired.needs_sign_in());
        assert!(expired.message.contains("expired or revoked"));
        for f in [&out, &expired] {
            assert!(!f.message.contains("github:"), "{}", f.message);
        }

        let offline = github_failure(&format!("{UNREACHABLE}\nio: Network is unreachable"));
        assert_eq!(offline.kind, FailureKind::Offline);
        assert_eq!(
            offline.message,
            "Can't reach GitHub. Check your connection."
        );
        assert_eq!(
            offline.detail.as_deref(),
            Some("io: Network is unreachable")
        );
        assert!(!offline.needs_sign_in());

        let other = github_failure("GitHub 422: Validation Failed");
        assert_eq!(other.kind, FailureKind::Other);
        assert_eq!(other.message, "GitHub 422: Validation Failed");
        assert_eq!(other.detail, None);
    }

    #[test]
    fn attachments_are_asked_once_and_their_first_link_kept() {
        let id = "012a2451-fa01-4fe5-8736-33f4a4d179f1";
        let text = format!("![shot](https://github.com/user-attachments/assets/{id})");
        let mut a = Attachments::default();
        assert!(a.wants(&[&text]));
        assert!(!a.wants(&[&text, "no images"]));
        assert!(a.add(HashMap::from([(
            id.to_string(),
            "https://signed/1".to_string()
        )])));
        // A reload's new signature for the same image changes nothing.
        assert!(!a.add(HashMap::from([(
            id.to_string(),
            "https://signed/2".to_string()
        )])));
        assert_eq!(a.signed[id], "https://signed/1");
    }
}
