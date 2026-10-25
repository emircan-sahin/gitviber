//! A markdown document shown rendered: gpui-component's text view (GFM: tables, task lists,
//! code blocks colored with the syntax theme), our alerts and diagram blocks, `<details>` that
//! open and shut, frontmatter as properties, links followed the way the old viewer did, and
//! images through a cache the view owns.

use std::collections::HashSet;
use std::hash::{Hash as _, Hasher as _};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::scroll::{self, Step};
use gitviber_ui::memory::Live;
use gpui_kit::assets::IconName;
use gpui_kit::base::text::{
    InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext,
    MarkdownPlugin, RangeHighlight, RenderedText, TextView, TextViewState, markdown_ast,
};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Bounds, ClipboardItem, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, Hsla, ImageSource, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, RenderImage, Resource, ScrollHandle, SharedString,
    SharedUri, StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div,
    image_cache, point, prelude::FluentBuilder as _, px, rems,
};

use crate::blocks::{self, Callout, Callouts, Details, DiagramKind, SourceBlock};
use crate::diagrams::{Diagrams, Drawn, Look};
use crate::find::{self, FindQuery};
use crate::images::BoundedImageCache;
use crate::links::{self, Link};
use crate::properties::{self, Value};

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
            callouts: Callouts::Repo,
        }
    }

    /// An image's source: a relative path inside the repo from disk, web and data URLs as they
    /// are. A pull request's text loads only from GitHub's own hosts, whatever its markdown says.
    fn image(&self, uri: &SharedUri) -> ImageSource {
        let href = uri.as_ref();
        if self.callouts == Callouts::GitHub && !links::is_github_hosted(href) {
            return ImageSource::Resource(Resource::Path(Arc::from(std::path::Path::new(""))));
        }
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
    /// A source range to scroll to once the text it's in is parsed.
    reveal: Option<Range<usize>>,
    /// Details and folding callouts opened or shut away from how they start, by `toggle_key`.
    toggled: HashSet<u64>,
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
        let text = cx.observe(&state, |this: &mut Self, _, cx| {
            this.find_again(cx);
            this.reveal_pending(cx);
        });
        MarkdownView {
            state,
            base,
            images,
            diagrams,
            extensions,
            inline,
            found: None,
            reveal_in: None,
            reveal: None,
            toggled: HashSet::new(),
            focus: cx.focus_handle(),
            _text: text,
            _live: Live::new("MarkdownView"),
        }
    }

    /// Shows other text (the file changed on disk); the scroll position stays where it can.
    pub fn set_source(&mut self, source: &str, cx: &mut Context<Self>) {
        self.reveal = None;
        self.toggled.clear();
        self.state
            .update(cx, |state, cx| state.set_text(source, cx));
    }

    /// Scrolls to where `range` of the markdown renders (an inline view: the `gpui::list` it's
    /// in, once its row is laid out), now or once the text is parsed.
    pub fn reveal_source(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.reveal = Some(range);
        self.reveal_pending(cx);
    }

    fn reveal_pending(&mut self, cx: &mut Context<Self>) {
        let Some(range) = self.reveal.clone() else {
            return;
        };
        let done = self.state.update(cx, |state, cx| {
            let rendered = state.rendered_text();
            match rendered.range_for_source(range) {
                Some(r) => {
                    _ = state.reveal_range(r, cx);
                    true
                }
                // Parsed and still not there: it renders no text.
                None => !rendered.source().is_empty(),
            }
        });
        if done {
            self.reveal = None;
        }
    }

    /// Whether the details or callout `key` shows its body, `open` being how it starts.
    fn is_open(&self, key: u64, open: bool) -> bool {
        open != self.toggled.contains(&key)
    }

    fn toggle(&mut self, key: u64, cx: &mut Context<Self>) {
        if !self.toggled.remove(&key) {
            self.toggled.insert(key);
        }
        // A scrolling view measured the block's height.
        self.state
            .update(cx, |s, cx| s.invalidate_inline_layout(cx));
        cx.notify();
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

    /// A scroll key (bound for "MarkdownView" in the app's keymap) moves the document. An
    /// inline view doesn't take them: they go on to the page it's in.
    fn scroll_key(&mut self, step: Step, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            if scroll::step_list(state.list_state(), step) {
                cx.notify();
            }
        });
    }

    /// How far a view that scrolls itself is scrolled (tests).
    #[doc(hidden)]
    pub fn scroll_top(&self, cx: &App) -> Pixels {
        -self
            .state
            .read(cx)
            .list_state()
            .scroll_px_offset_for_scrollbar()
            .y
    }

    /// Scrolls to the footnote or the heading `anchor` names (GitHub's anchors); false if
    /// there's none.
    pub fn scroll_to_anchor(&mut self, anchor: &str, cx: &mut Context<Self>) -> bool {
        self.state.update(cx, |state, cx| {
            let rendered = state.rendered_text();
            let source = rendered.source();
            let Some(range) = links::footnote_range(source, anchor)
                .or_else(|| links::heading_range(source, anchor))
            else {
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

/// A details' or a callout's key in `MarkdownView::toggled`: where it starts and what it says,
/// so one in a nested view (its offsets its own) doesn't take another's.
fn toggle_key(at: usize, a: &str, b: &str) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    (at, a, b).hash(&mut h);
    h.finish()
}

fn is_open(view: &WeakEntity<MarkdownView>, key: u64, open: bool, cx: &App) -> bool {
    view.upgrade()
        .map_or(open, |v| v.read(cx).is_open(key, open))
}

/// A line that opens and shuts what's under it, its chevron in front.
fn toggle_row(
    id: ElementId,
    view: &WeakEntity<MarkdownView>,
    key: u64,
    open: bool,
    cx: &App,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let view = view.clone();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            _ = view.update(cx, |v, cx| v.toggle(key, cx));
        })
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .small()
            .text_color(cx.app_theme().muted_foreground),
        )
}

