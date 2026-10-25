//! ⌘F on a PR's or an issue's page: one find box over all the text it shows (the title and the
//! line under it, checks and why one failed, commits and their messages, file names, labels,
//! the description and comments), stepping through the matches in the page's order. Text that's
//! folded away (a commit's message) or scrolled out of a list is searched too: the page shows it
//! when a match in it becomes the current one.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::ops::Range;
use std::panic::Location;

use gitviber_markdown::find::{MAX_MATCHES, find_matches};
use gitviber_markdown::{FindBar, FindBarEvent, FindChanged, FindQuery, MarkdownView};
use gitviber_ui::ActiveAppTheme as _;
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Div, Element, ElementId, Entity, EntityId, EventEmitter,
    FocusHandle, GlobalElementId, HighlightStyle, InspectorElementId, IntoElement, LayoutId,
    ParentElement as _, Pixels, ScrollHandle, SharedString, Styled as _, StyledText, Subscription,
    WeakEntity, Window, div, point, px, size,
};

/// A piece of text on the page: what it is and which one (a commit's index, a check's).
pub type Key = (&'static str, usize);

/// A page with a find: it lists what it shows and brings a piece into view.
pub trait FindHost: Sized + 'static {
    /// Every text the page shows in its order, what's folded away too; controls (buttons,
    /// section headings, counts) left out. Each piece drawn with [`Marks::text`] under its key.
    fn find_parts(&self, parts: &mut Parts, cx: &App);

    /// The current match is in `key`: draw it if it's folded away or scrolled out of a list.
    fn show_part(&mut self, _key: Key, _cx: &mut Context<Self>) {}

    /// The current match is in markdown view `view`: draw it if the page's list left it out.
    fn show_view(&mut self, _view: EntityId, _cx: &mut Context<Self>) {}
}

/// What a page lists for its find: only summed while the box is open, to notice a change, and
/// listed in full when there is one.
pub struct Parts {
    sum: DefaultHasher,
    list: Option<Listing>,
}

#[derive(Default)]
struct Listing {
    parts: Vec<Part>,
    /// Every text part's, one after the other: one regex runs over them all.
    text: String,
}

enum Part {
    /// Its bytes in [`Listing::text`].
    Text(Key, Range<usize>),
    View(Entity<MarkdownView>),
}

#[derive(Clone, Copy, PartialEq)]
enum PartId {
    Text(Key),
    View(EntityId),
}

impl Part {
    fn id(&self) -> PartId {
        match self {
            Part::Text(key, _) => PartId::Text(*key),
            Part::View(v) => PartId::View(v.entity_id()),
        }
    }
}

impl Parts {
    fn run<V: FindHost>(host: &V, list: bool, cx: &App) -> (u64, Option<Listing>) {
        let mut parts = Parts {
            sum: DefaultHasher::new(),
            list: list.then(Listing::default),
        };
        host.find_parts(&mut parts, cx);
        (parts.sum.finish(), parts.list)
    }

    pub fn text(&mut self, key: Key, text: &str) {
        if text.is_empty() {
            return;
        }
        key.hash(&mut self.sum);
        text.hash(&mut self.sum);
        if let Some(l) = &mut self.list {
            let start = l.text.len();
            l.text.push_str(text);
            l.parts.push(Part::Text(key, start..l.text.len()));
            // Between parts: no match runs from one into the next.
            l.text.push('\n');
        }
    }

    pub fn view(&mut self, view: &Entity<MarkdownView>) {
        view.entity_id().hash(&mut self.sum);
        if let Some(l) = &mut self.list {
            l.parts.push(Part::View(view.clone()));
        }
    }
}

/// A page's find, built on its first ⌘F.
#[derive(Default)]
pub struct PageFind(OnceCell<Built>);

struct Built {
    finder: Entity<Finder>,
    _subs: [Subscription; 3],
}

impl PageFind {
    /// ⌘F: the box over the page, focused with its text selected; the last query marked again.
    /// `scroll`: the page's, which a match revealed scrolls, or None for a page drawn as a list,
    /// which a drawn match asks to scroll; `page`: focused when the box closes.
    pub fn open<V: FindHost>(
        &self,
        host: &V,
        scroll: Option<&ScrollHandle>,
        page: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        let (sum, listing) = Parts::run(host, true, cx);
        let built = self.0.get_or_init(|| {
            let finder = cx.new(|cx| Finder::new(scroll.cloned(), page.clone(), window, cx));
            let redraw = cx.observe(&finder, |_, _, cx| cx.notify());
            let show = cx.subscribe(&finder, |host: &mut V, _, e: &ShowPart, cx| {
                host.show_part(e.0, cx)
            });
            let show_view = cx.subscribe(&finder, |host: &mut V, _, e: &ShowView, cx| {
                host.show_view(e.0, cx)
            });
            Built {
                finder,
                _subs: [redraw, show, show_view],
            }
        });
        let listing = listing.unwrap_or_default();
        built
            .finder
            .update(cx, |f, cx| f.open(listing, sum, window, cx));
    }

