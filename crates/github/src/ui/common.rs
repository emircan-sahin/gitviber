//! Pieces the GitHub views share: state icons and pills, CI and label chips, sections, notes, a
//! page's markdown blocks and their attachments, and opening or copying a link.

use std::collections::{HashMap, HashSet};

use gitviber_markdown::{Callouts, MarkdownBase, MarkdownView};
use gitviber_ui::widgets::{now_unix, relative_time};
use gitviber_ui::workspace::Toast;
use gitviber_ui::{ActiveAppTheme as _, AppTheme};
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Div, ElementId, Entity, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
    rems,
};

use crate::logic::text::{attachment_id, github_markdown};
use crate::model::{CiState, IssueItem, LabelItem, PullState, unix};

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
) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(3.))
        .bg(bg)
        .text_color(fg)
        .text_size(rems(0.656))
        .font_weight(FontWeight::SEMIBOLD)
        .when_some(border, |d, b| d.border_1().border_color(b))
        .child(label)
}

pub fn pull_pill(state: PullState, draft: bool, t: &AppTheme) -> Div {
    match state {
        PullState::Merged => pill("Merged", t.renamed_fill, t.on_status, None),
        PullState::Closed => pill("Closed", t.removed_fill, t.on_status, None),
        PullState::Open if draft => pill(
            "Draft",
            t.elevated,
            t.muted_foreground,
            Some(t.border_strong),
        ),
        PullState::Open => pill("Open", t.added_fill, t.on_status, None),
    }
}

pub fn issue_pill(i: &IssueItem, t: &AppTheme) -> Div {
    if i.open {
        pill("Open", t.added_fill, t.on_status, None)
    } else if i.not_planned() {
        pill(
            "Closed as not planned",
            t.elevated,
            t.muted_foreground,
            Some(t.border_strong),
        )
    } else {
        pill("Closed", t.renamed_fill, t.on_status, None)
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

/// A label as a small chip.
pub fn label_chip(label: &LabelItem, t: &AppTheme) -> Div {
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
        .child(div().truncate().child(label.name.clone()))
}

/// A boxed part of a PR or issue page with its uppercase title.
pub fn section(title: &'static str, aside: Option<SharedString>, t: &AppTheme) -> Div {
    div()
        .mt(px(18.))
        .rounded(px(6.))
        .border_1()
        .border_color(t.border)
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .h(px(30.))
                .px(px(12.))
                .bg(t.panel)
                .border_b_1()
                .border_color(t.border)
                .text_size(rems(0.656))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.subtle)
                .child(title.to_uppercase())
                .when_some(aside, |d, a| {
                    d.child(div().ml_auto().font_family(t.fonts.code.clone()).child(a))
                }),
        )
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

/// A failed load. With a list on screen, a note above it, not a blank panel.
pub fn list_error(error: &str, loaded: bool, t: &AppTheme) -> Div {
    if loaded {
        div()
            .mx(px(8.))
            .mb(px(4.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(3.))
            .bg(t.removed.opacity(0.1))
            .text_size(rems(0.72))
            .text_color(t.removed)
            .child(error.to_string())
    } else {
        div()
            .px(px(16.))
            .py(px(24.))
            .text_center()
            .text_size(rems(0.75))
            .text_color(t.muted_foreground)
            .child(error.to_string())
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
    /// resolve against.
    pub fn sync<V: 'static>(
        &mut self,
        texts: &[&str],
        repo: &str,
        page: &str,
        signed: &HashMap<String, String>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        let sign = |id: &str| signed.get(id).cloned();
        for (i, text) in texts.iter().enumerate() {
            let source = github_markdown(text, repo, page, &sign);
            match self.items.get_mut(i) {
                Some((old, _)) if *old == source => {}
                Some((old, view)) => {
                    view.update(cx, |v, cx| v.set_source(&source, cx));
                    *old = source;
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
    }

    /// Block `i`'s view, once synced.
    pub fn get(&self, i: usize) -> Option<Entity<MarkdownView>> {
        self.items.get(i).map(|(_, v)| v.clone())
    }

    /// Every block's view, in the page's order.
    pub fn views(&self) -> impl Iterator<Item = Entity<MarkdownView>> + '_ {
        self.items.iter().map(|(_, v)| v.clone())
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

impl Attachments {
    /// Whether `texts` hold attachments not asked for yet; marks them asked.
    pub fn wants(&mut self, texts: &[&str]) -> bool {
        let mut new = false;
        for text in texts {
            for word in text.split(['(', ')', '"', '\'', ' ', '\n', '<', '>']) {
                if let Some(id) = attachment_id(word)
                    && self.asked.insert(id.to_string())
                {
                    new = true;
                }
            }
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
    label: impl Into<SharedString>,
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
        .child(label.into())
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
