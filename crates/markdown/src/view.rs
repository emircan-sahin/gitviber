//! A markdown document shown rendered: gpui-component's text view (GFM: tables, task lists,
//! code blocks colored with the syntax theme), our alerts and diagram blocks, frontmatter as a
//! table, links followed the way the old viewer did, and images through a cache the view owns.

use std::path::PathBuf;
use std::sync::Arc;

use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::memory::Live;
use gpui_kit::assets::IconName;
use gpui_kit::base::text::{MarkdownExtensions, MarkdownNode, TextView, TextViewState};
use gpui_kit::component::text::FrontmatterPlugin;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, ElementId, Entity, EventEmitter, FocusHandle,
    Focusable, Hsla, ImageSource, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    Resource, SharedString, SharedUri, Styled as _, WeakEntity, Window, div, image_cache,
    prelude::FluentBuilder as _, px, rems,
};

use crate::blocks::{self, Callout, Callouts, SourceBlock};
use crate::images::BoundedImageCache;
use crate::links::{self, Link};

/// Where a document is, for its links and images.
#[derive(Clone, Debug, Default)]
pub struct MarkdownBase {
    /// The repo's root on disk; images load from under it. None: images with relative paths don't show.
    pub root: Option<PathBuf>,
    /// The document's repo-relative path; relative links resolve against its folder.
    pub file: Option<String>,
    pub callouts: Callouts,
}

impl MarkdownBase {
    pub fn file(root: impl Into<PathBuf>, file: impl Into<String>) -> Self {
        Self {
            root: Some(root.into()),
            file: Some(file.into()),
            callouts: Callouts::GitHub,
        }
    }

