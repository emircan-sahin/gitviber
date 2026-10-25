//! A note as Obsidian's reading view shows it. The note is cut into parts (markup::parts): each
//! text part is a gitviber-markdown view of the part rewritten (links to the files they resolve
//! to, highlights, tags), each embed alone on a line an `EmbedView` (another note or a part of
//! it in a box, an image, a player's card). Parts are built as they scroll into view and let go
//! past a bounded number, so a long note costs what's on screen.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use gitviber_core::api;
use gitviber_files::MediaView;
use gitviber_files::media::{MediaKind, media_kind};
use gitviber_markdown::find::{FindQuery, MAX_MATCHES, find_matches};
use gitviber_markdown::{
    BoundedImageCache, Callouts, FindBar, FindBarEvent, MarkdownBase, MarkdownEvent, MarkdownView,
};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::{editor, find};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Selection, Toast, Workspace};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ListAlignment, ListOffset, ListState, ObjectFit, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription,
    Task, WeakEntity, Window, div, image_cache, img, list, prelude::FluentBuilder as _, px,
};

use crate::links::{LinkIndex, note_name, split_target};
use crate::markup::{self, Part, Target};
use crate::properties;
use crate::vaults::{Vaults, VaultsEvent};

/// Notes embedded in notes embedded in…: deeper than this shows a link, as a cycle does.
const MAX_DEPTH: usize = 4;
/// Parts kept built out of sight; older ones are built again when they scroll back.
const MAX_BUILT: usize = 48;
/// An embedded note bigger than this shows a line saying so.
const MAX_EMBED_BYTES: usize = 512 * 1024;
/// A PDF embedded in a note is this tall, as the old app's; a video without a size, this.
const PDF_HEIGHT: f32 = 640.;
const VIDEO_HEIGHT: f32 = 360.;
const AUDIO_HEIGHT: f32 = 56.;

/// Where a note's links and embeds resolve from, for every view drawn for it.
#[derive(Clone)]
pub(crate) struct NoteCx {
    /// The vault's folder.
    pub vault: String,
    /// The note's vault-relative path (or the canvas's).
    pub path: String,
    /// 0 for the tab's note, one more per embed it's inside.
    pub depth: usize,
    /// The notes ("file#anchor") embedding this one, to stop one embedding itself.
    pub trail: Vec<String>,
    pub workspace: WeakEntity<Workspace>,
    /// The tab's note view, which scrolls to a part of itself.
    pub top: Option<WeakEntity<NoteView>>,
}

impl NoteCx {
    fn base(&self) -> MarkdownBase {
        MarkdownBase {
            root: Some(PathBuf::from(&self.vault)),
            file: Some(self.path.clone()),
            callouts: Callouts::Obsidian,
        }
    }

    fn inner(&self, file: &str, anchor: &str) -> NoteCx {
        let mut trail = self.trail.clone();
        trail.push(format!("{file}#{anchor}"));
        NoteCx {
            path: file.to_string(),
            depth: self.depth + 1,
            trail,
            top: None,
            ..self.clone()
        }
    }

    pub fn toast(&self, toast: Toast, cx: &mut App) {
        _ = self.workspace.update(cx, |ws, cx| ws.toast(toast, cx));
    }
}

/// Opens a file of `vault` in a tab, scrolled to `anchor` when there's one.
pub(crate) fn open_in_tab(
    workspace: &WeakEntity<Workspace>,
    vault: &str,
    path: &str,
    anchor: &str,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ws) = workspace.upgrade() else {
        return;
    };
    let sel = Selection::Vault {
        vault: vault.to_string(),
        path: path.to_string(),
    };
    let anchor = anchor.to_string();
    ws.update(cx, |ws, cx| {
        if !ws.open(sel.clone(), preview, window, cx) || anchor.is_empty() {
            return;
        }
        let tab = ws
            .tab_for(&sel)
            .and_then(|t| t.item.handle())
            .and_then(|h| h.view().downcast::<crate::view::VaultTab>().ok());
        if let Some(tab) = tab {
            tab.update(cx, |t, cx| t.reveal(&anchor, cx));
        }
    });
}

