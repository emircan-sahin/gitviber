//! A markdown document shown rendered: gpui-component's text view (GFM: tables, task lists,
//! code blocks colored with the syntax theme), our alerts and diagram blocks, frontmatter as a
//! table, links followed the way the old viewer did, and images through a cache the view owns.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::memory::Live;
use gpui_kit::assets::IconName;
use gpui_kit::base::text::{
    InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext,
    MarkdownPlugin, RangeHighlight, RenderedText, TextView, TextViewState, markdown_ast,
};
use gpui_kit::component::text::FrontmatterPlugin;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Bounds, ClipboardItem, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, Hsla, ImageSource, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, RenderImage, Resource, ScrollHandle, SharedString,
    SharedUri, StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div,
    image_cache, point, prelude::FluentBuilder as _, px, rems,
};

use crate::blocks::{self, Callout, Callouts, DiagramKind, SourceBlock};
use crate::diagrams::{Diagrams, Drawn, Look};
use crate::find::{self, FindQuery};
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

/// An image's bytes for its repo-relative path, for a document that isn't the working tree's
/// (a file at a revision), so its pictures come from the same version. Called off the main thread.
pub type ReadImage = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownEvent {
    /// A link to a file in the repo was followed: open it (at `line`).
    OpenFile { path: String, line: Option<u32> },
}

/// The text changed under a find ([`MarkdownView::find`]): its matches were looked for again,
/// `matches` of them now; the current one stays where it can.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindChanged {
    pub matches: usize,
}

/// A find the host asked for: the query, the text it ran on, what it found.
struct Found {
    query: FindQuery,
    /// What `matches` index; the view showing other text runs the query again.
    text: RenderedText,
    matches: Vec<Range<usize>>,
    current: Option<usize>,
}

pub struct MarkdownView {
    state: Entity<TextViewState>,
    base: Arc<MarkdownBase>,
    images: Entity<BoundedImageCache>,
    diagrams: Entity<Diagrams>,
    extensions: MarkdownExtensions,
    /// Sized to its content, inside something else that scrolls (a PR body, a comment).
    inline: bool,
    found: Option<Found>,
    /// The scrolling div an inline view sits in, for revealing a match.
    reveal_in: Option<ScrollHandle>,
    focus: FocusHandle,
    _text: Subscription,
    _live: Live,
}