    /// At each frame while the box is open: what the page shows is looked through again where it
    /// changed (a comment came, the files landed, a failure was shown).
    pub fn sync<V: FindHost>(&self, host: &V, cx: &mut App) {
        let Some(b) = self.0.get() else { return };
        let f = b.finder.read(cx);
        if !f.open || Parts::run(host, false, cx).0 == f.sum {
            return;
        }
        let (sum, listing) = Parts::run(host, true, cx);
        let listing = listing.unwrap_or_default();
        b.finder.update(cx, |f, cx| f.take(listing, sum, cx));
    }

    /// What draws the page's text with its matches marked; cheap to clone into a list's rows.
    pub fn marks(&self) -> Marks {
        Marks(self.0.get().map(|b| b.finder.clone()))
    }

    /// The box, over the top right of the page, while it's open.
    pub fn bar(&self, cx: &App) -> Option<Div> {
        let f = self.0.get()?.finder.read(cx);
        f.open.then(|| {
            div()
                .absolute()
                .top(px(8.))
                .right(px(16.))
                .child(f.bar.clone())
        })
    }

    #[cfg(test)]
    pub(crate) fn finder(&self) -> Option<Entity<Finder>> {
        self.0.get().map(|b| b.finder.clone())
    }
}

#[derive(Clone)]
pub struct Marks(Option<Entity<Finder>>);

impl Marks {
    /// `text`, the page's part `key`, with its matches washed while the box is open.
    pub fn text(&self, key: Key, text: impl Into<SharedString>, cx: &App) -> FindText {
        let shown = text.into();
        let whole = shown.clone();
        self.draw(key, &whole, 0..whole.len(), shown, cx)
    }

    /// Bytes `range` of part `key`, `whole`: a piece of it drawn apart (a line of a message).
    pub fn slice(&self, key: Key, whole: &str, range: Range<usize>, cx: &App) -> FindText {
        let shown = SharedString::from(whole[range.clone()].to_string());
        self.draw(key, whole, range, shown, cx)
    }

    /// Whether part `key` has matches to mark.
    pub fn any(&self, key: Key, cx: &App) -> bool {
        self.0.as_ref().is_some_and(|f| {
            let f = f.read(cx);
            f.open && f.by_key.get(&key).is_some_and(|p| !f.marks[*p].is_empty())
        })
    }

    fn draw(
        &self,
        key: Key,
        whole: &str,
        range: Range<usize>,
        shown: SharedString,
        cx: &App,
    ) -> FindText {
        let mut text = FindText {
            text: StyledText::new(shown),
            reveal: None,
            within: None,
        };
        let Some(finder) = &self.0 else { return text };
        let f = finder.read(cx);
        let Some(&p) = f.by_key.get(&key).filter(|_| f.open) else {
            return text;
        };
        let Part::Text(_, r) = &f.listing.parts[p] else {
            return text;
        };
        // The page drew other text than it listed (a time that moved on): the next sync recounts.
        if f.listing.text[r.clone()] != *whole || f.marks[p].is_empty() {
            return text;
        }
        let t = cx.app_theme();
        let mut marks = Vec::new();
        for (i, m) in f.marks[p].iter().enumerate() {
            let (start, end) = (m.start.max(range.start), m.end.min(range.end));
            if start >= end {
                continue;
            }
            let current = f.current == Some(f.bases[p] + i);
            if current && f.reveal == Some(key) && range.contains(&m.start) {
                text.reveal = Some(Reveal {
                    at: m.start - range.start,
                    page: f.scroll.clone(),
                    finder: finder.downgrade(),
                });
            }
            let wash = if current {
                t.find_current
            } else {
                t.find_match
            };
            let style = HighlightStyle {
                background_color: Some(wash),
                ..Default::default()
            };
            marks.push((start - range.start..end - range.start, style));
        }
        text.text = text.text.with_highlights(marks);
        text
    }
}