/// Follows a link a part handed back (markup::href, decoded), after the event: the views it
/// came through may be the ones it scrolls.
pub(crate) fn follow(note: &NoteCx, path: &str, window: &mut Window, cx: &mut App) {
    let note = note.clone();
    let path = path.to_string();
    window.defer(cx, move |window, cx| {
        let (file, anchor) = match markup::target(&path) {
            Target::Missing(name) => {
                return note.toast(
                    Toast::info("This vault has no file by that name.")
                        .title(format!("No note named “{name}”")),
                    cx,
                );
            }
            Target::File { path, anchor } => (path, anchor),
        };
        let index = Vaults::global(cx).update(cx, |m, cx| m.index(&note.vault, cx));
        let file = match &index {
            Some(i) if !i.contains(&file) => {
                // A link the rewriting left alone: resolved as written in the note.
                let dir = note.path.rfind('/').map_or("", |i| &note.path[..=i]);
                match i.resolve(file.strip_prefix(dir).unwrap_or(&file), &note.path) {
                    Some(f) => f,
                    None => {
                        return note.toast(
                            Toast::info("This vault has no file by that name.")
                                .title(format!("No note named “{file}”")),
                            cx,
                        );
                    }
                }
            }
            _ => file,
        };
        if file == note.path
            && note.depth == 0
            && let Some(top) = note.top.as_ref().and_then(WeakEntity::upgrade)
        {
            top.update(cx, |v, cx| v.reveal(&anchor, cx));
            return;
        }
        open_in_tab(
            &note.workspace,
            &note.vault,
            &file,
            &anchor,
            false,
            window,
            cx,
        );
    });
}

enum Built {
    Text {
        view: Entity<MarkdownView>,
        markdown: String,
        _sub: Subscription,
    },
    Embed(Entity<EmbedView>),
}

/// A note's parts and the views built for the ones drawn lately.
pub(crate) struct Doc {
    note: NoteCx,
    by_heading: bool,
    parts: Vec<Part>,
    /// The note's properties, drawn above its first part.
    props: Vec<(String, properties::Value)>,
    built: HashMap<usize, (Built, u64)>,
    frame: u64,
    /// ⌘F's query and its current match (the part, and which of that part's matches), marked
    /// in each text part as it's built.
    find: Option<(FindQuery, Option<(usize, usize)>)>,
    /// The current match's part wasn't built yet: it scrolls to the match once it is.
    reveal: bool,
}

impl Doc {
    pub fn new(note: NoteCx, src: &str, by_heading: bool) -> Doc {
        let parts = markup::parts(&markup::strip_comments(src), by_heading);
        Doc {
            props: props_of(&parts),
            parts,
            note,
            by_heading,
            built: HashMap::new(),
            frame: 0,
            find: None,
            reveal: false,
        }
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Before the owner draws: what it draws this time is kept over what it doesn't.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    fn markdown(&self, ix: usize, index: &LinkIndex) -> String {
        match self.parts.get(ix) {
            // The properties are drawn apart (`properties`), not as markdown.
            Some(Part::Text { source, .. }) => {
                let cx = markup::Context {
                    index,
                    note: &self.note.path,
                };
                markup::to_markdown("", source, &cx)
            }
            _ => String::new(),
        }
    }

