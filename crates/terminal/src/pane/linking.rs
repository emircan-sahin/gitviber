//! The pane's links: ⌘-hover (Ctrl off macOS) underlines what ⌘-click opens, read from the line
//! under the pointer; ⌃⇧Space puts a label on every link on screen, typed to open it.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash as _, Hasher as _};
use std::rc::Rc;

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use gitviber_ui::workspace::Toast;
use gpui::{
    AppContext as _, ClipboardItem, Context, KeyDownEvent, Modifiers, Pixels, Point, Window,
};

use super::{PaneEvent, TerminalPane};
use crate::hints::{HintKey, Hints};
use crate::linkhost::{self, LinkSite, SharedCache};
use crate::links::{
    CONTEXT_LINES, CellRow, Link, LinkKind, RowSource, Target, context_dirs, dirs_above,
    find_terminal_links, hyperlink_target, is_local_url, line_dirs, logical_line, named_dirs,
    split_position,
};
use crate::marks::is_mark_char;

/// What links resolve against, the panel's for all its panes.
pub(crate) struct LinkCtx {
    pub site: LinkSite,
    pub cache: SharedCache,
}

/// A link on screen: its first and last cell as (grid line, column), and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShownLink {
    pub start: (i32, usize),
    pub end: (i32, usize),
    pub text: String,
    pub target: Target,
    /// An OSC 8 link's address, which its text needn't be.
    pub uri: Option<String>,
}

impl ShownLink {
    pub(crate) fn covers(&self, line: i32, col: usize) -> bool {
        (line, col) >= self.start && (line, col) <= self.end
    }

    /// Its cells a row at a time: (line, from, to), `to` past the last.
    pub fn spans(&self, cols: usize) -> impl Iterator<Item = (i32, usize, usize)> + '_ {
        let (s, e) = (self.start, self.end);
        (s.0..=e.0).map(move |l| {
            (
                l,
                if l == s.0 { s.1 } else { 0 },
                if l == e.0 { e.1 + 1 } else { cols },
            )
        })
    }

    /// What ⌘-click does, for the hover.
    pub fn hint(&self) -> String {
        let does = match &self.target {
            Target::External { dir: true, .. } => "Show in the file manager".to_string(),
            Target::External { .. } => "Open file".into(),
            Target::Commit(_) => "Show in History".into(),
            Target::Item { number, .. } => format!("Open #{number}"),
            Target::Url(u) if is_local_url(u) => "Open in a browser tab".into(),
            Target::Url(_) => "Follow link".into(),
            Target::Path { dir: true, .. } => "Show folder".into(),
            Target::Path { .. } => "Open file".into(),
        };
        let click = if cfg!(target_os = "macos") {
            "⌘-click"
        } else {
            "Ctrl-click"
        };
        match &self.uri {
            Some(uri) => format!("{uri}\n{does} ({click})"),
            None => format!("{does} ({click})"),
        }
    }
}

/// The links of the line under the pointer, while that line reads the same.
pub(crate) struct Hover {
    key: u64,
    pub links: Vec<ShownLink>,
    /// The one under the pointer.
    pub at: Option<usize>,
}

/// The labels on screen, for the screen they were found on.
pub(crate) struct HintsShown {
    pub hints: Hints,
    pub links: Vec<ShownLink>,
    offset: usize,
    scrolled: u64,
    alt: bool,
}

/// The modifier that makes text a link: ⌘ on macOS, Ctrl elsewhere.
pub(crate) fn primary(m: &Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        m.platform
    } else {
        m.control
    }
}

/// A grid row's cells as links read them, the shell integration's marks left out.
fn cell_row<T>(term: &Term<T>, line: i32) -> Option<CellRow> {
    if line < term.topmost_line().0 || line > term.bottommost_line().0 {
        return None;
    }
    let row = &term.grid()[Line(line)];
    let cols = term.columns();
    let mut cells = Vec::with_capacity(cols);
    for x in 0..cols {
        let cell = &row[Column(x)];
        let width = if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            0
        } else if cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let mut text = String::new();
        text.push(if cell.c == '\t' { ' ' } else { cell.c });
        text.extend(
            cell.zerowidth()
                .unwrap_or_default()
                .iter()
                .filter(|c| !is_mark_char(**c)),
        );
        cells.push((text, width));
    }
    let wraps = cols > 0 && row[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
    Some(CellRow { cells, wraps })
}

/// The terminal's rows, each read once per look.
struct TermRows<'a, T> {
    term: &'a Term<T>,
    read: HashMap<i32, Option<Rc<CellRow>>>,
}

