//! The code view's map on its scrollbar track: the whole file's change bars (left lane) and
//! find matches (right lane, beside the thumb rather than under it). Worked out from the bar
//! list, the matches and the display map, never from the laid-out lines, so it's right before
//! any scroll; once per change, not per frame.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gitviber_ui::ActiveAppTheme as _;
use gpui_kit::component::input::{
    BufferPoint, DisplayMap, DisplayPoint, EditorState, Rope, RopeExt as _,
};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::{App, Bounds, Entity, Pixels, Size, Window, fill, point, px, size};

use crate::text::{Bar, BarKind};

/// A one-line mark is drawn this tall, so a single change in a long file still shows.
const MIN_MARK: f32 = 3.;
const LANE: f32 = 3.;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MarkKind {
    Added,
    Modified,
    Deleted,
    Found,
}

/// Track pixel rows `start..end` of one kind; marks of a kind that touch are one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    pub kind: MarkKind,
    pub start: u32,
    pub end: u32,
}

/// Where the text sits in the editor's display rows (soft-wrapped, folded).
pub(crate) trait Rows {
    fn row_count(&self) -> usize;
    /// The display rows of a 0-based buffer line; a folded one: its fold's row.
    fn line_rows(&self, line: usize) -> Range<usize>;
    /// The 0-based buffer line at a display row.
    fn row_line(&self, row: usize) -> usize;
    fn offset_row(&self, offset: usize) -> usize;
    /// The first byte offset of a display row.
    fn row_offset(&self, row: usize) -> usize;
}

pub(crate) struct EditorRows<'a> {
    pub map: &'a DisplayMap,
    pub text: &'a Rope,
}

impl Rows for EditorRows<'_> {
    fn row_count(&self) -> usize {
        self.map.display_row_count()
    }

    fn line_rows(&self, line: usize) -> Range<usize> {
        self.map
            .buffer_line_to_display_row_range(line)
            .unwrap_or_else(|| {
                let row = self.map.buffer_line_to_display_row(line);
                row..row + 1
            })
    }

    fn row_line(&self, row: usize) -> usize {
        self.map.display_row_to_buffer_line(row)
    }

    fn offset_row(&self, offset: usize) -> usize {
        let p = self.text.offset_to_point(offset);
        self.map
            .buffer_pos_to_display_pos(BufferPoint::new(p.row, p.column))
            .row
    }

    fn row_offset(&self, row: usize) -> usize {
        let p = self
            .map
            .display_pos_to_buffer_pos(DisplayPoint::new(row, 0));
        self.text.line_start_offset(p.line) + p.col
    }
}

/// Display rows to track pixel rows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Scale {
    /// Track pixels per display row.
    pub k: f64,
    /// The track's height in pixels.
    pub height: u32,
}

impl Scale {
    fn span(&self, rows: Range<usize>) -> (u32, u32) {
        let last = self.height.saturating_sub(1);
        let a = ((rows.start as f64 * self.k).floor() as u32).min(last);
        let b = ((rows.end as f64 * self.k).ceil() as u32)
            .max(a + 1)
            .min(self.height);
        (a, b)
    }

    /// The display row at track pixel `y`: every row before it ends at or above `y`.
    fn row_at(&self, y: u32) -> usize {
        (y as f64 / self.k).floor() as usize
    }
}

fn extend(out: &mut Vec<Mark>, run: &mut Option<Mark>, kind: MarkKind, (start, end): (u32, u32)) {
    match run {
        Some(m) if start <= m.end => m.end = m.end.max(end),
        _ => {
            out.extend(run.take());
            *run = Some(Mark { kind, start, end });
        }
    }
}

/// The marks of `bars` (sorted by line) and `found` (sorted byte ranges). Bars and matches inside
/// the run just drawn are skipped by a lookup instead of being placed one by one, so a file
/// changed on every one of 100k lines costs a few lookups per track pixel, not per line.
pub(crate) fn marks(
    bars: &[Bar],
    found: &[Range<usize>],
    text_len: usize,
    rows: &impl Rows,
    scale: Scale,
) -> Vec<Mark> {
    let mut out = Vec::new();
    let count = rows.row_count();
    if scale.height == 0 || count == 0 || scale.k <= 0. {
        return out;
    }
    let mut runs = [None; 3];
    // Per kind: bars on lines before this are inside that kind's run.
    let mut skip = [0usize; 3];
    for bar in bars {
        let (i, kind) = match bar.kind {
            BarKind::Added => (0, MarkKind::Added),
            BarKind::Modified => (1, MarkKind::Modified),
            BarKind::Deleted => (2, MarkKind::Deleted),
        };
        let line = bar.line.max(1) as usize - 1;
        if line < skip[i] {
            continue;
        }
        extend(
            &mut out,
            &mut runs[i],
            kind,
            scale.span(rows.line_rows(line)),
        );
        let row = scale.row_at(runs[i].map_or(0, |m| m.end));
        skip[i] = if row >= count {
            usize::MAX
        } else {
            rows.row_line(row)
        };
    }
    out.extend(runs.into_iter().flatten());
    let mut run = None;
    let mut i = 0;
    while i < found.len() {
        let row = rows.offset_row(found[i].start.min(text_len));
        extend(
            &mut out,
            &mut run,
            MarkKind::Found,
            scale.span(row..row + 1),
        );
        let row = scale.row_at(run.map_or(0, |m| m.end));
        if row >= count {
            break;
        }
        let bound = rows.row_offset(row);
        i = (i + 1).max(found.partition_point(|m| m.start < bound));
    }
    out.extend(run);
    out.sort_unstable_by_key(|m| (m.kind, m.start));
    out
}