    /// The note's text changed: parts that stayed keep their views, a text part that changed
    /// shows its new text in the one it had. The changed range, for the list: (start, old
    /// count, new count).
    pub fn replace(
        &mut self,
        src: &str,
        index: Option<&LinkIndex>,
        cx: &mut App,
    ) -> (usize, usize, usize) {
        let parts = markup::parts(&markup::strip_comments(src), self.by_heading);
        let (old, new) = (self.parts.len(), parts.len());
        let prefix = self
            .parts
            .iter()
            .zip(&parts)
            .take_while(|(a, b)| a == b)
            .count();
        let suffix = self.parts[prefix..]
            .iter()
            .rev()
            .zip(parts[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        self.props = props_of(&parts);
        let was = std::mem::replace(&mut self.parts, parts);
        for (ix, (built, stamp)) in std::mem::take(&mut self.built) {
            let at = if ix < prefix {
                Some(ix)
            } else if ix >= old - suffix {
                Some(ix + new - old)
            } else {
                // A text part rewritten in place keeps its view (its images, its height).
                (ix < new - suffix
                    && matches!(
                        (&was[ix], &self.parts[ix]),
                        (Part::Text { .. }, Part::Text { .. })
                    ))
                .then_some(ix)
            };
            let Some(at) = at else { continue };
            let built = match (built, index) {
                (
                    Built::Text {
                        view,
                        markdown,
                        _sub,
                    },
                    Some(index),
                ) if at >= prefix && at < new - suffix => {
                    let md = self.markdown(at, index);
                    if md != markdown {
                        view.update(cx, |v, cx| v.set_source(&md, cx));
                    }
                    Built::Text {
                        view,
                        markdown: md,
                        _sub,
                    }
                }
                (b, _) => b,
            };
            self.built.insert(at, (built, stamp));
        }
        (prefix, old - prefix - suffix, new - prefix - suffix)
    }

    /// The vault's files changed: links are resolved again.
    pub fn reindex(&mut self, index: &LinkIndex, cx: &mut App) {
        let ixs: Vec<usize> = self.built.keys().copied().collect();
        for ix in ixs {
            let md = self.markdown(ix, index);
            if let Some((Built::Text { view, markdown, .. }, _)) = self.built.get_mut(&ix)
                && *markdown != md
            {
                view.update(cx, |v, cx| v.set_source(&md, cx));
                *markdown = md;
            }
        }
    }

    /// The part at `ix`, built if it isn't; the note's properties above the first.
    pub fn element<V: 'static>(
        &mut self,
        ix: usize,
        index: &LinkIndex,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> AnyElement {
        let part = self.part(ix, index, window, cx);
        if ix > 0 || self.props.is_empty() {
            return part;
        }
        div()
            .flex()
            .flex_col()
            .child(properties::element(&self.props, cx))
            .child(part)
            .into_any_element()
    }

    fn part<V: 'static>(
        &mut self,
        ix: usize,
        index: &LinkIndex,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> AnyElement {
        if let Some((built, stamp)) = self.built.get_mut(&ix) {
            *stamp = self.frame;
            return match built {
                Built::Text { view, .. } => view.clone().into_any_element(),
                Built::Embed(view) => view.clone().into_any_element(),
            };
        }
        let built = match &self.parts[ix] {
            Part::Text { .. } => {
                let markdown = self.markdown(ix, index);
                let base = self.note.base();
                let view = cx.new(|cx| MarkdownView::inline(&markdown, base, window, cx));
                let note = self.note.clone();
                let sub =
                    cx.subscribe_in(&view, window, move |_, _, e: &MarkdownEvent, window, cx| {
                        let MarkdownEvent::OpenFile { path, .. } = e;
                        follow(&note, path, window, cx);
                    });
                Built::Text {
                    view,
                    markdown,
                    _sub: sub,
                }
            }
            Part::Embed { target, alias } => {
                let (note, target, alias) = (self.note.clone(), target.clone(), alias.clone());
                Built::Embed(cx.new(|cx| EmbedView::new(note, target, alias, false, cx)))
            }
        };
        let element = match &built {
            Built::Text { view, .. } => view.clone().into_any_element(),
            Built::Embed(view) => view.clone().into_any_element(),
        };
        self.built.insert(ix, (built, self.frame));
        let reveal = self.reveal && self.current_part() == Some(ix);
        self.reveal &= !reveal;
        self.mark(ix, reveal, cx);
        self.evict();
        element
    }

    /// Past MAX_BUILT, the views drawn longest ago go; never one drawn this frame.
    fn evict(&mut self) {
        while self.built.len() > MAX_BUILT {
            let oldest = self
                .built
                .iter()
                .filter(|(_, (_, stamp))| *stamp < self.frame)
                .min_by_key(|(_, (_, stamp))| *stamp)
                .map(|(ix, _)| *ix);
            match oldest {
                Some(ix) => {
                    self.built.remove(&ix);
                }
                None => break,
            }
        }
    }

    /// Where `query` is found as the note reads: (part, which match in it), at most MAX_MATCHES.
    /// Counted in every part's text, built or not, so scrolling doesn't change the count.
    /// Err: why the regex doesn't parse.
    pub fn search(&self, query: &FindQuery) -> Result<Vec<(usize, usize)>, String> {
        let mut hits = Vec::new();
        if query.text.is_empty() {
            return Ok(hits);
        }
        for (ix, part) in self.parts.iter().enumerate() {
            let Part::Text { source, .. } = part else {
                continue;
            };
            let left = MAX_MATCHES - hits.len();
            let n = find_matches(&markup::plain(source), query, left)?.len();
            hits.extend((0..n).map(|k| (ix, k)));
            if hits.len() >= MAX_MATCHES {
                break;
            }
        }
        Ok(hits)
    }

    /// Marks a find in the parts built (None clears them); `reveal` scrolls to its current
    /// match, now or once its part is built.
    pub fn set_find(
        &mut self,
        find: Option<(FindQuery, Option<(usize, usize)>)>,
        reveal: bool,
        cx: &mut App,
    ) {
        self.find = find;
        let current = self.current_part();
        self.reveal = reveal && current.is_some_and(|ix| !self.built.contains_key(&ix));
        let ixs: Vec<usize> = self.built.keys().copied().collect();
        for ix in ixs {
            self.mark(ix, reveal && current == Some(ix), cx);
        }
    }

    fn current_part(&self) -> Option<usize> {
        self.find.as_ref()?.1.map(|(part, _)| part)
    }

    /// The find's matches washed in the part at `ix` (its rendered text), the current one apart.
    fn mark(&self, ix: usize, reveal: bool, cx: &mut App) {
        let Some((Built::Text { view, .. }, _)) = self.built.get(&ix) else {
            return;
        };
        let none = FindQuery::default();
        let (query, current) = match &self.find {
            Some((q, at)) => (q, at.filter(|(part, _)| *part == ix).map(|(_, k)| k)),
            None => (&none, None),
        };
        view.update(cx, |v, cx| {
            _ = v.find(query, cx);
            v.select_match(current, reveal, cx);
        });
    }

    /// The built text parts' find marks: (part, (current, count)).
    #[cfg(test)]
    pub fn marks(&self, cx: &App) -> Vec<(usize, (Option<usize>, usize))> {
        let mut out: Vec<_> = self
            .built
            .iter()
            .filter_map(|(ix, (b, _))| match b {
                Built::Text { view, .. } => Some((*ix, view.read(cx).matches())),
                Built::Embed(_) => None,
            })
            .collect();
        out.sort();
        out
    }

    /// Whether the part at `ix` starts with a heading: it keeps the heading's space above it.
    pub fn heading_at(&self, ix: usize) -> bool {
        matches!(
            self.parts.get(ix),
            Some(Part::Text {
                heading: Some(_),
                ..
            })
        )
    }

    /// The part a #Heading or #^block names (subheadings joined by #: the last counts).
    pub fn find(&self, anchor: &str) -> Option<usize> {
        let last = anchor.split('#').rfind(|s| !s.is_empty())?;
        if last.starts_with('^') {
            return self.parts.iter().position(
                |p| matches!(p, Part::Text { ids, .. } if ids.iter().any(|i| i == last)),
            );
        }
        let want = markup::heading_key(last);
        self.parts
            .iter()
            .position(|p| matches!(p, Part::Text { heading: Some(h), .. } if *h == want))
    }

    #[cfg(test)]
    pub fn built_count(&self) -> usize {
        self.built.len()
    }

    /// The embeds built, in the note's order.
    #[cfg(test)]
    pub fn embeds(&self) -> Vec<Entity<EmbedView>> {
        let mut built: Vec<_> = self.built.iter().collect();
        built.sort_by_key(|(ix, _)| **ix);
        built
            .into_iter()
            .filter_map(|(_, (b, _))| match b {
                Built::Embed(e) => Some(e.clone()),
                Built::Text { .. } => None,
            })
            .collect()
    }
}

fn props_of(parts: &[Part]) -> Vec<(String, properties::Value)> {
    match parts.first() {
        Some(Part::Text { front, .. }) if !front.is_empty() => properties::parse(front),
        _ => vec![],
    }
}

/// A note rendered in its tab.
pub struct NoteView {
    doc: Doc,
    /// The note's text, for the code view to start from.
    source: String,
    list: ListState,
    index: Option<Arc<LinkIndex>>,
    images: Entity<BoundedImageCache>,
    /// A #heading or #^block to show once the vault's files are read.
    pending: Option<String>,
    find: Option<Find>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
    _live: Live,
}

/// ⌘F's box, kept while closed so ⌘F opens on its query and match again; its matches as
/// (part, which in it), the one shown.
struct Find {
    bar: Entity<FindBar>,
    open: bool,
    hits: Vec<(usize, usize)>,
    current: usize,
    _sub: Subscription,
}

impl NoteView {
    pub(crate) fn new(
        workspace: WeakEntity<Workspace>,
        vault: String,
        path: String,
        source: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let note = NoteCx {
            trail: vec![format!("{path}#")],
            vault: vault.clone(),
            path,
            depth: 0,
            workspace,
            top: Some(cx.weak_entity()),
        };
        let doc = Doc::new(note, &source, true);
        let list = ListState::new(doc.len(), ListAlignment::Top, px(800.));
        let model = Vaults::global(cx);
        let index = model.update(cx, |m, cx| m.index(&vault, cx));
        let sub = cx.subscribe(
            &model,
            move |this: &mut Self, model, e: &VaultsEvent, cx| {
                if *e == VaultsEvent::IndexChanged(vault.clone()) {
                    this.index = model.update(cx, |m, cx| m.index(&vault, cx));
                    if let Some(index) = this.index.clone() {
                        this.doc.reindex(&index, cx);
                    }
                    if let Some(anchor) = this.pending.take() {
                        this.reveal(&anchor, cx);
                    }
                    cx.notify();
                }
            },
        );
        NoteView {
            doc,
            source,
            list,
            index,
            images: BoundedImageCache::new(BoundedImageCache::display_side(window), cx),
            pending: None,
            find: None,
            focus: cx.focus_handle(),
            _subs: vec![sub],
            _live: Live::new("NoteView"),
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The note changed on disk (or was saved from the code view).
    pub fn set_source(&mut self, source: String, cx: &mut Context<Self>) {
        if source == self.source {
            return;
        }
        let (start, old, new) = self.doc.replace(&source, self.index.as_deref(), cx);
        self.source = source;
        if old > 0 || new > 0 {
            self.list.splice(start..start + old, new);
        }
        self.search(false, cx);
        self.mark(false, cx);
        cx.notify();
    }

    /// ⌘F: the find box, focused with its query selected.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let f = self.find.get_or_insert_with(|| {
            let bar = cx.new(|cx| FindBar::new(window, cx));
            let sub = cx.subscribe_in(
                &bar,
                window,
                |this, _, e: &FindBarEvent, window, cx| match e {
                    FindBarEvent::Changed => {
                        this.search(true, cx);
                        this.mark(true, cx);
                    }
                    FindBarEvent::Step(dir) => this.step(*dir, cx),
                    FindBarEvent::Close => this.close_find(window, cx),
                },
            );
            Find {
                bar,
                open: false,
                hits: vec![],
                current: 0,
                _sub: sub,
            }
        });
        let bar = f.bar.clone();
        if !std::mem::replace(&mut f.open, true) {
            self.search(false, cx);
            self.mark(true, cx);
        }
        bar.update(cx, |b, cx| b.focus(window, cx));
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut().filter(|f| f.open) else {
            return;
        };
        f.open = false;
        f.hits = vec![];
        self.doc.set_find(None, false, cx);
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// The find box, while it's open.
    fn open_box(&mut self) -> Option<&mut Find> {
        self.find.as_mut().filter(|f| f.open)
    }

    /// Looks for the query again (it changed, or the note did); `first` goes to the first match,
    /// else the current one stays where it can.
    fn search(&mut self, first: bool, cx: &mut Context<Self>) {
        let doc = &self.doc;
        let Some(f) = self.find.as_mut().filter(|f| f.open) else {
            return;
        };
        let error = match doc.search(f.bar.read(cx).query()) {
            Ok(hits) => {
                f.hits = hits;
                None
            }
            Err(e) => {
                f.hits = vec![];
                Some(e)
            }
        };
        let n = f.hits.len();
        f.current = if first {
            0
        } else {
            f.current.min(n.saturating_sub(1))
        };
        let found = error.map_or(Ok(((n > 0).then_some(f.current), n)), Err);
        f.bar.update(cx, |b, cx| b.set_found(found, cx));
        cx.notify();
    }

    /// The next (1) or the one before (-1), round.
    fn step(&mut self, dir: isize, cx: &mut Context<Self>) {
        let Some(f) = self.open_box() else { return };
        if f.hits.is_empty() {
            return;
        }
        let n = f.hits.len();
        f.current = (f.current as isize + dir).rem_euclid(n as isize) as usize;
        let found = Ok((Some(f.current), n));
        f.bar.update(cx, |b, cx| b.set_found(found, cx));
        self.mark(true, cx);
        cx.notify();
    }

    /// ⌘G / ⇧⌘G: a step while the find box is open; else the key goes on (⇧⌘G to Git).
    fn find_key(&mut self, dir: isize, cx: &mut Context<Self>) {
        if self.open_box().is_some() {
            self.step(dir, cx);
        } else {
            cx.propagate();
        }
    }

    /// The matches marked in the note; `reveal` scrolls the current one into view.
    fn mark(&mut self, reveal: bool, cx: &mut Context<Self>) {
        let Some(f) = self.open_box() else { return };
        let (query, current) = (
            f.bar.read(cx).query().clone(),
            f.hits.get(f.current).copied(),
        );
        if reveal && let Some((part, _)) = current {
            // The part first; its view then scrolls the list to the match once it's laid out.
            self.list.scroll_to_reveal_item(part);
        }
        self.doc.set_find(Some((query, current)), reveal, cx);
    }

    #[cfg(test)]
    pub(crate) fn find_hits(&self) -> Option<(usize, usize)> {
        let f = self.find.as_ref().filter(|f| f.open)?;
        Some((f.current, f.hits.len()))
    }

    /// Scrolls to the part a #heading or #^block names.
    pub fn reveal(&mut self, anchor: &str, cx: &mut Context<Self>) {
        if self.index.is_none() {
            self.pending = Some(anchor.to_string());
            return;
        }
        if let Some(ix) = self.doc.find(anchor) {
            self.list.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
            cx.notify();
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    #[cfg(test)]
    pub(crate) fn doc(&self) -> &Doc {
        &self.doc
    }

    fn render_part(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(index) = self.index.clone() else {
            return div().into_any_element();
        };
        let last = ix + 1 == self.doc.len();
        let part = self.doc.element(ix, &index, window, cx);
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(860. + 64.))
                    .px_8()
                    .when(ix == 0, |d| d.pt_4())
                    // Each part's markdown drops its first block's margin; a heading keeps it.
                    .when(ix > 0 && self.doc.heading_at(ix), |d| d.pt_4())
                    .when(last, |d| d.pb_8())
                    .child(part),
            )
            .into_any_element()
    }
}

impl Render for NoteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.doc.begin_frame();
        let empty = self.doc.len() == 0;
        let find = self.find.as_ref().filter(|f| f.open).map(|f| f.bar.clone());
        let t = cx.app_theme();
        div()
            .id("vault-note")
            .key_context("MarkdownView")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .on_action(cx.listener(|this, _: &editor::Find, window, cx| this.open_find(window, cx)))
            .on_action(cx.listener(|this, _: &find::Next, _, cx| this.find_key(1, cx)))
            .on_action(cx.listener(|this, _: &find::Prev, _, cx| this.find_key(-1, cx)))
            .bg(t.code(t.background))
            .when(empty, |d| {
                d.flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.5))
                    .text_color(t.muted_foreground)
                    .child("This note is empty")
            })
            .when(!empty, |d| {
                d.child(
                    image_cache(self.images.clone()).size_full().child(
                        list(
                            self.list.clone(),
                            cx.processor(|this, ix: usize, window, cx| {
                                this.render_part(ix, window, cx)
                            }),
                        )
                        .size_full(),
                    ),
                )
            })
            .children(find.map(|bar| div().absolute().top_2().right_4().child(bar)))
    }
}