/// A part's text, its matches washed; the current one scrolled to once it's drawn.
pub struct FindText {
    text: StyledText,
    reveal: Option<Reveal>,
    within: Option<ScrollHandle>,
}

struct Reveal {
    /// The match's first byte in the text drawn.
    at: usize,
    /// None: the page is a list, asked to scroll by autoscroll.
    page: Option<ScrollHandle>,
    finder: WeakEntity<Finder>,
}

impl FindText {
    /// It sits in a box that scrolls on its own, inside the page (the commits list).
    pub fn within(mut self, scroll: &ScrollHandle) -> Self {
        if self.reveal.is_some() {
            self.within = Some(scroll.clone());
        }
        self
    }
}

impl IntoElement for FindText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for FindText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.text.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text.prepaint(id, inspector, bounds, state, window, cx);
        let Some(r) = self.reveal.take() else { return };
        let layout = self.text.layout();
        if let Some(at) = layout.position_for_index(r.at) {
            let mut line = Bounds::new(at, size(px(1.), layout.line_height()));
            for scroll in self.within.iter().chain(&r.page) {
                line.origin.y += scroll_to(scroll, line);
            }
            // A list scrolls the bounds to its edge: some lines of the page show past the match.
            if r.page.is_none() {
                let margin = layout.line_height() * 3.;
                window.request_autoscroll(Bounds::from_corners(
                    point(line.left(), line.top() - margin),
                    point(line.right(), line.bottom() + margin),
                ));
            }
            window.refresh();
        }
        _ = r.finder.update(cx, |f, _| f.reveal = None);
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .paint(id, inspector, bounds, state, prepaint, window, cx);
    }
}

/// Scrolls `line` a third of the way down `scroll`'s box when it's out of it; how far it moved.
fn scroll_to(scroll: &ScrollHandle, line: Bounds<Pixels>) -> Pixels {
    let view = scroll.bounds();
    if line.top() >= view.top() && line.bottom() <= view.bottom() {
        return px(0.);
    }
    let offset = scroll.offset();
    let max = scroll.max_offset().y;
    let y = (offset.y + view.top() + view.size.height / 3. - line.top()).clamp(-max, px(0.));
    scroll.set_offset(point(offset.x, y));
    y - offset.y
}

/// The current match moved into text part `.0`: the page shows it.
struct ShowPart(Key);

/// The current match moved into markdown view `.0`: the page draws it.
struct ShowView(EntityId);

impl EventEmitter<ShowPart> for Finder {}
impl EventEmitter<ShowView> for Finder {}

pub struct Finder {
    bar: Entity<FindBar>,
    open: bool,
    /// The page's, None for a list (see `PageFind::open`).
    scroll: Option<ScrollHandle>,
    page: FocusHandle,
    /// What the page listed last, and its sum.
    listing: Listing,
    sum: u64,
    by_key: HashMap<Key, usize>,
    /// Each text part's matches, in bytes of its text; a view keeps its own.
    marks: Vec<Vec<Range<usize>>>,
    /// Each part's matches, and the page-wide index of its first.
    counts: Vec<usize>,
    bases: Vec<usize>,
    /// The current match over the whole page, the parts' in turn.
    current: Option<usize>,
    /// The text part whose current match waits to be scrolled to, once it's drawn.
    reveal: Option<Key>,
    error: Option<String>,
    views: Vec<Entity<MarkdownView>>,
    _bar: Subscription,
    /// Each view's text changing under the find (a comment edited): the count follows.
    _views: Vec<Subscription>,
}

impl Finder {
    fn new(
        scroll: Option<ScrollHandle>,
        page: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let bar = cx.new(|cx| FindBar::new(window, cx));
        let sub = cx.subscribe_in(
            &bar,
            window,
            |this, _, e: &FindBarEvent, window, cx| match e {
                FindBarEvent::Changed => this.run(true, None, cx),
                FindBarEvent::Step(dir) => this.step(*dir, cx),
                FindBarEvent::Close => this.close(window, cx),
            },
        );
        Finder {
            bar,
            open: false,
            scroll,
            page,
            listing: Listing::default(),
            sum: 0,
            by_key: HashMap::new(),
            marks: vec![],
            counts: vec![],
            bases: vec![],
            current: None,
            reveal: None,
            error: None,
            views: vec![],
            _bar: sub,
            _views: vec![],
        }
    }

    /// The current match, how many there are, and the text part still to be scrolled to.
    #[cfg(test)]
    pub(crate) fn state(&self) -> (Option<usize>, usize, Option<Key>) {
        (self.current, self.counts.iter().sum(), self.reveal)
    }

