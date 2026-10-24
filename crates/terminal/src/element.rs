//! Paints a pane's grid: only the rows on screen, text in runs of one style shaped per word and
//! reused across frames while a row's cells stay the same, box drawing as rectangles, then the
//! selection, find's matches, the cursor, the command marks and an IME's composition on top.

use std::collections::HashMap;
use std::rc::Rc;

use alacritty_terminal::grid::{Dimensions, Row};
use alacritty_terminal::index::{Column, Line, Point as GridPoint};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use gpui::{
    App, BorderStyle, Bounds, CursorStyle as MouseCursor, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, Font, FontFeatures, FontStyle, FontWeight, GlobalElementId,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, Rgba, ScrollWheelEvent,
    ShapedLine, SharedString, Style, TextAlign, TextRun, Window, fill, outline, point, px,
    relative, size,
};

use crate::boxdraw::{self, Glyph};
use crate::config::TerminalTheme;
use crate::marks::{is_mark_char, mark_on};
use crate::pane::{Metrics, PAD_BOTTOM, PAD_LEFT, PAD_RIGHT, PAD_TOP, TerminalPane};

/// Rows shaped in the last frame, by their cells' hash: a row that only moved (scrolling) or
/// didn't change is painted from here. Holds a screenful; cleared on a resize, font or theme change.
#[derive(Default)]
pub(crate) struct RowCache {
    rows: HashMap<u64, Rc<RowShape>>,
    key: u64,
}

impl RowCache {
    pub fn clear(&mut self) {
        self.rows = HashMap::new();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Deco {
    Under,
    Double,
    Curl,
    Dotted,
    Dashed,
    Strike,
}

#[derive(Default)]
pub(crate) struct RowShape {
    texts: Vec<(usize, ShapedLine)>,
    backgrounds: Vec<(usize, usize, Hsla)>,
    decorations: Vec<(usize, usize, Deco, Hsla)>,
    boxes: Vec<(usize, usize, Glyph, Hsla)>,
}

pub(crate) struct TerminalElement {
    pane: Entity<TerminalPane>,
}

impl TerminalElement {
    pub fn new(pane: Entity<TerminalPane>) -> Self {
        TerminalElement { pane }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct CursorPaint {
    bounds: Bounds<Pixels>,
    shape: CursorShape,
    text: Option<ShapedLine>,
}

pub(crate) struct Frame {
    hitbox: Hitbox,
    metrics: Metrics,
    background: Hsla,
    rows: Vec<(Pixels, Rc<RowShape>)>,
    highlights: Vec<(Bounds<Pixels>, Hsla)>,
    cursor: Option<CursorPaint>,
    cursor_color: Hsla,
    marks: Vec<(Bounds<Pixels>, Hsla)>,
    preedit: Option<(Bounds<Pixels>, ShapedLine)>,
    scrollbar: Option<(Bounds<Pixels>, Hsla)>,
    takes_mouse: bool,
    font_size: f32,
}

/// FNV-1a over a row's cells: what decides whether its shape can be reused.
fn hash_row(row: &Row<Cell>, cols: usize) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut mix = |v: u64| h = (h ^ v).wrapping_mul(0x100000001b3);
    for col in 0..cols {
        let cell = &row[Column(col)];
        mix(cell.c as u64);
        mix(color_key(cell.fg) as u64 | (color_key(cell.bg) as u64) << 32);
        mix(cell.flags.bits() as u64);
        if let Some(extra) = cell.zerowidth() {
            for c in extra {
                mix(*c as u64);
            }
        }
        if let Some(c) = cell.underline_color() {
            mix(color_key(c) as u64 ^ 0x5555);
        }
    }
    h
}

fn color_key(c: Color) -> u32 {
    match c {
        Color::Named(n) => 0x1000_0000 | n as u32,
        Color::Spec(Rgb { r, g, b }) => 0x2000_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32,
        Color::Indexed(i) => 0x3000_0000 | i as u32,
    }
}

fn rgb(c: Rgb) -> Hsla {
    Rgba {
        r: c.r as f32 / 255.,
        g: c.g as f32 / 255.,
        b: c.b as f32 / 255.,
        a: 1.,
    }
    .into()
}

/// xterm's 256 colors past the 16: the 6×6×6 cube, then the greys.
fn indexed(i: u8, theme: &TerminalTheme) -> Hsla {
    match i {
        0..16 => theme.ansi[i as usize],
        16..232 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            rgb(Rgb {
                r: level(i / 36),
                g: level((i / 6) % 6),
                b: level(i % 6),
            })
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            rgb(Rgb { r: v, g: v, b: v })
        }
    }
}

struct Palette<'a> {
    theme: &'a TerminalTheme,
    /// A program's own colors, copied: rows are shaped after the terminal's lock is let go.
    colors: Colors,
}

impl Palette<'_> {
    fn named(&self, n: NamedColor) -> Hsla {
        if let Some(c) = self.colors[n] {
            return rgb(c);
        }
        let t = self.theme;
        match n {
            NamedColor::Foreground | NamedColor::BrightForeground => t.foreground,
            NamedColor::Background => t.background,
            NamedColor::Cursor => t.cursor,
            NamedColor::DimForeground => t.foreground.opacity(0.5),
            n if (n as usize) < 16 => self.index(n as u8),
            // DimBlack..DimWhite.
            n => self
                .index(
                    (n as usize)
                        .saturating_sub(NamedColor::DimBlack as usize)
                        .min(7) as u8,
                )
                .opacity(0.5),
        }
    }