/// Whether a callout's title is written in markdown, drawn as such; a plain one keeps the
/// callout's color and weight.
fn has_markup(title: &str) -> bool {
    title.contains(['*', '_', '`', '[', '<', '~', '=', '$', '\\'])
}

/// The parsers and renderers for our blocks. `top`: the document itself; a callout's or a
/// details' parts get the same minus frontmatter.
fn extensions(
    view: WeakEntity<MarkdownView>,
    base: Arc<MarkdownBase>,
    top: bool,
) -> MarkdownExtensions {
    let flavor = base.callouts;
    let mut ext = MarkdownExtensions::default().parser_revision(1);
    // GitHub reads no frontmatter in a pull request's text: its `---` is a rule.
    if top && flavor != Callouts::GitHub {
        ext = ext
            .frontmatter()
            .block_parser(|node, cx| {
                let markdown_ast::Node::Yaml(_) = node else {
                    return None;
                };
                let props = properties::parse(cx.node_source(node)?);
                let text = props
                    .iter()
                    .map(|(key, value)| format!("{key}: {}", value.text()))
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(MarkdownNode::new("gv-properties", props).text(text))
            })
            .block_renderer("gv-properties", |node, _, cx| {
                match node.data::<Vec<(String, Value)>>() {
                    Some(props) if !props.is_empty() => {
                        properties::element(props, cx).into_any_element()
                    }
                    _ => div().into_any_element(),
                }
            });
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
        })
        .block_parser(|node, _| {
            let markdown_ast::Node::Html(html) = node else {
                return None;
            };
            let details = blocks::parse_details(&html.value)?;
            let text = format!(
                "{}\n{}",
                blocks::html_text(&details.summary),
                blocks::html_text(&details.body)
            );
            Some(MarkdownNode::new("gv-details", details).text(text))
        });
    let nested = (top).then(|| extensions(view.clone(), base.clone(), false));
    let nested_view = {
        let (view, base, nested) = (view.clone(), base.clone(), nested.clone());
        move |id: String, source: &str| {
            text_view(
                ElementId::Name(id.into()),
                Some(source.to_string().into()),
                None,
                &view,
                &base,
                nested.clone().unwrap_or_default(),
            )
        }
    };
    ext = ext.block_renderer("gv-details", {
        let (view, nested_view) = (view.clone(), nested_view.clone());
        move |node, _window, cx| {
            let Some(d) = node.data::<Details>() else {
                return div().into_any_element();
            };
            let at = node.source_range().map_or(0, |r| r.start);
            let key = toggle_key(at, &d.summary, &d.body);
            let open = is_open(&view, key, d.open, cx);
            let part = |name: &str, source: &str| {
                (!source.trim().is_empty())
                    .then(|| nested_view(format!("gv-details-{at}-{name}"), source))
            };
            div()
                .flex()
                .flex_col()
                .children(part("before", &d.before))
                .child(
                    toggle_row(
                        ElementId::Name(format!("gv-details-{at}").into()),
                        &view,
                        key,
                        open,
                        cx,
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .children(part("summary", &d.summary)),
                    ),
                )
                .children(part("body", &d.body).filter(|_| open))
                .children(part("after", &d.after))
                .into_any_element()
        }
    });
    ext.block_renderer("gv-callout", move |node, _window, cx| {
        let Some(callout) = node.data::<Callout>() else {
            return div().into_any_element();
        };
        let t = cx.app_theme();
        let color = kind_color(&callout.kind, t.dark);
        let github = callout.kind.starts_with("gh-");
        let at = node.source_range().map_or(0, |r| r.start);
        // `-` starts shut, `+` open; without either it doesn't fold.
        let key = toggle_key(at, &callout.title, &callout.body);
        let folds = !callout.fold.is_empty();
        let open = !folds || is_open(&view, key, callout.fold == "+", cx);
        let body = (open && !callout.body.trim().is_empty())
            .then(|| nested_view(format!("gv-callout-{at}"), &callout.body));
        let title = if !github && has_markup(&callout.title) {
            nested_view(format!("gv-callout-{at}-title"), &callout.title).into_any_element()
        } else {
            callout.title.clone().into_any_element()
        };
        let header = div()
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
            .child(div().flex_1().min_w_0().child(title));
        let header = if folds {
            toggle_row(
                ElementId::Name(format!("gv-callout-{at}-fold").into()),
                &view,
                key,
                open,
                cx,
            )
            .child(header)
            .into_any_element()
        } else {
            header.into_any_element()
        };
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
            .child(header)
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
            .map(|el| scroll::on_keys(el, Self::scroll_key, cx))
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

#[cfg(test)]
mod tests {
    use gpui_kit::{AnyView, TestAppContext, VisualTestContext};

    use super::*;
    use crate::FindBar;

    struct Host(Option<AnyView>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(self.0.clone())
        }
    }

    fn window(cx: &mut TestAppContext) -> (Entity<Host>, &mut VisualTestContext) {
        gitviber_ui::test_support::config_dir("markdown-test");
        cx.update(gitviber_ui::init);
        cx.add_window_view(|_, _| Host(None))
    }

    /// `text` shown inline as `callouts` reads it, drawn and parsed.
    fn show(
        host: &Entity<Host>,
        text: &str,
        callouts: Callouts,
        cx: &mut VisualTestContext,
    ) -> Entity<MarkdownView> {
        let view = cx.update(|window, cx| {
            let base = MarkdownBase {
                root: None,
                file: None,
                callouts,
            };
            let view = cx.new(|cx| MarkdownView::inline(text, base, window, cx));
            host.update(cx, |h, cx| {
                h.0 = Some(view.clone().into());
                cx.notify();
            });
            view
        });
        draw(cx);
        view
    }

    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
    }

    fn rendered(view: &Entity<MarkdownView>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |v, cx| {
            v.state.read(cx).rendered_text().as_str().to_string()
        })
    }

    #[gpui_kit::test]
    fn obsidians_syntax_stays_text_outside_a_vault(cx: &mut TestAppContext) {
        let (host, cx) = window(cx);
        for callouts in [Callouts::Repo, Callouts::GitHub] {
            let view = show(&host, "[[Note]] ==hi== #tag %%c%%", callouts, cx);
            assert_eq!(rendered(&view, cx).trim_end(), "[[Note]] ==hi== #tag %%c%%");
        }
    }

    #[gpui_kit::test]
    fn frontmatter_is_a_files_not_a_pull_requests(cx: &mut TestAppContext) {
        let (host, cx) = window(cx);
        let text = "---\ntags: [a, b]\nnested:\n  k: v\n---\nBody";
        let file = rendered(&show(&host, text, Callouts::Repo, cx), cx);
        assert!(
            file.contains("tags: a, b") && file.contains("nested: k: v") && !file.contains('['),
            "{file}"
        );
        // GitHub reads a rule, then a heading.
        let pr = rendered(&show(&host, text, Callouts::GitHub, cx), cx);
        assert!(
            pr.contains("tags: [a, b]") && !pr.contains("tags: a, b"),
            "{pr}"
        );
    }

    #[gpui_kit::test]
    fn raw_html_reads_as_on_github(cx: &mut TestAppContext) {
        let (host, cx) = window(cx);
        let text = "a <mark>m</mark> <a href=\"https://x.dev\">link</a> <script>alert(1)</script>b";
        let view = show(&host, text, Callouts::GitHub, cx);
        assert_eq!(rendered(&view, cx).trim_end(), "a m link b");
    }

    #[gpui_kit::test]
    fn details_open_and_shut(cx: &mut TestAppContext) {
        let (host, cx) = window(cx);
        let run = "<details><summary>Shot</summary>\n\nHidden **text**\n\n</details>";
        let text = format!(
            "Intro\n\n{run}\n\n<details open>\n<summary>Open <b>one</b></summary>\nshown\n</details>\n\nAfter"
        );
        let view = show(&host, &text, Callouts::GitHub, cx);
        let shown = rendered(&view, cx);
        // The run is one block: its markdown isn't drawn as the document's own.
        assert!(
            shown.contains("Shot") && shown.contains("After") && !shown.contains("Hidden text"),
            "{shown}"
        );
        let shut = blocks::parse_details(run).unwrap();
        let shut = (toggle_key(7, &shut.summary, &shut.body), shut.open);
        assert!(!shut.1);
        let open = blocks::parse_details(&text[7 + run.len() + 2..]).unwrap();
        assert!(open.open && open.summary == "<p>Open <b>one</b></p>");
        view.update(cx, |v, cx| {
            assert!(!v.is_open(shut.0, shut.1));
            v.toggle(shut.0, cx);
            assert!(v.is_open(shut.0, shut.1));
        });
        // Drawn open; a new text starts over.
        draw(cx);
        view.update(cx, |v, cx| {
            v.set_source("other", cx);
            assert!(!v.is_open(shut.0, shut.1));
        });
    }

    #[gpui_kit::test]
    fn callouts_fold_and_their_titles_are_markdown(cx: &mut TestAppContext) {
        let (host, cx) = window(cx);
        let text = "> [!faq]- Why **this** [Plan](Plan.md)?\n> Because.\n\n> [!tip]+ Open\n> Body";
        let view = show(&host, text, Callouts::Obsidian, cx);
        let faq = blocks::parse_callout(
            "> [!faq]- Why **this** [Plan](Plan.md)?\n> Because.",
            Callouts::Obsidian,
        )
        .unwrap();
        assert!(has_markup(&faq.title) && !has_markup("Open"));
        let key = toggle_key(0, &faq.title, &faq.body);
        view.update(cx, |v, cx| {
            assert!(!v.is_open(key, faq.fold == "+"));
            v.toggle(key, cx);
        });
        draw(cx);
        assert!(view.read_with(cx, |v, _| v.is_open(key, false)));
    }

    #[gpui_kit::test]
    fn footnotes_link_to_their_note_and_back(cx: &mut TestAppContext) {
        let (host, cx) = window(cx);
        let view = show(&host, "Claim[^1].\n\n[^1]: Source.", Callouts::GitHub, cx);
        let shown = rendered(&view, cx);
        assert!(
            shown.contains("Claim[1].") && shown.contains("[1]: Source. \u{21a9}"),
            "{shown}"
        );
        view.update(cx, |v, cx| {
            assert!(v.scroll_to_anchor("user-content-fn-1", cx));
            assert!(v.scroll_to_anchor("user-content-fnref-1", cx));
            assert!(!v.scroll_to_anchor("user-content-fn-2", cx));
        });
    }

    #[gpui_kit::test]
    fn a_reveal_waits_for_its_text(cx: &mut TestAppContext) {
        let (host, cx) = window(cx);
        // Past what's parsed at once, so it lands later.
        let text = format!("{}\n\nthe end", "word ".repeat(2000));
        let view = show(&host, "", Callouts::Repo, cx);
        view.update(cx, |v, cx| {
            v.set_source(&text, cx);
            v.reveal_source(text.len() - 7..text.len(), cx);
            assert!(v.reveal.is_some());
        });
        draw(cx);
        assert!(view.read_with(cx, |v, _| v.reveal.is_none()));
    }

    /// A long document filling the window, as a file's preview shows it, focused.
    fn long_page(cx: &mut TestAppContext) -> (Entity<MarkdownView>, &mut VisualTestContext) {
        let (host, cx) = window(cx);
        let text: String = (1..=300)
            .map(|i| format!("Paragraph {i}, a line of words to read.\n\n"))
            .collect();
        let view = cx.update(|window, cx| {
            let view = cx.new(|cx| MarkdownView::new(&text, MarkdownBase::default(), window, cx));
            host.update(cx, |h, cx| {
                h.0 = Some(view.clone().into());
                cx.notify();
            });
            window.focus(&view.focus_handle(cx), cx);
            view
        });
        draw(cx);
        (view, cx)
    }

    /// How far the document is scrolled, and how far it can be.
    fn scrolled(view: &Entity<MarkdownView>, cx: &mut VisualTestContext) -> (f32, f32) {
        view.read_with(cx, |v, cx| {
            let max = v.state.read(cx).list_state().max_offset_for_scrollbar().y;
            (f32::from(v.scroll_top(cx)), f32::from(max))
        })
    }

    #[gpui_kit::test]
    fn scroll_keys_move_a_focused_document(cx: &mut TestAppContext) {
        let (view, cx) = long_page(cx);
        let (_, max) = scrolled(&view, cx);
        let height = view.read_with(cx, |v, cx| {
            f32::from(v.state.read(cx).list_state().viewport_bounds().size.height)
        });
        assert!(height > 100. && max > height * 4., "{height} {max}");
        let (line, page) = (f32::from(scroll::LINE), height * scroll::PAGE);
        let (top, bottom) = if cfg!(target_os = "macos") {
            ("cmd-up", "cmd-down")
        } else {
            ("ctrl-home", "ctrl-end")
        };
        for (keys, want) in [
            ("up", 0.),
            ("down", line),
            ("down down", line * 3.),
            ("up", line * 2.),
            ("pagedown", line * 2. + page),
            ("space", line * 2. + page * 2.),
            ("shift-space", line * 2. + page),
            ("pageup", line * 2.),
            ("end", max),
            ("down pagedown space", max),
            ("home", 0.),
            (bottom, max),
            (top, 0.),
        ] {
            cx.simulate_keystrokes(keys);
            draw(cx);
            let (at, _) = scrolled(&view, cx);
            assert!((at - want).abs() < 0.5, "{keys}: at {at}, not {want}");
        }
    }

    /// A page of inline views, as a note or a pull request is: it scrolls with the keys, also
    /// from a view focused in it, and never takes them from its find box.
    struct Page {
        view: Entity<MarkdownView>,
        bar: Entity<FindBar>,
        steps: Vec<Step>,
    }

    impl Render for Page {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .key_context("MarkdownView")
                .size_full()
                .map(|el| scroll::on_keys(el, |p: &mut Self, step, _| p.steps.push(step), cx))
                .child(self.view.clone())
                .child(self.bar.clone())
        }
    }

    #[gpui_kit::test]
    fn an_inline_view_leaves_the_keys_to_its_page_and_a_field_keeps_them(cx: &mut TestAppContext) {
        gitviber_ui::test_support::config_dir("markdown-test");
        cx.update(gitviber_ui::init);
        let (page, cx) = cx.add_window_view(|window, cx| Page {
            view: cx
                .new(|cx| MarkdownView::inline("Some text", MarkdownBase::default(), window, cx)),
            bar: cx.new(|cx| FindBar::new(window, cx)),
            steps: Vec::new(),
        });
        draw(cx);
        cx.update(|window, cx| {
            let focus = page.read(cx).view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("pagedown down");
        let steps = page.read_with(cx, |p, _| p.steps.clone());
        assert_eq!(steps, [Step::Pages(1.), Step::Lines(1.)]);
        cx.update(|window, cx| {
            let bar = page.read(cx).bar.clone();
            bar.update(cx, |b, cx| b.focus(window, cx));
        });
        cx.simulate_keystrokes("space pagedown end");
        let (steps, query) = page.read_with(cx, |p, cx| {
            (p.steps.len(), p.bar.read(cx).query().text.clone())
        });
        assert_eq!((steps, query.as_str()), (2, " "), "typed into the box");
    }

    #[test]
    fn a_pull_requests_images_load_only_from_github() {
        let blocked = |base: &MarkdownBase, uri: &'static str| {
            matches!(
                base.image(&SharedUri::from(uri)),
                ImageSource::Resource(Resource::Path(p)) if p.as_os_str().is_empty()
            )
        };
        let pr = MarkdownBase {
            callouts: Callouts::GitHub,
            ..Default::default()
        };
        assert!(blocked(&pr, "https://img.shields.io/b.svg"));
        assert!(blocked(&pr, "data:image/png;base64,AAAA"));
        assert!(!blocked(
            &pr,
            "https://github.com/user-attachments/assets/x"
        ));
        assert!(!blocked(
            &MarkdownBase::default(),
            "https://img.shields.io/b.svg"
        ));
    }
}