impl<'a, T> TermRows<'a, T> {
    fn new(term: &'a Term<T>) -> Self {
        TermRows {
            term,
            read: HashMap::new(),
        }
    }
}

impl<T> RowSource for TermRows<'_, T> {
    fn cols(&self) -> usize {
        self.term.columns()
    }
    fn top(&self) -> i32 {
        self.term.topmost_line().0
    }
    fn row(&mut self, line: i32) -> Option<Rc<CellRow>> {
        let term = self.term;
        self.read
            .entry(line)
            .or_insert_with(|| cell_row(term, line).map(Rc::new))
            .clone()
    }
}

fn bare_name(l: &Link) -> bool {
    l.kind == LinkKind::File && !split_position(&l.spec).0.contains(['/', '\\'])
}

impl TerminalPane {
    pub(crate) fn set_links(&mut self, ctx: Option<Rc<LinkCtx>>, cx: &mut Context<Self>) {
        let same_place = match (&self.links, &ctx) {
            (Some(a), Some(b)) => {
                (&a.site.root, &a.site.worktrees, &a.site.repos)
                    == (&b.site.root, &b.site.worktrees, &b.site.repos)
            }
            _ => false,
        };
        self.links = ctx;
        // Only the working tree moved on (an agent writing files): what's shown and what's
        // being asked stand, and the next text under the pointer asks anew.
        if same_place {
            return;
        }
        self.clear_hover(cx);
        self.rehover(cx);
    }

    /// The pointer's line again, while ⌘ is held: the view scrolled, the screen changed.
    pub(crate) fn rehover(&mut self, cx: &mut Context<Self>) {
        if self.link_primary || self.hover.is_some() || self.hover_ask.is_some() {
            self.hover_links(self.link_pos, self.link_primary, cx);
        }
    }

    /// The window lost the keys: ⌘ goes up unseen (⌘-Tab), so the underline goes now.
    pub(crate) fn links_window_left(&mut self, cx: &mut Context<Self>) {
        self.link_primary = false;
        self.clear_hover(cx);
    }

    pub(crate) fn clear_hover(&mut self, cx: &mut Context<Self>) {
        self.link_task = None;
        self.hover_ask = None;
        if self.hover.take().is_some() {
            cx.notify();
        }
    }