/// What the marks were worked out from; any change works them out again.
struct Key {
    bars: Arc<Vec<Bar>>,
    found: Option<Rc<Vec<Range<usize>>>>,
    text_len: usize,
    rows: (usize, usize),
    line_height: Pixels,
    /// The text area: its height is the track's, its width the wrap's.
    area: Size<Pixels>,
}

impl PartialEq for Key {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.bars, &o.bars)
            && match (&self.found, &o.found) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && self.text_len == o.text_len
            && self.rows == o.rows
            && self.line_height == o.line_height
            && self.area == o.area
    }
}

#[derive(Default)]
pub(crate) struct CodeMap {
    key: Option<Key>,
    marks: Vec<Mark>,
}

impl CodeMap {
    #[cfg(test)]
    pub(crate) fn marks(&self) -> &[Mark] {
        &self.marks
    }
}

/// The editor's track: the right edge of its text area, as tall as it is.
pub(crate) fn track(state: &EditorState) -> Option<(Bounds<Pixels>, Scale)> {
    let line_height = state.line_height()?;
    let area = state.input_bounds();
    let width = Scrollbar::width();
    let height = f32::from(area.size.height);
    if height < 1. || line_height <= px(0.) {
        return None;
    }
    // The editor's scroll height: every wrapped row plus its empty bottom (half the view, at
    // least 3 rows), never less than the view. The thumb covers the same share.
    let rows = state.display_map().wrap_row_count() as f32;
    let lh = f32::from(line_height);
    let content = (rows * lh + (height / 2.).max(3. * lh)).max(height);
    let bounds = Bounds::new(
        point(area.right() - width, area.top()),
        size(width, area.size.height),
    );
    let scale = Scale {
        k: (height as f64) * (lh as f64) / content as f64,
        height: height as u32,
    };
    Some((bounds, scale))
}