    /// An image's source: a relative path inside the repo from disk, web and data URLs as they are.
    fn image(&self, uri: &SharedUri) -> ImageSource {
        let href = uri.as_ref();
        let lower = href.to_ascii_lowercase();
        if lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("data:")
        {
            return ImageSource::from(uri.clone());
        }
        let path = match (&self.root, links::classify(self.file.as_deref(), href)) {
            (Some(root), Link::File { path, .. }) => root.join(path),
            // Nothing outside the repo is read for a document in it.
            _ => PathBuf::new(),
        };
        ImageSource::Resource(Resource::Path(Arc::from(path.as_path())))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownEvent {
    /// A link to a file in the repo was followed: open it (at `line`).
    OpenFile { path: String, line: Option<u32> },
}

pub struct MarkdownView {
    state: Entity<TextViewState>,
    base: Arc<MarkdownBase>,
    images: Entity<BoundedImageCache>,
    extensions: MarkdownExtensions,
    /// Sized to its content, inside something else that scrolls (a PR body, a comment).
    inline: bool,
    focus: FocusHandle,
    _live: Live,
}

impl EventEmitter<MarkdownEvent> for MarkdownView {}

impl MarkdownView {
    /// A document filling its place: it scrolls, on the code background, centered.
    pub fn new(
        source: &str,
        base: MarkdownBase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(source, base, false, window, cx)
    }

    /// A document as tall as its content, the width its parent gives it, with no background or
    /// padding of its own: for text inside a page that scrolls (a PR's body, a comment).
    pub fn inline(
        source: &str,
        base: MarkdownBase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(source, base, true, window, cx)
    }

    fn build(
        source: &str,
        base: MarkdownBase,
        inline: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let base = Arc::new(base);
        let state = cx.new(|cx| TextViewState::markdown(source, cx));
        let images = BoundedImageCache::new(BoundedImageCache::display_side(window), cx);
        let extensions = extensions(cx.weak_entity(), base.clone(), true);
        MarkdownView {
            state,
            base,
            images,
            extensions,
            inline,
            focus: cx.focus_handle(),
            _live: Live::new("MarkdownView"),
        }
    }

    /// Shows other text (the file changed on disk); the scroll position stays where it can.
    pub fn set_source(&mut self, source: &str, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.set_text(source, cx));
    }

    pub fn base(&self) -> &MarkdownBase {
        &self.base
    }

    /// Follows a link the way the old viewer did: anchors scroll, web links open in the browser,
    /// other schemes are copied, files in the repo open as tabs (`MarkdownEvent::OpenFile`).
    pub fn follow_link(&mut self, href: &str, window: &mut Window, cx: &mut Context<Self>) {
        match links::classify(self.base.file.as_deref(), href) {
            Link::Anchor(anchor) => {
                self.scroll_to_anchor(&anchor, cx);
            }
            Link::Web(url) => cx.open_url(&url),
            Link::Other(url) | Link::Outside(url) => {
                cx.write_to_clipboard(ClipboardItem::new_string(url));
                gitviber_ui::workspace::Toast::info("Link copied: it can't be opened from here.")
                    .show(window, cx);
            }
            Link::File { path, line } => cx.emit(MarkdownEvent::OpenFile { path, line }),
        }
    }

    /// Scrolls to the heading `anchor` names (GitHub's slug); false if there's none.
    pub fn scroll_to_anchor(&mut self, anchor: &str, cx: &mut Context<Self>) -> bool {
        self.state.update(cx, |state, cx| {
            let rendered = state.rendered_text();
            let Some(range) = links::heading_range(rendered.source(), anchor) else {
                return false;
            };
            let Some(range) = rendered.range_for_source(range) else {
                return false;
            };
            state.reveal_range(range, cx).is_ok()
        })
    }
}

impl Focusable for MarkdownView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The text view as this view configures it, for this document's links and images.
fn text_view(
    id: ElementId,
    source: Option<SharedString>,
    state: Option<&Entity<TextViewState>>,
    view: &WeakEntity<MarkdownView>,
    base: &Arc<MarkdownBase>,
    extensions: MarkdownExtensions,
) -> TextView {
    let view = view.clone();
    let images = base.clone();
    let tv = match (state, source) {
        (Some(state), _) => TextView::new(state),
        (None, source) => TextView::markdown(id, source.unwrap_or_default()),
    };
    tv.markdown_extensions(extensions)
        .image_source(move |uri| images.image(uri))
        .on_link_click(move |url, _, window, cx| {
            let url = url.clone();
            _ = view.update(cx, |view, cx| view.follow_link(&url, window, cx));
        })
}

fn kind_color(kind: &str, dark: bool) -> Hsla {
    let rgb = |d: u32, l: u32| gpui_kit::rgb(if dark { d } else { l }).into();
    match kind {
        "gh-note" => rgb(0x4493f8, 0x0969da),
        "gh-tip" => rgb(0x3fb950, 0x1a7f37),
        "gh-important" => rgb(0xab7df8, 0x8250df),
        "gh-warning" => rgb(0xd29922, 0x9a6700),
        "gh-caution" => rgb(0xf85149, 0xd1242f),
        "abstract" | "tip" => rgb(0x53dfdd, 0x00bfbc),
        "success" => rgb(0x44cf6e, 0x08b94e),
        "question" | "warning" => rgb(0xe9973f, 0xec7500),
        "failure" | "danger" | "bug" => rgb(0xfb464c, 0xe93147),
        "example" => rgb(0xa882ff, 0x7852ee),
        "quote" => rgb(0x9e9e9e, 0x9e9e9e),
        _ => rgb(0x027aff, 0x086ddd),
    }
}

fn kind_icon(kind: &str) -> IconName {
    match kind {
        "gh-note" | "info" => IconName::Info,
        "gh-tip" => IconName::Lightbulb,
        "gh-important" => IconName::MessageSquareWarning,
        "gh-warning" | "warning" => IconName::TriangleAlert,
        "gh-caution" => IconName::OctagonAlert,
        "abstract" => IconName::ClipboardList,
        "todo" => IconName::CircleCheck,
        "tip" => IconName::Flame,
        "success" => IconName::Check,
        "question" => IconName::CircleQuestionMark,
        "failure" => IconName::X,
        "danger" => IconName::Zap,
        "bug" => IconName::Bug,
        "example" => IconName::List,
        "quote" => IconName::Quote,
        _ => IconName::Pencil,
    }
}

/// The parsers and renderers for our blocks. `top`: the document itself; a callout's body gets
/// the same minus frontmatter.
fn extensions(
    view: WeakEntity<MarkdownView>,
    base: Arc<MarkdownBase>,
    top: bool,
) -> MarkdownExtensions {
    let flavor = base.callouts;
    let mut ext = MarkdownExtensions::default().parser_revision(1);
    if top {
        ext = ext.frontmatter().plugin(FrontmatterPlugin::new());
    }
    ext = ext
        .block_parser(|node, _| {
            let block = blocks::parse_source_block(node)?;
            let text = block.source.clone();
            Some(MarkdownNode::new("gv-source", block).text(text))
        })
        .block_renderer("gv-source", |node, window, cx| {
            let Some(block) = node.data::<SourceBlock>() else {
                return div().into_any_element();
            };
            if let Some(drawn) =
                blocks::diagram_renderer(cx).and_then(|r| r(block.kind, &block.source, window, cx))
            {
                return drawn;
            }
            let t = cx.app_theme();
            div()
                .my_2()
                .p_3()
                .rounded(px(6.))
                .border_1()
                .border_color(t.border)
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(rems(0.6875))
                        .text_color(t.subtle)
                        .child(block.kind.label()),
                )
                .child(
                    div()
                        .font_family(t.fonts.code.clone())
                        .text_size(t.fonts.code_size)
                        .text_color(t.foreground)
                        .child(block.source.trim_end().to_string()),
                )
                .into_any_element()
        })
        .block_parser(move |node, cx| {
            let gpui_kit::component::text::markdown_ast::Node::Blockquote(_) = node else {
                return None;
            };
            let callout = blocks::parse_callout(cx.node_source(node)?, flavor)?;
            let text = format!("{}\n{}", callout.title, callout.body);
            Some(MarkdownNode::new("gv-callout", callout).text(text))
        });
    let nested = (top).then(|| extensions(view.clone(), base.clone(), false));
    ext.block_renderer("gv-callout", move |node, _window, cx| {
        let Some(callout) = node.data::<Callout>() else {
            return div().into_any_element();
        };
        let t = cx.app_theme();
        let color = kind_color(&callout.kind, t.dark);
        let github = callout.kind.starts_with("gh-");
        let id = ElementId::Name(
            format!("gv-callout-{}", node.source_range().map_or(0, |r| r.start)).into(),
        );
        let body = (!callout.body.trim().is_empty()).then(|| {
            text_view(
                id,
                Some(callout.body.clone().into()),
                None,
                &view,
                &base,
                nested.clone().unwrap_or_default(),
            )
        });
        div()
            .my_2()
            .when(github, |d| {
                d.pl_4().py_1().border_l(px(4.)).border_color(color)
            })
            .when(!github, |d| {
                d.px(px(14.))
                    .py(px(10.))
                    .rounded(px(6.))
                    .border_l(px(3.))
                    .border_color(color.opacity(0.6))
                    .bg(color.opacity(0.1))
            })
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(color)
                    .font_weight(if github {
                        gpui_kit::FontWeight::MEDIUM
                    } else {
                        gpui_kit::FontWeight::SEMIBOLD
                    })
                    .child(Icon::new(kind_icon(&callout.kind)).small())
                    .child(callout.title.clone()),
            )
            .children(body)
            .into_any_element()
    })
}

impl Render for MarkdownView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let view = text_view(
            ElementId::Name("markdown".into()),
            None,
            Some(&self.state),
            &cx.weak_entity(),
            &self.base,
            self.extensions.clone(),
        )
        .scrollable(!self.inline)
        .selectable(true);
        if self.inline {
            return div()
                .id("markdown-inline")
                .key_context("MarkdownView")
                .track_focus(&self.focus)
                .w_full()
                .child(image_cache(self.images.clone()).w_full().child(view));
        }
        div()
            .id("markdown-view")
            .key_context("MarkdownView")
            .track_focus(&self.focus)
            .size_full()
            .bg(t.code(t.background))
            .flex()
            .justify_center()
            .child(
                image_cache(self.images.clone())
                    .size_full()
                    .max_w(px(860. + 64.))
                    .px_8()
                    .pt_4()
                    .child(view),
            )
    }
}

/// Whether `path` is a markdown file by its name.
pub fn is_markdown(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".md", ".markdown", ".mdown", ".mkd"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}