    /// The pointer at `pos`, `primary` held or not: the links of the line under it, asked of the
    /// repo once per text that line shows.
    pub(crate) fn hover_links(
        &mut self,
        pos: Option<Point<Pixels>>,
        primary: bool,
        cx: &mut Context<Self>,
    ) {
        self.link_pos = pos;
        self.link_primary = primary;
        let cell = pos.and_then(|p| self.cell_at(p)).filter(|c| c.3);
        let (Some((col, row, ..)), true, Some(ctx)) = (cell, primary, self.links.clone()) else {
            return self.clear_hover(cx);
        };
        let emu = self.session.emu.lock();
        let line = row as i32 - emu.term.grid().display_offset() as i32;
        let mut rows = TermRows::new(&emu.term);
        let text = logical_line(&mut rows, line);
        let grid_row = &emu.term.grid()[Line(line)];
        let hyperlink = grid_row[Column(col)].hyperlink();
        // An OSC 8 link's text needn't be where it goes: each one under the pointer is its own.
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (text.first, emu.scrolled, &text.text).hash(&mut h);
        hyperlink.as_ref().map(|l| (l.id(), l.uri())).hash(&mut h);
        let key = h.finish();
        if let Some(hover) = self.hover.as_mut().filter(|h| h.key == key) {
            let at = hover.links.iter().position(|l| l.covers(line, col));
            if at != hover.at {
                hover.at = at;
                cx.notify();
            }
            return;
        }
        if self.hover_ask == Some(key) {
            return;
        }
        if self.hover.take().is_some() {
            cx.notify();
        }
        self.hover_ask = Some(key);
        let (site, cache, core) = (
            ctx.site.clone(),
            ctx.cache.clone(),
            self.session.core().clone(),
        );
        // An OSC 8 hyperlink: its own cells along the row, and where it says it goes.
        if let Some(link) = hyperlink {
            let cols = emu.term.columns();
            let same = |x: usize| grid_row[Column(x)].hyperlink().as_ref() == Some(&link);
            let (mut from, mut to) = (col, col);
            while from > 0 && same(from - 1) {
                from -= 1;
            }
            while to + 1 < cols && same(to + 1) {
                to += 1;
            }
            drop(emu);
            let uri = link.uri().to_string();
            let Some(target) = hyperlink_target(&uri, &site.root) else {
                return self.clear_hover(cx);
            };
            self.link_task = Some(cx.spawn(async move |this, cx| {
                let target = cx
                    .background_spawn(async move {
                        linkhost::hyperlink_file(&core, &cache, &site, target)
                    })
                    .await;
                let _ = this.update(cx, |pane, cx| {
                    if pane.hover_ask != Some(key) {
                        return;
                    }
                    pane.hover_ask = None;
                    let links: Vec<ShownLink> = target
                        .into_iter()
                        .map(|target| ShownLink {
                            start: (line, from),
                            end: (line, to),
                            text: uri.clone(),
                            target,
                            uri: Some(uri.clone()),
                        })
                        .collect();
                    let at = (!links.is_empty()).then_some(0);
                    pane.hover = Some(Hover { key, links, at });
                    cx.notify();
                });
            }));
            return;
        }
        let r = (line - text.first) as usize;
        let near = (text.starts[r], text.starts[r + 1]);
        let found: Vec<Link> = find_terminal_links(&text.text, Some(near))
            .into_iter()
            .filter(|l| {
                let (s, e) = text.range(l);
                s.0 <= line && e.0 >= line && text.whole(l)
            })
            .collect();
        if found.is_empty() {
            drop(emu);
            self.hover = Some(Hover {
                key,
                links: vec![],
                at: None,
            });
            self.hover_ask = None;
            return;
        }
        let dirs = if found.iter().any(bare_name) {
            context_dirs(&mut rows, line)
        } else {
            vec![]
        };
        drop(emu);
        let dirs = vec![dirs; found.len()];
        let cwd = self.dir.to_string_lossy().into_owned();
        self.link_task = Some(cx.spawn(async move |this, cx| {
            let resolved = cx
                .background_spawn(async move {
                    linkhost::resolve_links(&core, &cache, &site, &cwd, &found, &dirs)
                })
                .await;
            let _ = this.update(cx, |pane, cx| {
                if pane.hover_ask != Some(key) {
                    return;
                }
                pane.hover_ask = None;
                let links: Vec<ShownLink> = resolved
                    .into_iter()
                    .map(|(_, l, target)| {
                        let (start, end) = text.range(&l);
                        ShownLink {
                            start,
                            end,
                            text: l.spec,
                            target,
                            uri: None,
                        }
                    })
                    .collect();
                let at = links.iter().position(|l| l.covers(line, col));
                pane.hover = Some(Hover { key, links, at });
                cx.notify();
            });
        }));
    }

    /// The hovered link, which a primary click opens.
    pub(crate) fn hovered_link(&self) -> Option<&ShownLink> {
        let h = self.hover.as_ref()?;
        h.links.get(h.at?)
    }

    /// New output: the line under a held ⌘ may read otherwise now; the labels go with a screen
    /// that scrolled or switched.
    pub(crate) fn links_after_output(&mut self, cx: &mut Context<Self>) {
        if let Some(h) = &self.hints {
            let emu = self.session.emu.lock();
            let alt = emu
                .mode()
                .contains(alacritty_terminal::term::TermMode::ALT_SCREEN);
            let moved = emu.scrolled != h.scrolled
                || emu.term.grid().display_offset() != h.offset
                || alt != h.alt;
            drop(emu);
            if moved {
                self.end_hints(cx);
            }
        }
        self.rehover(cx);
    }

    pub(crate) fn end_hints(&mut self, cx: &mut Context<Self>) {
        self.hint_task = None;
        if self.hints.take().is_some() {
            cx.notify();
        }
    }