impl EventEmitter<MarkdownEvent> for MarkdownView {}
impl EventEmitter<FindChanged> for MarkdownView {}

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
        let diagrams = Diagrams::new(cx);
        // A picture landing changes its block's height: the document is laid out again.
        let doc = state.downgrade();
        diagrams.update(cx, |d, _| {
            d.on_ready(move |cx| {
                _ = doc.update(cx, |s, cx| s.invalidate_inline_layout(cx));
            })
        });
        let extensions = extensions(cx.weak_entity(), base.clone(), true);
        let text = cx.observe(&state, |this: &mut Self, _, cx| this.find_again(cx));
        MarkdownView {
            state,
            base,
            images,
            diagrams,
            extensions,
            inline,
            found: None,
            reveal_in: None,
            focus: cx.focus_handle(),
            _text: text,
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

    /// Relative images come from `read` instead of from disk (None: from disk again); the
    /// base's root and file still say which path an image is. Images already shown are dropped.
    pub fn read_images(
        &mut self,
        read: Option<ReadImage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The cache sees paths under the root (MarkdownBase::image); the host wants them relative.
        let read = read.zip(self.base.root.clone()).map(|(read, root)| {
            Arc::new(move |path: &std::path::Path| {
                let path = path.strip_prefix(&root).map_err(|e| e.to_string())?;
                read(&path.to_string_lossy())
            }) as crate::images::ReadBytes
        });
        self.images
            .update(cx, |images, cx| images.set_read(read, window, cx));
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn images(&self) -> &Entity<BoundedImageCache> {
        &self.images
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

    /// Find in the text the view shows (not its markdown source): every match of `query` is
    /// washed in the theme's find color and their count comes back (at most
    /// [`find::MAX_MATCHES`]), or why the query can't run. An empty query clears the marks.
    /// None is current until [`Self::select_match`]. Text that changes later is searched again,
    /// with [`FindChanged`] emitted.
    pub fn find(&mut self, query: &FindQuery, cx: &mut Context<Self>) -> Result<usize, String> {
        if query.text.is_empty() {
            if self.found.take().is_some() {
                self.state.update(cx, |s, cx| s.clear_range_highlights(cx));
            }
            return Ok(0);
        }
        let text = self.state.read(cx).rendered_text();
        if let Some(f) = &self.found
            && f.query == *query
            && f.text == text
        {
            return Ok(f.matches.len());
        }
        let matches = match find::find_matches(text.as_str(), query, find::MAX_MATCHES) {
            Ok(m) => m,
            Err(e) => {
                _ = self.find(&FindQuery::default(), cx);
                return Err(e);
            }
        };
        let n = matches.len();
        self.found = Some(Found {
            query: query.clone(),
            text,
            matches,
            current: None,
        });
        self.paint_matches(cx);
        Ok(n)
    }

    /// Makes match `ix` of the find the current one, or none in this view (`None`: for a host
    /// whose matches run across several views). `reveal` scrolls to it: a view that scrolls does
    /// so itself; an inline one scrolls the `gpui::list` around it once its row is laid out
    /// (scroll the list to that row first), or the div given to [`Self::reveal_in`].
    pub fn select_match(&mut self, ix: Option<usize>, reveal: bool, cx: &mut Context<Self>) {
        let Some(f) = &mut self.found else { return };
        let ix = ix.filter(|i| *i < f.matches.len());
        let range = ix.map(|i| f.matches[i].clone());
        if f.current != ix {
            f.current = ix;
            self.paint_matches(cx);
        }
        if let Some(range) = range.filter(|_| reveal) {
            self.state.update(cx, |s, cx| {
                _ = s.reveal_range(range, cx);
            });
        }
    }

    /// The next match (`dir` 1) or the one before (-1), round, revealed; the first or the last
    /// when none is current. The current one, if any.
    pub fn step_match(&mut self, dir: isize, cx: &mut Context<Self>) -> Option<usize> {
        let (current, n) = self.matches();
        if n == 0 {
            return None;
        }
        let next = match current {
            Some(i) => (i as isize + dir).rem_euclid(n as isize) as usize,
            None if dir < 0 => n - 1,
            None => 0,
        };
        self.select_match(Some(next), true, cx);
        Some(next)
    }

    /// The find's current match and how many there are.
    pub fn matches(&self) -> (Option<usize>, usize) {
        self.found
            .as_ref()
            .map_or((None, 0), |f| (f.current, f.matches.len()))
    }

    /// For an inline view in a div that scrolls (`overflow_y_scroll` + `track_scroll(handle)`):
    /// revealing a match scrolls that div.
    pub fn reveal_in(&mut self, handle: ScrollHandle) {
        self.reveal_in = Some(handle);
    }

    /// The view shows other text than a find ran on: run it again.
    fn find_again(&mut self, cx: &mut Context<Self>) {
        let Some(f) = &self.found else { return };
        let text = self.state.read(cx).rendered_text();
        if f.text == text {
            return;
        }
        let matches =
            find::find_matches(text.as_str(), &f.query, find::MAX_MATCHES).unwrap_or_default();
        let n = matches.len();
        let current = f
            .current
            .and_then(|i| n.checked_sub(1).map(|last| i.min(last)));
        self.found = Some(Found {
            query: f.query.clone(),
            text,
            matches,
            current,
        });
        self.paint_matches(cx);
        cx.emit(FindChanged { matches: n });
    }

    fn paint_matches(&mut self, cx: &mut Context<Self>) {
        let Some(f) = &self.found else { return };
        let t = cx.app_theme();
        let (wash, current) = (t.find_match, t.find_current);
        let marks: Vec<RangeHighlight> = f
            .matches
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let color = if Some(i) == f.current { current } else { wash };
                RangeHighlight::new(r.clone(), color)
            })
            .collect();
        self.state.update(cx, |s, cx| {
            _ = s.set_range_highlights(marks, cx);
        });
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

/// How pictures are drawn now: the theme's ink, the display's density.
fn look(font_size: gpui_kit::Pixels, window: &Window, cx: &App) -> Look {
    let t = cx.app_theme();
    Look {
        dark: t.dark,
        color: t.foreground,
        font_size: f32::from(font_size),
        scale: window.scale_factor(),
    }
}

/// The view's picture of `source`, starting it the first time.
fn drawn(
    view: &WeakEntity<MarkdownView>,
    kind: DiagramKind,
    source: &str,
    inline: bool,
    look: Look,
    cx: &mut App,
) -> Option<Drawn> {
    let diagrams = view.upgrade()?.read(cx).diagrams.clone();
    diagrams.update(cx, |d, cx| d.get(kind, source, inline, look, cx))
}

/// A drawn diagram or formula: its own size, centered; a wide one scrolls sideways.
fn picture(
    image: Arc<RenderImage>,
    scale: f32,
    kind: DiagramKind,
    cx: &App,
) -> gpui_kit::AnyElement {
    let t = cx.app_theme();
    let size = image.size(0);
    let (w, h) = (
        i32::from(size.width) as f32 / scale,
        i32::from(size.height) as f32 / scale,
    );
    div()
        .id(ElementId::Name(
            format!("gv-picture-{:p}", Arc::as_ptr(&image)).into(),
        ))
        .my_2()
        .w_full()
        .overflow_x_scroll()
        .flex()
        .justify_center()
        .when(kind == DiagramKind::Mermaid, |d| {
            d.p_3().rounded(px(6.)).border_1().border_color(t.border)
        })
        .child(
            gpui_kit::img(ImageSource::Render(image))
                .w(px(w))
                .h(px(h))
                .flex_none(),
        )
        .into_any_element()
}

/// A diagram or formula as its source under a label: while it's drawn, or when it can't be.
fn source_block(block: &SourceBlock, failed: bool, cx: &App) -> gpui_kit::AnyElement {
    let t = cx.app_theme();
    let label = if failed {
        format!("{} (couldn't be drawn)", block.kind.label())
    } else {
        block.kind.label().to_string()
    };
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
                .child(label),
        )
        .child(
            div()
                .font_family(t.fonts.code.clone())
                .text_size(t.fonts.code_size)
                .text_color(t.foreground)
                .child(block.source.trim_end().to_string()),
        )
        .into_any_element()
}

/// `$…$` in a paragraph, drawn in the line; its source shows until then, or if it can't be drawn.
struct InlineMath {
    view: WeakEntity<MarkdownView>,
}

impl MarkdownPlugin for InlineMath {
    fn name(&self) -> &str {
        "gv-math"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::InlineMath(math) = node else {
            return None;
        };
        let end = node.position()?.end.offset.checked_sub(cx.offset())?;
        if !blocks::is_inline_math(&math.value, cx.source().get(end..).unwrap_or("")) {
            return None;
        }
        Some(MarkdownNode::new("gv-math", math.value.clone()).text(math.value.clone()))
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let source = node.data::<String>()?;
        let look = look(context.font_size(), window, cx);
        let Some(Drawn::Ready { image, scale }) =
            drawn(&self.view, DiagramKind::Math, source, true, look, cx)
        else {
            return None;
        };
        let size = image.size(0);
        let (w, h) = (
            i32::from(size.width) as f32 / scale,
            i32::from(size.height) as f32 / scale,
        );
        Some(InlineElement::new(
            gpui_kit::img(ImageSource::Render(image)).w(px(w)).h(px(h)),
        ))
    }
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
        .block_renderer("gv-source", {
            let view = view.clone();
            move |node, window, cx| {
                let Some(block) = node.data::<SourceBlock>() else {
                    return div().into_any_element();
                };
                let look = look(window.rem_size(), window, cx);
                match drawn(&view, block.kind, &block.source, false, look, cx) {
                    Some(Drawn::Ready { image, scale }) => picture(image, scale, block.kind, cx),
                    drawn => source_block(block, matches!(drawn, Some(Drawn::Failed(_))), cx),
                }
            }
        })
        .plugin(InlineMath { view: view.clone() })
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
        .selectable(true)
        .when_some(
            self.reveal_in.clone().filter(|_| self.inline),
            |v, handle| v.on_reveal(move |line, window, _| scroll_to_line(&handle, line, window)),
        );
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

/// Brings `line` (window coordinates) into `handle`'s view a third of the way down, only when
/// it's off screen.
fn scroll_to_line(handle: &ScrollHandle, line: Bounds<Pixels>, window: &mut Window) {
    let view = handle.bounds();
    if line.top() >= view.top() && line.bottom() <= view.bottom() {
        return;
    }
    let offset = handle.offset();
    let y = offset.y + view.top() + view.size.height / 3. - line.top();
    let max = handle.max_offset().y;
    handle.set_offset(point(offset.x, y.clamp(-max, px(0.))));
    window.refresh();
}

/// Whether `path` is a markdown file by its name.
pub fn is_markdown(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".md", ".markdown", ".mdown", ".mkd"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}