    fn index(&self, i: u8) -> Hsla {
        match self.colors[i as usize] {
            Some(c) => rgb(c),
            None => indexed(i, self.theme),
        }
    }

    fn color(&self, c: Color, bright: bool) -> Hsla {
        match c {
            Color::Named(n) if bright && (n as usize) < 8 => self.index(n as u8 + 8),
            Color::Named(n) => self.named(n),
            Color::Indexed(i) if bright && i < 8 => self.index(i + 8),
            Color::Indexed(i) => self.index(i),
            Color::Spec(c) => rgb(c),
        }
    }

    /// A cell's text and background colors after bold, dim, inverse and hidden.
    fn cell(&self, cell: &Cell) -> (Hsla, Hsla, bool) {
        let bold = cell.flags.contains(Flags::BOLD);
        let mut fg = self.color(cell.fg, bold);
        let mut bg = self.color(cell.bg, false);
        let mut default_bg = cell.bg == Color::Named(NamedColor::Background);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
            default_bg = false;
        }
        if cell.flags.contains(Flags::DIM) {
            fg = fg.opacity(0.5);
        } else {
            fg = contrast(fg, bg, self.theme.minimum_contrast);
        }
        (fg, bg, default_bg)
    }
}

fn luminance(c: Hsla) -> f32 {
    let Rgba { r, g, b, .. } = c.to_rgb();
    let lin = |v: f32| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn ratio(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `fg` moved toward white or black until it reads on `bg` at `min`: a theme's dim colors
/// (Solarized Dark's bright black is its background) are lifted so autosuggestions show.
fn contrast(fg: Hsla, bg: Hsla, min: f32) -> Hsla {
    if min <= 1. || fg.a < 1. {
        return fg;
    }
    let lb = luminance(bg);
    if ratio(luminance(fg), lb) >= min {
        return fg;
    }
    let toward = if lb < 0.5 { 1. } else { 0. };
    let mut out = fg;
    for step in 1..=10 {
        out.l = fg.l + (toward - fg.l) * step as f32 / 10.;
        if ratio(luminance(out), lb) >= min {
            break;
        }
    }
    out
}

struct Fonts {
    regular: Font,
    bold: Font,
    italic: Font,
    bold_italic: Font,
}

impl Fonts {
    fn new(family: &SharedString, weight: f32) -> Self {
        let font = |w: f32, style| Font {
            family: family.clone(),
            features: FontFeatures::disable_ligatures(),
            fallbacks: None,
            weight: FontWeight(w),
            style,
        };
        // Bold stays two steps above whatever the text is, so it still stands out at Semibold.
        let bold = (weight + 200.).min(900.);
        Fonts {
            regular: font(weight, FontStyle::Normal),
            bold: font(bold, FontStyle::Normal),
            italic: font(weight, FontStyle::Italic),
            bold_italic: font(bold, FontStyle::Italic),
        }
    }

    fn for_flags(&self, flags: Flags) -> &Font {
        match (flags.contains(Flags::BOLD), flags.contains(Flags::ITALIC)) {
            (false, false) => &self.regular,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (true, true) => &self.bold_italic,
        }
    }
}

/// A run of narrow cells with glyphs, shaped as one line.
struct Segment {
    start: usize,
    text: String,
    runs: Vec<TextRun>,
}

fn build_row(
    row: &Row<Cell>,
    cols: usize,
    palette: &Palette,
    fonts: &Fonts,
    font_size: Pixels,
    cell_w: Pixels,
    window: &Window,
) -> RowShape {
    let mut shape = RowShape::default();
    let mut seg: Option<Segment> = None;
    let shape_segment = |seg: Segment, shape: &mut RowShape| {
        let line =
            window
                .text_system()
                .shape_line(seg.text.into(), font_size, &seg.runs, Some(cell_w));
        shape.texts.push((seg.start, line));
    };
    let mut col = 0;
    while col < cols {
        let cell = &row[Column(col)];
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            if let Some(s) = seg.take() {
                shape_segment(s, &mut shape);
            }
            col += 1;
            continue;
        }
        let width = if cell.flags.contains(Flags::WIDE_CHAR) {
            2.min(cols - col)
        } else {
            1
        };
        let (fg, bg, default_bg) = palette.cell(cell);
        if !default_bg {
            match shape.backgrounds.last_mut() {
                Some((_, end, c)) if *end == col && *c == bg => *end = col + width,
                _ => shape.backgrounds.push((col, col + width, bg)),
            }
        }
        let deco = if cell.flags.contains(Flags::STRIKEOUT) {
            Some(Deco::Strike)
        } else {
            None
        };
        let under = if cell.flags.contains(Flags::DOUBLE_UNDERLINE) {
            Some(Deco::Double)
        } else if cell.flags.contains(Flags::UNDERCURL) {
            Some(Deco::Curl)
        } else if cell.flags.contains(Flags::DOTTED_UNDERLINE) {
            Some(Deco::Dotted)
        } else if cell.flags.contains(Flags::DASHED_UNDERLINE) {
            Some(Deco::Dashed)
        } else if cell.flags.contains(Flags::UNDERLINE) {
            Some(Deco::Under)
        } else {
            None
        };
        for d in [under, deco].into_iter().flatten() {
            let color = match (d, cell.underline_color()) {
                (Deco::Strike, _) | (_, None) => fg,
                (_, Some(c)) => palette.color(c, false),
            };
            match shape.decorations.iter_mut().rev().find(|x| x.2 == d) {
                Some((_, end, _, c)) if *end == col && *c == color => *end = col + width,
                _ => shape.decorations.push((col, col + width, d, color)),
            }
        }
        let c = cell.c;
        let blank = c == ' ' || c == '\t' || cell.flags.contains(Flags::HIDDEN);
        let boxed = if blank { None } else { boxdraw::glyph(c) };
        if (blank || boxed.is_some() || width == 2)
            && let Some(s) = seg.take()
        {
            shape_segment(s, &mut shape);
        }
        if let Some(g) = boxed {
            shape.boxes.push((col, width, g, fg));
        } else if !blank {
            let font = fonts.for_flags(cell.flags);
            let s = seg.get_or_insert_with(|| Segment {
                start: col,
                text: String::new(),
                runs: Vec::new(),
            });
            let before = s.text.len();
            s.text.push(c);
            for z in cell.zerowidth().into_iter().flatten() {
                if !is_mark_char(*z) {
                    s.text.push(*z);
                }
            }
            let len = s.text.len() - before;
            match s.runs.last_mut() {
                Some(r) if r.color == fg && r.font == *font => r.len += len,
                _ => s.runs.push(TextRun {
                    len,
                    font: font.clone(),
                    color: fg,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }),
            }
            if width == 2
                && let Some(s) = seg.take()
            {
                shape_segment(s, &mut shape);
            }
        }
        col += width;
    }
    if let Some(s) = seg.take() {
        shape_segment(s, &mut shape);
    }
    shape
}

/// A line's spans `[from, to]` of columns on screen row `row`, for the selection and matches.
fn spans(
    start: GridPoint,
    end: GridPoint,
    block: bool,
    line: Line,
    cols: usize,
) -> Option<(usize, usize)> {
    if line < start.line || line > end.line {
        return None;
    }
    if block {
        let (a, b) = (
            start.column.0.min(end.column.0),
            start.column.0.max(end.column.0),
        );
        return Some((a, b));
    }
    let from = if line == start.line {
        start.column.0
    } else {
        0
    };
    let to = if line == end.line {
        end.column.0
    } else {
        cols - 1
    };
    (from <= to).then_some((from, to))
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Frame;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Frame {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let (config, theme) = {
            let p = self.pane.read(cx);
            (p.config.clone(), p.theme.clone())
        };
        let fonts = Fonts::new(&config.font_family, config.font_weight);
        let font_size = px(config.font_size);
        let scale = window.scale_factor();
        let ts = window.text_system();
        let font_id = ts.resolve_font(&fonts.regular);
        let advance = ts
            .advance(font_id, font_size, 'm')
            .map(|s| s.width)
            .unwrap_or(font_size * 0.6);
        let ascent = f32::from(ts.ascent(font_id, font_size));
        let descent = f32::from(ts.descent(font_id, font_size)).abs();
        let cell_w = px((f32::from(advance) * scale).round().max(1.) / scale);
        let cell_h = px(((ascent + descent) * config.line_height * scale)
            .ceil()
            .max(1.)
            / scale);
        let origin = bounds.origin + point(px(PAD_LEFT), px(PAD_TOP));
        let room_w = f32::from(bounds.size.width) - PAD_LEFT - PAD_RIGHT;
        let room_h = f32::from(bounds.size.height) - PAD_TOP - PAD_BOTTOM;
        let metrics = Metrics {
            bounds,
            origin,
            cell: size(cell_w, cell_h),
            cols: (room_w / f32::from(cell_w)).floor().max(0.) as usize,
            rows: (room_h / f32::from(cell_h)).floor().max(0.) as usize,
        };
        self.pane.update(cx, |p, cx| p.fit(metrics, cx));

        let pane = self.pane.read(cx);
        let emu = pane.session.emu.lock();
        let term = &emu.term;
        let grid = term.grid();
        let mode = *term.mode();
        let cols = term.columns();
        let screen = term.screen_lines();
        let offset = grid.display_offset() as i32;
        let palette = Palette {
            theme: &theme,
            colors: *term.colors(),
        };

        // The shapes depend on the font, the cell size and a program's palette changes too.
        let mut key: u64 = 0xcbf29ce484222325;
        let mut mix = |v: u64| key = (key ^ v).wrapping_mul(0x100000001b3);
        mix(f32::from(cell_w).to_bits() as u64);
        mix(config.font_size.to_bits() as u64);
        for i in 0..alacritty_terminal::term::color::COUNT {
            if let Some(c) = term.colors()[i] {
                mix((i as u64) << 24 | (c.r as u64) << 16 | (c.g as u64) << 8 | c.b as u64);
            }
        }
        let mut cache = pane.cache.borrow_mut();
        if cache.key != key {
            cache.clear();
            cache.key = key;
        }
        let mut next: HashMap<u64, Rc<RowShape>> = HashMap::with_capacity(screen);
        let mut rows: Vec<(Pixels, Option<Rc<RowShape>>)> = Vec::with_capacity(screen);
        // Rows to shape, copied out: the reader thread waits on the lock while it's held.
        let mut missed = Vec::new();
        let mut marks = Vec::new();
        for r in 0..screen {
            let line = Line(r as i32 - offset);
            let row = &grid[line];
            let h = hash_row(row, cols);
            let y = origin.y + cell_h * r as f32;
            match next.get(&h).cloned().or_else(|| cache.rows.remove(&h)) {
                Some(shape) => {
                    next.insert(h, shape.clone());
                    rows.push((y, Some(shape)));
                }
                None => {
                    missed.push((rows.len(), h, row.clone()));
                    rows.push((y, None));
                }
            }
            if let Some(id) = mark_on(term, line)
                && let Some(ended) = emu.marks.command(id).and_then(|c| c.end.as_ref())
            {
                let failed = ended.end.exit.is_some_and(|e| e != 0);
                let dot = px(5.);
                let at = point(origin.x - px(8.), y + (cell_h - dot) / 2.);
                marks.push((
                    Bounds::new(at, size(dot, dot)),
                    if failed {
                        theme.destructive
                    } else {
                        theme.subtle
                    },
                ));
            }
        }
        let mut highlights = Vec::new();
        let row_bounds = |r: usize, from: usize, to: usize| {
            Bounds::new(
                point(
                    origin.x + cell_w * from as f32,
                    origin.y + cell_h * r as f32,
                ),
                size(cell_w * (to + 1 - from) as f32, cell_h),
            )
        };
        if let Some(find) = pane.find.as_ref() {
            let (top, bottom) = (-offset, screen as i32 - 1 - offset);
            // Found before the rows that came since scrolled them up.
            let delta = emu.scrolled - find.scanned_at;
            for (i, m) in find.window.matches.iter().enumerate() {
                let (start, end) = (
                    crate::pane::shifted(*m.start(), delta),
                    crate::pane::shifted(*m.end(), delta),
                );
                if end.line.0 < top || start.line.0 > bottom {
                    continue;
                }
                let color = if find.current == Some(i) {
                    theme.find_current
                } else {
                    theme.find_match
                };
                for l in start.line.0.max(top)..=end.line.0.min(bottom) {
                    if let Some((a, b)) = spans(start, end, false, Line(l), cols) {
                        highlights.push((row_bounds((l + offset) as usize, a, b), color));
                    }
                }
            }
        }
        if let Some(sel) = term.selection.as_ref().and_then(|s| s.to_range(term)) {
            for r in 0..screen {
                let line = Line(r as i32 - offset);
                if let Some((a, b)) = spans(sel.start, sel.end, sel.is_block, line, cols) {
                    highlights.push((row_bounds(r, a, b), theme.selection));
                }
            }
        }

        let style = term.cursor_style();
        let cursor_point = grid.cursor.point;
        let cursor_row = cursor_point.line.0 + offset;
        let blink_off = style.blinking && config.cursor_blink && !pane.blink_visible;
        let mut cursor = None;
        if mode.contains(TermMode::SHOW_CURSOR)
            && (0..screen as i32).contains(&cursor_row)
            && !blink_off
            && pane.ime.is_none()
        {
            let cell = &grid[cursor_point];
            let wide = cell.flags.contains(Flags::WIDE_CHAR);
            let at = point(
                origin.x + cell_w * cursor_point.column.0 as f32,
                origin.y + cell_h * cursor_row as f32,
            );
            let w = if wide { cell_w * 2. } else { cell_w };
            let shape = if pane.focused {
                style.shape
            } else {
                CursorShape::HollowBlock
            };
            let text = (shape == CursorShape::Block
                && cell.c != ' '
                && cell.c != '\t'
                && boxdraw::glyph(cell.c).is_none())
            .then(|| {
                let run = TextRun {
                    len: cell.c.len_utf8(),
                    font: fonts.for_flags(cell.flags).clone(),
                    color: theme.cursor_text,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                window
                    .text_system()
                    .shape_line(cell.c.to_string().into(), font_size, &[run], None)
            });
            cursor = Some(CursorPaint {
                bounds: Bounds::new(at, size(w, cell_h)),
                shape,
                text,
            });
        }

        let preedit = pane.ime.as_ref().filter(|t| !t.is_empty()).map(|text| {
            let run = TextRun {
                len: text.len(),
                font: fonts.regular.clone(),
                color: theme.foreground,
                background_color: Some(theme.background),
                underline: Some(gpui::UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme.foreground),
                    wavy: false,
                }),
                strikethrough: None,
            };
            let line =
                window
                    .text_system()
                    .shape_line(text.clone().into(), font_size, &[run], None);
            let at = point(
                origin.x + cell_w * cursor_point.column.0 as f32,
                origin.y + cell_h * cursor_row.max(0) as f32,
            );
            (Bounds::new(at, size(line.width, cell_h)), line)
        });

        let history = term.history_size();
        let scrollbar = (offset > 0 && history > 0).then(|| {
            let track = cell_h * screen as f32;
            let total = (history + screen) as f32;
            let thumb = (track * (screen as f32 / total)).max(px(20.));
            let top = origin.y + (track - thumb) * (1. - offset as f32 / history as f32);
            let x = bounds.origin.x + bounds.size.width - px(7.);
            (
                Bounds::new(point(x, top), size(px(5.), thumb)),
                theme.foreground.opacity(0.3),
            )
        });

        let takes_mouse = mode.intersects(TermMode::MOUSE_MODE);
        drop(emu);
        for (i, h, row) in missed {
            let shape = match next.get(&h) {
                Some(shape) => shape.clone(),
                None => {
                    let shape = Rc::new(build_row(
                        &row, cols, &palette, &fonts, font_size, cell_w, window,
                    ));
                    next.insert(h, shape.clone());
                    shape
                }
            };
            rows[i].1 = Some(shape);
        }
        cache.rows = next;
        drop(cache);
        let rows: Vec<(Pixels, Rc<RowShape>)> = rows
            .into_iter()
            .map(|(y, shape)| (y, shape.unwrap_or_default()))
            .collect();
        Frame {
            hitbox,
            metrics,
            background: theme.background,
            rows,
            highlights,
            cursor,
            cursor_color: theme.cursor,
            marks,
            preedit,
            scrollbar,
            takes_mouse,
            font_size: config.font_size,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        frame: &mut Frame,
        window: &mut Window,
        cx: &mut App,
    ) {
        let m = frame.metrics;
        let (cw, ch) = (m.cell.width, m.cell.height);
        window.paint_quad(fill(bounds, frame.background));
        window.set_cursor_style(
            if frame.takes_mouse {
                MouseCursor::Arrow
            } else {
                MouseCursor::IBeam
            },
            &frame.hitbox,
        );
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for (y, row) in &frame.rows {
                for &(from, to, color) in &row.backgrounds {
                    let b = Bounds::new(
                        point(m.origin.x + cw * from as f32, *y),
                        size(cw * (to - from) as f32, ch),
                    );
                    window.paint_quad(fill(b, color));
                }
            }
            for (b, color) in &frame.highlights {
                window.paint_quad(fill(*b, *color));
            }
            if let Some(c) = frame
                .cursor
                .as_ref()
                .filter(|c| c.shape == CursorShape::Block)
            {
                window.paint_quad(fill(c.bounds, frame.cursor_color));
            }
            for (y, row) in &frame.rows {
                for (col, line) in &row.texts {
                    let at = point(m.origin.x + cw * *col as f32, *y);
                    let _ = line.paint(at, ch, TextAlign::Left, None, window, cx);
                }
                for &(col, width, glyph, color) in &row.boxes {
                    let cell = Bounds::new(
                        point(m.origin.x + cw * col as f32, *y),
                        size(cw * width as f32, ch),
                    );
                    boxdraw::paint(glyph, cell, color, frame.font_size, window);
                }
                for &(from, to, deco, color) in &row.decorations {
                    paint_decoration(
                        deco,
                        m.origin.x + cw * from as f32,
                        cw * (to - from) as f32,
                        *y,
                        ch,
                        color,
                        window,
                    );
                }
            }
            if let Some(c) = frame.cursor.as_ref() {
                let b = c.bounds;
                match c.shape {
                    CursorShape::Block => {
                        if let Some(text) = &c.text {
                            let _ = text.paint(b.origin, ch, TextAlign::Left, None, window, cx);
                        }
                    }
                    CursorShape::Beam => window.paint_quad(fill(
                        Bounds::new(b.origin, size(px(2.), ch)),
                        frame.cursor_color,
                    )),
                    CursorShape::Underline => window.paint_quad(fill(
                        Bounds::new(
                            point(b.origin.x, b.origin.y + ch - px(2.)),
                            size(b.size.width, px(2.)),
                        ),
                        frame.cursor_color,
                    )),
                    CursorShape::HollowBlock => {
                        window.paint_quad(outline(b, frame.cursor_color, BorderStyle::Solid))
                    }
                    CursorShape::Hidden => {}
                }
            }
            if let Some((b, line)) = &frame.preedit {
                window.paint_quad(fill(*b, frame.background));
                let _ = line.paint(b.origin, ch, TextAlign::Left, None, window, cx);
            }
        });
        for (b, color) in &frame.marks {
            window.paint_quad(gpui::quad(
                *b,
                px(2.5),
                *color,
                px(0.),
                gpui::transparent_black(),
                BorderStyle::Solid,
            ));
        }
        if let Some((b, color)) = frame.scrollbar {
            window.paint_quad(gpui::quad(
                b,
                px(2.5),
                color,
                px(0.),
                gpui::transparent_black(),
                BorderStyle::Solid,
            ));
        }

        let pane = self.pane.clone();
        let focus = pane.read(cx).focus_handle_ref().clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, pane.clone()), cx);

        let hitbox = frame.hitbox.clone();
        let takes_mouse = frame.takes_mouse;
        window.on_mouse_event({
            let pane = pane.clone();
            let hitbox = hitbox.clone();
            move |e: &MouseDownEvent, phase, window, cx| {
                if !hitbox.is_hovered(window) {
                    return;
                }
                // A program reading the mouse gets the right-click, ahead of the pane's menu.
                let program = takes_mouse
                    && !(if cfg!(target_os = "macos") {
                        e.modifiers.alt
                    } else {
                        e.modifiers.shift
                    });
                let wanted = match e.button {
                    MouseButton::Right => program && phase == DispatchPhase::Capture,
                    _ => phase == DispatchPhase::Bubble,
                };
                if wanted {
                    pane.update(cx, |p, cx| p.mouse_down(e, window, cx));
                }
            }
        });
        window.on_mouse_event({
            let pane = pane.clone();
            let hitbox = hitbox.clone();
            move |e: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && (e.pressed_button.is_some() || hitbox.is_hovered(window))
                {
                    pane.update(cx, |p, cx| p.mouse_move(e, window, cx));
                }
            }
        });
        window.on_mouse_event({
            let pane = pane.clone();
            move |e: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    pane.update(cx, |p, cx| p.mouse_up(e, window, cx));
                }
            }
        });
        window.on_mouse_event({
            let pane = pane.clone();
            let hitbox = hitbox.clone();
            move |e: &ScrollWheelEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    pane.update(cx, |p, cx| p.scroll_wheel(e, window, cx));
                }
            }
        });
        window.on_mouse_event(move |e: &PinchEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                pane.update(cx, |p, cx| p.pinched(e, window, cx));
            }
        });
    }
}