    /// ⌃⇧Space: a label on every link on screen, or none when they show.
    pub fn toggle_hints(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.hints.is_some() {
            return self.end_hints(cx);
        }
        let Some(ctx) = self.links.clone() else {
            return;
        };
        if self.hint_task.is_some() {
            return;
        }
        let emu = self.session.emu.lock();
        let offset = emu.term.grid().display_offset();
        let scrolled = emu.scrolled;
        let alt = emu
            .mode()
            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN);
        let top = -(offset as i32);
        let bottom = top + emu.term.screen_lines() as i32 - 1;
        let mut rows = TermRows::new(&emu.term);
        // The folders the lines above the screen name, oldest first; each screen line's are
        // added on the way down, so a bare name's folder is found in one pass.
        let mut seen: VecDeque<Vec<String>> = VecDeque::new();
        let mut r = logical_line(&mut rows, top).first - 1;
        while r >= rows.top() && seen.len() < CONTEXT_LINES {
            let line = logical_line(&mut rows, r);
            seen.push_front(line_dirs(&line.text));
            r = line.first - 1;
        }
        let mut found = vec![];
        let mut dirs = vec![];
        let mut lines = vec![];
        let mut of = vec![];
        let mut y = top;
        while y <= bottom {
            let line = logical_line(&mut rows, y);
            let links = find_terminal_links(&line.text, None);
            let above = if links.iter().any(bare_name) {
                dirs_above(&seen)
            } else {
                vec![]
            };
            for l in &links {
                // Labelled at its first cell, which must be on screen.
                if line.whole(l) && (top..=bottom).contains(&line.range(l).0.0) {
                    of.push(lines.len());
                    dirs.push(if bare_name(l) { above.clone() } else { vec![] });
                    found.push(l.clone());
                }
            }
            let mut named = vec![];
            named_dirs(&links, &mut named);
            seen.push_back(named);
            if seen.len() > CONTEXT_LINES {
                seen.pop_front();
            }
            y = line.last.max(y) + 1;
            lines.push(line);
        }
        drop(emu);
        let (site, cache, core) = (
            ctx.site.clone(),
            ctx.cache.clone(),
            self.session.core().clone(),
        );
        let cwd = self.dir.to_string_lossy().into_owned();
        self.hint_task = Some(cx.spawn_in(window, async move |this, cx| {
            let resolved = cx
                .background_spawn(async move {
                    linkhost::resolve_links(&core, &cache, &site, &cwd, &found, &dirs)
                })
                .await;
            let _ = this.update_in(cx, |pane, window, cx| {
                pane.hint_task = None;
                let emu = pane.session.emu.lock();
                // Scrolled while the repo was asked: what it said is of another screen.
                let same = emu.scrolled == scrolled && emu.term.grid().display_offset() == offset;
                drop(emu);
                if !same {
                    return;
                }
                let links: Vec<ShownLink> = resolved
                    .into_iter()
                    .map(|(i, l, target)| {
                        let (start, end) = lines[of[i]].range(&l);
                        ShownLink {
                            start,
                            end,
                            text: l.spec,
                            target,
                            uri: None,
                        }
                    })
                    .collect();
                if links.is_empty() {
                    Toast::info(
                        "Hints go on the URLs, files, commits and #123 the terminal shows.",
                    )
                    .title("No links on screen")
                    .show(window, cx);
                    return;
                }
                pane.hints = Some(HintsShown {
                    hints: Hints::new(links.len()),
                    links,
                    offset,
                    scrolled,
                    alt,
                });
                pane.focus(window, cx);
                cx.notify();
            });
        }));
    }

    /// A key while hints show: letters type toward a label, Backspace takes one back, Esc ends
    /// them, all kept from the program. A key with ⌘, ⌃ or ⌥ ends them and goes on as usual.
    /// False when it isn't the hints'.
    pub(crate) fn hint_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(shown) = self.hints.as_mut() else {
            return false;
        };
        let m = e.keystroke.modifiers;
        if m.platform || m.control || m.alt {
            self.end_hints(cx);
            return false;
        }
        cx.stop_propagation();
        match shown.hints.key(&e.keystroke.key, m.shift) {
            HintKey::Chosen { index, copy } => {
                let link = shown.links[index].clone();
                self.end_hints(cx);
                if copy {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.text.clone()));
                    cx.emit(PaneEvent::Copied(link.text));
                } else {
                    cx.emit(PaneEvent::OpenLink(link.target));
                }
            }
            HintKey::End => self.end_hints(cx),
            HintKey::Narrowed => cx.notify(),
            HintKey::Ignored => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_opens_the_link_only_on_its_own_cells() {
        // Wrapped from column 4 of one row to column 6 of the next.
        let link = ShownLink {
            start: (2, 4),
            end: (3, 6),
            text: "src/a.rs".into(),
            target: Target::Url("https://example.com".into()),
            uri: None,
        };
        for (line, col) in [(2, 4), (2, 79), (3, 0), (3, 6)] {
            assert!(link.covers(line, col), "{line}:{col}");
        }
        for (line, col) in [(2, 3), (3, 7), (1, 10), (4, 0)] {
            assert!(!link.covers(line, col), "{line}:{col}");
        }
        assert_eq!(link.spans(80).collect::<Vec<_>>(), [(2, 4, 80), (3, 0, 7)]);
    }
}