/// What an embed shows.
enum Shown {
    /// The vault's files are being read.
    Waiting,
    Missing(String),
    /// Too deep, embedding itself, or a file with nothing to show in place.
    Link {
        file: String,
        anchor: String,
    },
    Note {
        file: String,
        anchor: String,
        doc: Option<Box<Doc>>,
        problem: Option<SharedString>,
    },
    Image {
        file: String,
        size: Option<(u32, Option<u32>)>,
    },
    Media {
        file: String,
        kind: MediaKind,
        /// The file's size and last change as its view was made: an overwrite makes it again.
        rev: Rev,
        /// The player or PDF viewer (native on macOS), made as it's first drawn.
        view: Option<Entity<MediaView>>,
    },
}

type Rev = Option<(u64, SystemTime)>;

fn rev_of(path: &Path) -> Rev {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// ![[embed]]: a note (or one of its parts) shown in place, an image, a player's card; anything
/// else as a link. `bare`: a canvas's file card, which is the file itself, not a quote of it.
pub(crate) struct EmbedView {
    note: NoteCx,
    target: String,
    alias: String,
    bare: bool,
    shown: Shown,
    _load: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl EmbedView {
    pub fn new(
        note: NoteCx,
        target: String,
        alias: String,
        bare: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let model = Vaults::global(cx);
        let vault = note.vault.clone();
        let sub = cx.subscribe(
            &model,
            move |this: &mut Self, _, e: &VaultsEvent, cx| match e {
                VaultsEvent::IndexChanged(v) if *v == vault => this.resolve(cx),
                e => match &this.shown {
                    Shown::Note { file, .. } | Shown::Image { file, .. }
                        if e.touches(&vault, file) =>
                    {
                        this.resolve(cx)
                    }
                    Shown::Media { file, kind, .. } if e.touches(&vault, file) => {
                        let (file, kind) = (file.clone(), *kind);
                        this.show_media(file, kind, cx);
                    }
                    _ => {}
                },
            },
        );
        let mut this = EmbedView {
            note,
            target,
            alias,
            bare,
            shown: Shown::Waiting,
            _load: Task::ready(()),
            _subs: vec![sub],
            _live: Live::new("EmbedView"),
        };
        this.resolve(cx);
        this
    }

    /// Finds the file again and reads it.
    fn resolve(&mut self, cx: &mut Context<Self>) {
        let vault = self.note.vault.clone();
        let Some(index) = Vaults::global(cx).update(cx, |m, cx| m.index(&vault, cx)) else {
            return;
        };
        let (path, anchor) = split_target(&self.target);
        let (path, anchor) = (path.to_string(), anchor.to_string());
        let file = if path.is_empty() {
            Some(self.note.path.clone())
        } else {
            index.resolve(&path, &self.note.path)
        };
        // The same file found again (another file came or went): a playing video plays on.
        if matches!((&self.shown, &file), (Shown::Media { file: f, .. }, Some(file)) if f == file) {
            return;
        }
        // A read still on its way is for what was shown before.
        self._load = Task::ready(());
        let Some(file) = file else {
            self.shown = Shown::Missing(path);
            return cx.notify();
        };
        if gitviber_markdown::is_markdown(&file) {
            // The same note and part again, inside itself: a link, or it would go on forever.
            let again = self.note.trail.contains(&format!("{file}#{anchor}"));
            if self.note.depth >= MAX_DEPTH || again {
                self.shown = Shown::Link { file, anchor };
                return cx.notify();
            }
            self.load(file, anchor, cx);
            return;
        }
        self.shown = match media_kind(&file) {
            Some(MediaKind::Image) => Shown::Image {
                file,
                size: sized(&self.alias),
            },
            Some(kind) => return self.show_media(file, kind, cx),
            None => Shown::Link { file, anchor },
        };
        cx.notify();
    }

    /// A player or a PDF, made again only once its own file was overwritten: a change elsewhere
    /// in the vault (or none, in a vault read again whole) leaves a playing video playing.
    fn show_media(&mut self, file: String, kind: MediaKind, cx: &mut Context<Self>) {
        let abs = self.abs(&file);
        self._load = cx.spawn(async move |this, cx| {
            let now = cx.background_spawn(async move { rev_of(&abs) }).await;
            _ = this.update(cx, |this, cx| {
                if let Shown::Media {
                    file: f, rev, view, ..
                } = &mut this.shown
                    && *f == file
                {
                    if *rev == now {
                        return;
                    }
                    *rev = now;
                    *view = None;
                } else {
                    this.shown = Shown::Media {
                        file,
                        kind,
                        rev: now,
                        view: None,
                    };
                }
                cx.notify();
            });
        });
    }

    fn load(&mut self, file: String, anchor: String, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        let (vault, f) = (self.note.vault.clone(), file.clone());
        self._load = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { api::obsidian::vault_read_file(&core, vault, f) })
                .await;
            _ = this.update(cx, |this, cx| {
                let problem: Option<SharedString> = match &read {
                    Err(e) => Some(e.clone().into()),
                    Ok(t) if !t.exists || t.binary => {
                        Some(format!("“{}” isn't in this vault", note_name(&file)).into())
                    }
                    Ok(t) if t.too_large || t.text.len() > MAX_EMBED_BYTES => {
                        Some(format!("“{}” is too large to show here", note_name(&file)).into())
                    }
                    Ok(_) => None,
                };
                let doc = match (&problem, read) {
                    (None, Ok(t)) => Some(Box::new(match markup::section(&t.text, &anchor) {
                        Ok(text) => Doc::new(this.note.inner(&file, &anchor), &text, false),
                        Err(missing) => Doc::new(
                            this.note.inner(&file, &anchor),
                            &format!("*{missing}*"),
                            false,
                        ),
                    })),
                    _ => None,
                };
                this.shown = Shown::Note {
                    file,
                    anchor,
                    doc,
                    problem,
                };
                cx.notify();
            });
        });
    }

    fn label(&self, file: &str, anchor: &str) -> String {
        if !self.alias.is_empty() && sized(&self.alias).is_none() {
            return self.alias.clone();
        }
        std::iter::once(note_name(file))
            .chain(anchor.split('#').filter(|s| !s.is_empty()))
            .collect::<Vec<_>>()
            .join(" > ")
    }

    fn link(
        &self,
        id: &'static str,
        file: &str,
        anchor: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = cx.app_theme();
        let note = self.note.clone();
        let (file, anchor) = (file.to_string(), anchor.to_string());
        div()
            .id(id)
            .cursor_pointer()
            .text_color(t.primary)
            .hover(|s| s.underline())
            .child(self.label(&file, &anchor))
            .on_click(move |_, window, cx| {
                open_in_tab(
                    &note.workspace,
                    &note.vault,
                    &file,
                    &anchor,
                    false,
                    window,
                    cx,
                )
            })
    }

    fn abs(&self, file: &str) -> PathBuf {
        PathBuf::from(&self.note.vault).join(file)
    }

    #[cfg(test)]
    pub(crate) fn media(&self) -> Option<Entity<MediaView>> {
        match &self.shown {
            Shown::Media { view, .. } => view.clone(),
            _ => None,
        }
    }
}