    fn open(&mut self, listing: Listing, sum: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.open = true;
            self.take_listing(listing, sum, cx);
            self.run(true, None, cx);
        }
        self.bar.update(cx, |b, cx| b.focus(window, cx));
        cx.notify();
    }

    /// The page shows other text: looked through again, the current match kept where it can.
    fn take(&mut self, listing: Listing, sum: u64, cx: &mut Context<Self>) {
        let anchor = self.anchor();
        self.take_listing(listing, sum, cx);
        self.run(false, anchor, cx);
    }

    fn take_listing(&mut self, listing: Listing, sum: u64, cx: &mut Context<Self>) {
        let views: Vec<Entity<MarkdownView>> = listing
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::View(v) => Some(v.clone()),
                Part::Text(..) => None,
            })
            .collect();
        if views != self.views {
            // A view the page no longer shows (the description while it's edited) keeps no marks.
            let none = FindQuery::default();
            for gone in self.views.iter().filter(|v| !views.contains(v)) {
                gone.update(cx, |v, cx| _ = v.find(&none, cx));
            }
            self._views = views
                .iter()
                .map(|v| {
                    // A list scrolls to a view's match by itself: the view asks it to.
                    if let Some(scroll) = &self.scroll {
                        v.update(cx, |v, _| v.reveal_in(scroll.clone()));
                    }
                    cx.subscribe(v, |this, _, _: &FindChanged, cx| {
                        let anchor = this.anchor();
                        this.apply(false, anchor, cx)
                    })
                })
                .collect();
            self.views = views;
        }
        self.by_key = listing
            .parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| match p {
                Part::Text(key, _) => Some((*key, i)),
                Part::View(_) => None,
            })
            .collect();
        self.listing = listing;
        self.sum = sum;
    }

    /// Where the current match is: its part and which of the part's it is.
    fn anchor(&self) -> Option<(PartId, usize)> {
        let c = self.current?;
        let p = (0..self.counts.len())
            .find(|&p| (self.bases[p]..self.bases[p] + self.counts[p]).contains(&c))?;
        Some((self.listing.parts.get(p)?.id(), c - self.bases[p]))
    }

    /// Looks for the box's query everywhere. `fresh` (typed, opened): from the first match,
    /// revealed; else the current one stays at `anchor` where it can.
    fn run(&mut self, fresh: bool, anchor: Option<(PartId, usize)>, cx: &mut Context<Self>) {
        let query = self.bar.read(cx).query().clone();
        let mut error = None;
        let found = find_matches(&self.listing.text, &query, MAX_MATCHES).unwrap_or_else(|e| {
            error = Some(e);
            vec![]
        });
        // Both in the text's order: each match goes to the part it's in.
        let mut found = found.into_iter().peekable();
        self.marks = self
            .listing
            .parts
            .iter()
            .map(|p| {
                let Part::Text(_, r) = p else { return vec![] };
                let mut marks = vec![];
                while let Some(m) = found.next_if(|m| m.start < r.end) {
                    if m.start >= r.start && m.end <= r.end {
                        marks.push(m.start - r.start..m.end - r.start);
                    }
                }
                marks
            })
            .collect();
        for v in &self.views {
            if let Err(e) = v.update(cx, |v, cx| v.find(&query, cx)) {
                error = Some(e);
            }
        }
        self.error = error;
        if fresh {
            self.current = Some(0);
        }
        self.apply(fresh, anchor, cx);
    }

    /// Counts again, puts the current match at `anchor` (or keeps its index), marks it where it
    /// falls and none elsewhere, and reveals it if `reveal`.
    fn apply(&mut self, reveal: bool, anchor: Option<(PartId, usize)>, cx: &mut Context<Self>) {
        self.counts = self
            .listing
            .parts
            .iter()
            .zip(&self.marks)
            .map(|(p, marks)| match p {
                Part::Text(..) => marks.len(),
                Part::View(v) => v.read(cx).matches().1,
            })
            .collect();
        let mut total = 0;
        self.bases = self
            .counts
            .iter()
            .map(|n| {
                total += n;
                total - n
            })
            .collect();
        let at = anchor.and_then(|(id, local)| {
            let p = self.listing.parts.iter().position(|p| p.id() == id)?;
            Some(self.bases[p] + local.min(self.counts[p].saturating_sub(1)))
        });
        self.current = at
            .or(self.current)
            .filter(|_| total > 0)
            .map(|c| c.min(total - 1));
        let (mut text, mut view) = (None, None);
        for (p, part) in self.listing.parts.iter().enumerate() {
            let local = self
                .current
                .filter(|c| (self.bases[p]..self.bases[p] + self.counts[p]).contains(c))
                .map(|c| c - self.bases[p]);
            match part {
                Part::View(v) => {
                    if local.is_some() {
                        view = Some(v.entity_id());
                    }
                    v.update(cx, |v, cx| {
                        v.select_match(local, reveal && local.is_some(), cx)
                    })
                }
                Part::Text(key, _) if local.is_some() => text = Some(*key),
                Part::Text(..) => {}
            }
        }
        if reveal {
            self.reveal = text;
            if let Some(key) = text {
                cx.emit(ShowPart(key));
            }
            if let Some(id) = view {
                cx.emit(ShowView(id));
            }
        }
        let found = match &self.error {
            Some(e) => Err(e.clone()),
            None => Ok((self.current, total)),
        };
        self.bar.update(cx, |b, cx| b.set_found(found, cx));
        cx.notify();
    }

    /// The next match (1) or the one before (-1), round the page; the first or the last when
    /// none is current.
    fn step(&mut self, dir: isize, cx: &mut Context<Self>) {
        let total: usize = self.counts.iter().sum();
        if total == 0 {
            return;
        }
        self.current = Some(match self.current {
            Some(c) => (c as isize + dir).rem_euclid(total as isize) as usize,
            None if dir < 0 => total - 1,
            None => 0,
        });
        self.apply(true, None, cx);
    }

    /// Esc or ×: the marks go, the query stays for the next ⌘F, focus goes back to the page.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.open) {
            return;
        }
        let none = FindQuery::default();
        for v in self.views.drain(..) {
            v.update(cx, |v, cx| _ = v.find(&none, cx));
        }
        self._views.clear();
        self.listing = Listing::default();
        self.sum = 0;
        self.by_key.clear();
        self.marks.clear();
        self.counts.clear();
        self.bases.clear();
        self.current = None;
        self.reveal = None;
        self.page.focus(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use gitviber_markdown::MarkdownBase;
    use gpui_kit::{
        InteractiveElement as _, Render, StatefulInteractiveElement as _, TestAppContext,
        VisualTestContext,
    };

    use gpui_kit::prelude::FluentBuilder as _;

    use super::*;

    struct Host {
        find: PageFind,
        title: String,
        views: Vec<Entity<MarkdownView>>,
        /// Listed always, drawn once shown: far down the page.
        folded: String,
        unfolded: bool,
        scroll: ScrollHandle,
        focus: FocusHandle,
    }

    impl FindHost for Host {
        fn find_parts(&self, parts: &mut Parts, _: &App) {
            parts.text(("title", 0), &self.title);
            for v in &self.views {
                parts.view(v);
            }
            parts.text(("folded", 0), &self.folded);
        }

        fn show_part(&mut self, key: Key, cx: &mut Context<Self>) {
            if key == ("folded", 0) {
                self.unfolded = true;
                cx.notify();
            }
        }
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.find.sync(self, cx);
            let marks = self.find.marks();
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("page")
                        .track_focus(&self.focus)
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(marks.text(("title", 0), self.title.clone(), cx))
                        .children(self.views.clone())
                        .child(div().h(px(5000.)))
                        .when(self.unfolded, |d| {
                            d.child(marks.text(("folded", 0), self.folded.clone(), cx))
                        }),
                )
                .children(self.find.bar(cx))
        }
    }

    fn init(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("gitviber-github-test-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    fn open(host: &Entity<Host>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            host.update(cx, |h, cx| {
                let (scroll, focus) = (h.scroll.clone(), h.focus.clone());
                h.find.open(h, Some(&scroll), &focus, window, cx);
            })
        });
        draw(cx);
    }

    #[gpui_kit::test]
    fn one_find_steps_through_the_page_and_shows_what_is_folded(cx: &mut TestAppContext) {
        init(cx);
        let (host, cx) = cx.add_window_view(|window, cx| {
            let view = |text: &str, window: &mut Window, cx: &mut Context<Host>| {
                cx.new(|cx| MarkdownView::inline(text, MarkdownBase::default(), window, cx))
            };
            Host {
                find: PageFind::default(),
                title: "Fix the foo".into(),
                views: vec![
                    view("The **foo** body", window, cx),
                    view("A foo, and a Foo", window, cx),
                ],
                folded: "a foo deep down".into(),
                unfolded: false,
                scroll: ScrollHandle::new(),
                focus: cx.focus_handle(),
            }
        });
        draw(cx);
        open(&host, cx);
        cx.simulate_input("foo");
        draw(cx);
        let finder = host.read_with(cx, |h, _| h.find.finder()).expect("open");
        let views = host.read_with(cx, |h, _| h.views.clone());
        let state = |cx: &mut VisualTestContext| {
            let current = finder.read_with(cx, |f, _| f.current);
            let marks: Vec<_> = views
                .iter()
                .map(|v| v.read_with(cx, |v, _| v.matches()))
                .collect();
            (current, marks)
        };
        let step = |dir, cx: &mut VisualTestContext| {
            finder.update(cx, |f, cx| f.step(dir, cx));
            draw(cx);
        };
        // Typed: the first match, the title's; the folded text counts too.
        assert_eq!(state(cx), (Some(0), vec![(None, 1), (None, 2)]));
        assert_eq!(
            finder.read_with(cx, |f, _| f.counts.iter().sum::<usize>()),
            5
        );
        step(1, cx);
        assert_eq!(state(cx), (Some(1), vec![(Some(0), 1), (None, 2)]));
        step(1, cx);
        step(1, cx);
        assert_eq!(state(cx), (Some(3), vec![(None, 1), (Some(1), 2)]));
        // Into the folded text: the page shows it and scrolls down to it.
        step(1, cx);
        assert_eq!(state(cx), (Some(4), vec![(None, 1), (None, 2)]));
        assert!(host.read_with(cx, |h, _| h.unfolded));
        assert_eq!(finder.read_with(cx, |f, _| f.reveal), None);
        let down = host.read_with(cx, |h, _| h.scroll.offset().y);
        assert!(down < px(-4000.), "scrolled to {down:?}");
        // A comment edited above it: the current match stays the folded one.
        views[1].update(cx, |v, cx| v.set_source("Nothing here", cx));
        draw(cx);
        assert_eq!(state(cx), (Some(2), vec![(None, 1), (None, 0)]));
        // The title changed: looked through again, still on the folded one.
        host.update(cx, |h, cx| {
            h.title = "Foo foo".into();
            cx.notify();
        });
        draw(cx);
        assert_eq!(state(cx).0, Some(3));
        assert_eq!(
            finder.read_with(cx, |f, _| f.counts.iter().sum::<usize>()),
            4
        );
        // Back round to the title: the page scrolls up to it.
        step(1, cx);
        assert_eq!(state(cx).0, Some(0));
        assert_eq!(host.read_with(cx, |h, _| h.scroll.offset().y), px(0.));
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(state(cx), (None, vec![(None, 0), (None, 0)]));
        assert!(host.read_with(cx, |h, cx| h.find.bar(cx).is_none()));
        assert!(finder.read_with(cx, |f, _| f.listing.text.is_empty()));
        drop((finder, views));

        // A page closed with its find open gives the box and the marks back.
        let round = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                host.update(cx, |h, cx| {
                    h.views = vec![cx.new(|cx| {
                        MarkdownView::inline("foo", MarkdownBase::default(), window, cx)
                    })];
                })
            });
            open(&host, cx);
            cx.simulate_input("o");
            draw(cx);
            host.update(cx, |h, _| {
                h.find = PageFind::default();
                h.views.clear();
            });
            draw(cx);
        };
        round(cx);
        let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
        for _ in 0..4 {
            round(cx);
        }
        cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    }

    #[gpui_kit::test]
    fn a_match_never_runs_from_one_part_into_the_next(cx: &mut TestAppContext) {
        init(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host {
            find: PageFind::default(),
            title: "ends in ab".into(),
            views: vec![],
            folded: "cd starts it".into(),
            unfolded: true,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
        });
        draw(cx);
        open(&host, cx);
        let finder = host.read_with(cx, |h, _| h.find.finder()).expect("open");
        let total = |cx: &mut VisualTestContext| {
            finder.read_with(cx, |f, _| f.counts.iter().sum::<usize>())
        };
        // A regex, which could match the break between the title and the next part.
        cx.simulate_keystrokes("alt-cmd-r");
        cx.simulate_input(r"b\sc");
        draw(cx);
        assert_eq!(total(cx), 0);
        // Each part is a line of its own.
        cx.simulate_keystrokes("cmd-a backspace");
        cx.simulate_input("^cd|ab$");
        draw(cx);
        assert_eq!(total(cx), 2);
    }
}