fn paint_decoration(
    deco: Deco,
    x: Pixels,
    w: Pixels,
    y: Pixels,
    h: Pixels,
    color: Hsla,
    window: &mut Window,
) {
    let t = px(1.);
    let base = y + h - px(2.);
    let mut line = |at: Pixels, from: Pixels, len: Pixels| {
        window.paint_quad(fill(Bounds::new(point(from, at), size(len, t)), color))
    };
    match deco {
        Deco::Under => line(base, x, w),
        Deco::Double => {
            line(base, x, w);
            line(base - px(2.), x, w);
        }
        Deco::Strike => line(y + h / 2., x, w),
        Deco::Dotted | Deco::Dashed | Deco::Curl => {
            let (on, gap) = match deco {
                Deco::Dotted => (px(1.), px(1.)),
                Deco::Dashed => (px(3.), px(2.)),
                _ => (px(2.), px(2.)),
            };
            let mut at = x;
            let mut up = false;
            while at < x + w {
                let len = on.min(x + w - at);
                let dy = if deco == Deco::Curl && up {
                    px(-1.)
                } else {
                    px(0.)
                };
                line(base + dy, at, len);
                at += on + if deco == Deco::Curl { px(0.) } else { gap };
                up = !up;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dim_theme_colors_are_lifted_until_they_read() {
        let bg = crate::config::hex("#002b36");
        let fg = crate::config::hex("#073642");
        let lifted = contrast(fg, bg, 4.5);
        assert!(ratio(luminance(lifted), luminance(bg)) >= 4.5);
        let fine = crate::config::hex("#ececee");
        assert_eq!(contrast(fine, bg, 4.5), fine);
    }

    #[test]
    fn the_256_colors_are_xterms() {
        let t = TerminalTheme::dark();
        assert_eq!(indexed(16, &t), rgb(Rgb { r: 0, g: 0, b: 0 }));
        assert_eq!(indexed(196, &t), rgb(Rgb { r: 255, g: 0, b: 0 }));
        assert_eq!(indexed(232, &t), rgb(Rgb { r: 8, g: 8, b: 8 }));
        assert_eq!(
            indexed(255, &t),
            rgb(Rgb {
                r: 238,
                g: 238,
                b: 238
            })
        );
        assert_eq!(indexed(1, &t), t.ansi[1]);
    }

    #[test]
    fn selection_spans_per_row() {
        let p = |l: i32, c: usize| GridPoint::new(Line(l), Column(c));
        assert_eq!(spans(p(0, 3), p(2, 5), false, Line(0), 80), Some((3, 79)));
        assert_eq!(spans(p(0, 3), p(2, 5), false, Line(1), 80), Some((0, 79)));
        assert_eq!(spans(p(0, 3), p(2, 5), false, Line(2), 80), Some((0, 5)));
        assert_eq!(spans(p(0, 3), p(2, 5), false, Line(3), 80), None);
        assert_eq!(spans(p(0, 8), p(2, 5), true, Line(1), 80), Some((5, 8)));
    }
}