/// An embed's alias as a size: "300" or "300x200".
fn sized(alias: &str) -> Option<(u32, Option<u32>)> {
    let size = alias.rsplit('|').next()?.trim();
    let (w, h) = match size.split_once('x') {
        Some((w, h)) => (w, Some(h)),
        None => (size, None),
    };
    let w = w.parse().ok()?;
    match h {
        Some(h) => Some((w, Some(h.parse().ok()?))),
        None => Some((w, None)),
    }
}

impl Render for EmbedView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Shown::Media {
            file,
            kind,
            view: view @ None,
            ..
        } = &mut self.shown
        {
            let (abs, kind) = (PathBuf::from(&self.note.vault).join(&*file), *kind);
            *view = Some(cx.new(|cx| MediaView::new(abs, kind, window, cx)));
        }
        let t = cx.app_theme().clone();
        let quiet = |text: SharedString| {
            div()
                .text_size(px(12.))
                .text_color(t.subtle)
                .italic()
                .child(text)
        };
        let body: AnyElement = match &self.shown {
            Shown::Waiting => div().into_any_element(),
            Shown::Missing(name) => {
                quiet(format!("“{name}” isn't in this vault").into()).into_any_element()
            }
            Shown::Link { file, anchor } => {
                let (file, anchor) = (file.clone(), anchor.clone());
                self.link("embed-link", &file, &anchor, cx)
                    .into_any_element()
            }
            Shown::Image { file, size } => {
                let image = img(self.abs(file)).object_fit(ObjectFit::Contain);
                let image = match (size, self.bare) {
                    (_, true) => image.size_full(),
                    (Some((w, Some(h))), _) => image.w(px(*w as f32)).h(px(*h as f32)),
                    (Some((w, None)), _) => image.w(px(*w as f32)),
                    (None, _) => image.max_w_full(),
                };
                div()
                    .when(self.bare, |d| d.size_full())
                    .child(image)
                    .into_any_element()
            }
            Shown::Media { kind, view, .. } => {
                let size = sized(&self.alias);
                let frame = div()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(t.border);
                let frame = match (self.bare, kind, size) {
                    (true, _, _) => frame.size_full(),
                    (_, MediaKind::Video, Some((w, h))) => frame
                        .w(px(w as f32))
                        .h(px(h.map_or(w as f32 * 9. / 16., |h| h as f32))),
                    (_, MediaKind::Pdf, _) => frame.w_full().h(px(PDF_HEIGHT)),
                    (_, MediaKind::Audio, _) => frame.w_full().h(px(AUDIO_HEIGHT)),
                    _ => frame.w_full().h(px(VIDEO_HEIGHT)),
                };
                frame.children(view.clone()).into_any_element()
            }
            Shown::Note {
                problem: Some(p), ..
            } => quiet(p.clone()).into_any_element(),
            Shown::Note { doc: None, .. } => div().into_any_element(),
            Shown::Note { .. } => {
                let index = Vaults::global(cx).update(cx, |m, cx| m.index(&self.note.vault, cx));
                let Shown::Note { doc: Some(doc), .. } = &mut self.shown else {
                    unreachable!()
                };
                doc.begin_frame();
                let parts: Vec<AnyElement> = match index {
                    Some(index) => (0..doc.len())
                        .map(|ix| doc.element(ix, &index, window, cx))
                        .collect(),
                    None => vec![],
                };
                div().flex().flex_col().children(parts).into_any_element()
            }
        };
        if self.bare {
            return div().size_full().child(body);
        }
        let title = match &self.shown {
            Shown::Note { file, anchor, .. } => {
                let (file, anchor) = (file.clone(), anchor.clone());
                Some(self.link("embed-title", &file, &anchor, cx))
            }
            _ => None,
        };
        let boxed = matches!(self.shown, Shown::Note { .. });
        div()
            .my_2()
            .when(boxed, |d| {
                d.pl_4().py_1().border_l_2().border_color(t.primary)
            })
            .flex()
            .flex_col()
            .gap_1()
            .children(title.map(|l| div().text_size(px(12.)).child(l)))
            .child(body)
    }
}