pub(crate) fn paint(
    map: &RefCell<CodeMap>,
    state: &Entity<EditorState>,
    bars: &Arc<Vec<Bar>>,
    window: &mut Window,
    cx: &mut App,
) {
    let s = state.read(cx);
    let Some((track, scale)) = track(s) else {
        return;
    };
    let session = s.search_session();
    let key = Key {
        bars: bars.clone(),
        found: session
            .is_active()
            .then(|| session.matcher.matched_ranges()),
        text_len: s.text().len(),
        rows: (
            s.display_map().wrap_row_count(),
            s.display_map().display_row_count(),
        ),
        line_height: s.line_height().unwrap_or_default(),
        area: s.input_bounds().size,
    };
    let mut map = map.borrow_mut();
    if map.key.as_ref() != Some(&key) {
        let rows = EditorRows {
            map: s.display_map(),
            text: s.text(),
        };
        let found = key.found.as_deref().map_or(&[][..], |f| &f[..]);
        map.marks = marks(&key.bars, found, key.text_len, &rows, scale);
        map.key = Some(key);
    }
    let t = cx.app_theme();
    for m in &map.marks {
        let (color, x) = match m.kind {
            MarkKind::Added => (t.added, track.left() + px(1.)),
            MarkKind::Modified => (t.primary, track.left() + px(1.)),
            MarkKind::Deleted => (t.removed, track.left() + px(1.)),
            MarkKind::Found => (t.find_mark, track.right() - px(1. + LANE)),
        };
        let len = (m.end - m.start) as f32;
        let top = if len < MIN_MARK {
            (m.start as f32 + len / 2. - MIN_MARK / 2.)
                .clamp(0., (scale.height as f32 - MIN_MARK).max(0.))
        } else {
            m.start as f32
        };
        let quad = Bounds::new(
            point(x, track.top() + px(top)),
            size(px(LANE), px(len.max(MIN_MARK))),
        );
        window.paint_quad(fill(quad, color));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::Cell;

    use super::*;

    /// Lines of `len` bytes, each `wraps[i]` display rows tall, a row holding `len / wraps[i]`.
    struct Fake {
        starts: Vec<usize>,
        len: usize,
        lookups: Cell<usize>,
    }

    impl Fake {
        fn new(wraps: &[usize], len: usize) -> Self {
            let mut starts = vec![0];
            for w in wraps {
                starts.push(starts.last().unwrap() + w);
            }
            Fake {
                starts,
                len,
                lookups: Cell::new(0),
            }
        }
    }

    impl Rows for Fake {
        fn row_count(&self) -> usize {
            *self.starts.last().unwrap()
        }
        fn line_rows(&self, line: usize) -> Range<usize> {
            self.lookups.set(self.lookups.get() + 1);
            self.starts[line]..self.starts[line + 1]
        }
        fn row_line(&self, row: usize) -> usize {
            self.lookups.set(self.lookups.get() + 1);
            self.starts.partition_point(|&s| s <= row) - 1
        }
        fn offset_row(&self, offset: usize) -> usize {
            self.lookups.set(self.lookups.get() + 1);
            let line = offset / self.len;
            let wraps = self.starts[line + 1] - self.starts[line];
            self.starts[line] + (offset % self.len / (self.len / wraps)).min(wraps - 1)
        }
        fn row_offset(&self, row: usize) -> usize {
            let line = self.row_line(row);
            let wraps = self.starts[line + 1] - self.starts[line];
            line * self.len + (row - self.starts[line]) * (self.len / wraps)
        }
    }

    /// Each bar and match painted on its own, then touching pixels joined.
    pub(crate) fn naive(
        bars: &[Bar],
        found: &[Range<usize>],
        rows: &impl Rows,
        scale: Scale,
    ) -> Vec<Mark> {
        let kinds = [
            MarkKind::Added,
            MarkKind::Modified,
            MarkKind::Deleted,
            MarkKind::Found,
        ];
        let mut covered = vec![vec![false; scale.height as usize]; 4];
        for bar in bars {
            let (a, b) = scale.span(rows.line_rows(bar.line as usize - 1));
            covered[bar.kind as usize][a as usize..b as usize].fill(true);
        }
        for m in found {
            let row = rows.offset_row(m.start);
            let (a, b) = scale.span(row..row + 1);
            covered[3][a as usize..b as usize].fill(true);
        }
        let mut out = vec![];
        for (kind, px) in kinds.into_iter().zip(covered) {
            let mut y = 0;
            while y < px.len() {
                if px[y] {
                    let start = y;
                    while y < px.len() && px[y] {
                        y += 1;
                    }
                    out.push(Mark {
                        kind,
                        start: start as u32,
                        end: y as u32,
                    });
                }
                y += 1;
            }
        }
        out
    }

    fn bar(line: u32, kind: BarKind) -> Bar {
        Bar { line, kind }
    }

    #[test]
    fn marks_match_each_bar_and_match_placed_alone() {
        // Wrapped lines (1 to 3 rows), a short file (rows taller than a pixel) and a long one.
        for (lines, height) in [(40usize, 600u32), (5_000, 700), (100_000, 900)] {
            let wraps: Vec<usize> = (0..lines).map(|i| 1 + (i * 7 % 3)).collect();
            let rows = Fake::new(&wraps, 12);
            let scale = Scale {
                k: height as f64 / (rows.row_count() as f64 * 1.3),
                height,
            };
            let mut bars = vec![];
            for l in 1..=lines as u32 {
                let kind = match l % 11 {
                    0 => Some(BarKind::Added),
                    3 | 4 => Some(BarKind::Modified),
                    7 => Some(BarKind::Deleted),
                    _ => None,
                };
                bars.extend(kind.map(|k| bar(l, k)));
            }
            bars.push(bar(lines as u32, BarKind::Deleted));
            // Matches at a line's start and on its last wrapped row.
            let mut found: Vec<Range<usize>> = vec![];
            for l in 0..lines {
                if l % 5 == 2 || l == lines - 1 {
                    found.push(l * 12 + 1..l * 12 + 3);
                }
                if l % 7 == 3 {
                    found.push(l * 12 + 9..l * 12 + 11);
                }
            }
            let got = marks(&bars, &found, lines * 12, &rows, scale);
            assert_eq!(got, naive(&bars, &found, &rows, scale), "{lines} lines");
            // The last line's deletion is far down without anything having scrolled there.
            assert!(
                got.iter()
                    .any(|m| m.kind == MarkKind::Deleted && m.end > height / 2)
            );
        }
    }

    #[test]
    fn a_whole_file_change_costs_lookups_per_pixel_not_per_line() {
        let lines = 100_000;
        let rows = Fake::new(&vec![1; lines], 12);
        let height = 800;
        let scale = Scale {
            k: height as f64 / lines as f64,
            height,
        };
        let bars: Vec<Bar> = (1..=lines as u32)
            .map(|l| bar(l, BarKind::Modified))
            .collect();
        let found: Vec<Range<usize>> = (0..lines).map(|l| l * 12..l * 12 + 2).collect();
        let got = marks(&bars, &found, lines * 12, &rows, scale);
        assert_eq!(
            got,
            vec![
                Mark {
                    kind: MarkKind::Modified,
                    start: 0,
                    end: height
                },
                Mark {
                    kind: MarkKind::Found,
                    start: 0,
                    end: height
                },
            ]
        );
        assert!(
            rows.lookups.get() < 8 * height as usize,
            "{}",
            rows.lookups.get()
        );
    }
}
