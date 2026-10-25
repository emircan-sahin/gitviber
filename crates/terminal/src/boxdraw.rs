//! Box drawing and block elements drawn as rectangles on the cell grid rather than taken from the
//! font: a font's glyphs leave gaps between rows at most line heights, and Claude Code's and
//! Codex's frames are built from them. Characters not listed here come from the font.

use gpui::{Bounds, Corners, Edges, Hsla, Pixels, Window, fill, point, px, quad, size};

/// Weights of the four arms, up, right, down, left: 0 none, 1 light, 2 heavy, 3 double.
type Arms = [u8; 4];

/// U+2500..=U+254B by arms; dashes (2504–250B) are drawn as dashes.
const LINES: [Arms; 76] = [
    [0, 1, 0, 1],
    [0, 2, 0, 2],
    [1, 0, 1, 0],
    [2, 0, 2, 0], // ─ ━ │ ┃
    [0, 1, 0, 1],
    [0, 2, 0, 2],
    [1, 0, 1, 0],
    [2, 0, 2, 0], // ┄ ┅ ┆ ┇
    [0, 1, 0, 1],
    [0, 2, 0, 2],
    [1, 0, 1, 0],
    [2, 0, 2, 0], // ┈ ┉ ┊ ┋
    [0, 1, 1, 0],
    [0, 2, 1, 0],
    [0, 1, 2, 0],
    [0, 2, 2, 0], // ┌ ┍ ┎ ┏
    [0, 0, 1, 1],
    [0, 0, 1, 2],
    [0, 0, 2, 1],
    [0, 0, 2, 2], // ┐ ┑ ┒ ┓
    [1, 1, 0, 0],
    [1, 2, 0, 0],
    [2, 1, 0, 0],
    [2, 2, 0, 0], // └ ┕ ┖ ┗
    [1, 0, 0, 1],
    [1, 0, 0, 2],
    [2, 0, 0, 1],
    [2, 0, 0, 2], // ┘ ┙ ┚ ┛
    [1, 1, 1, 0],
    [1, 2, 1, 0],
    [2, 1, 1, 0],
    [1, 1, 2, 0], // ├ ┝ ┞ ┟
    [2, 1, 2, 0],
    [2, 2, 1, 0],
    [1, 2, 2, 0],
    [2, 2, 2, 0], // ┠ ┡ ┢ ┣
    [1, 0, 1, 1],
    [1, 0, 1, 2],
    [2, 0, 1, 1],
    [1, 0, 2, 1], // ┤ ┥ ┦ ┧
    [2, 0, 2, 1],
    [2, 0, 1, 2],
    [1, 0, 2, 2],
    [2, 0, 2, 2], // ┨ ┩ ┪ ┫
    [0, 1, 1, 1],
    [0, 1, 1, 2],
    [0, 2, 1, 1],
    [0, 2, 1, 2], // ┬ ┭ ┮ ┯
    [0, 1, 2, 1],
    [0, 1, 2, 2],
    [0, 2, 2, 1],
    [0, 2, 2, 2], // ┰ ┱ ┲ ┳
    [1, 1, 0, 1],
    [1, 1, 0, 2],
    [1, 2, 0, 1],
    [1, 2, 0, 2], // ┴ ┵ ┶ ┷
    [2, 1, 0, 1],
    [2, 1, 0, 2],
    [2, 2, 0, 1],
    [2, 2, 0, 2], // ┸ ┹ ┺ ┻
    [1, 1, 1, 1],
    [1, 1, 1, 2],
    [1, 2, 1, 1],
    [1, 2, 1, 2], // ┼ ┽ ┾ ┿
    [2, 1, 1, 1],
    [1, 1, 2, 1],
    [2, 1, 2, 1],
    [2, 1, 1, 2], // ╀ ╁ ╂ ╃
    [2, 2, 1, 1],
    [1, 1, 2, 2],
    [1, 2, 2, 1],
    [2, 2, 1, 2], // ╄ ╅ ╆ ╇
    [1, 2, 2, 2],
    [2, 1, 2, 2],
    [2, 2, 2, 1],
    [2, 2, 2, 2], // ╈ ╉ ╊ ╋
];

/// U+2550..=U+256C: double lines, alone and with single ones.
const DOUBLES: [Arms; 29] = [
    [0, 3, 0, 3],
    [3, 0, 3, 0],
    [0, 3, 1, 0],
    [0, 1, 3, 0], // ═ ║ ╒ ╓
    [0, 3, 3, 0],
    [0, 0, 1, 3],
    [0, 0, 3, 1],
    [0, 0, 3, 3], // ╔ ╕ ╖ ╗
    [1, 3, 0, 0],
    [3, 1, 0, 0],
    [3, 3, 0, 0],
    [1, 0, 0, 3], // ╘ ╙ ╚ ╛
    [3, 0, 0, 1],
    [3, 0, 0, 3],
    [1, 3, 1, 0],
    [3, 1, 3, 0], // ╜ ╝ ╞ ╟
    [3, 3, 3, 0],
    [1, 0, 1, 3],
    [3, 0, 3, 1],
    [3, 0, 3, 3], // ╠ ╡ ╢ ╣
    [0, 3, 1, 3],
    [0, 1, 3, 1],
    [0, 3, 3, 3],
    [1, 3, 0, 3], // ╤ ╥ ╦ ╧
    [3, 1, 0, 1],
    [3, 3, 0, 3],
    [1, 3, 1, 3],
    [3, 1, 3, 1], // ╨ ╩ ╪ ╫
    [3, 3, 3, 3], // ╬
];

/// U+2574..=U+257F: half lines and mixed weights.
const HALVES: [Arms; 12] = [
    [0, 0, 0, 1],
    [1, 0, 0, 0],
    [0, 1, 0, 0],
    [0, 0, 1, 0], // ╴ ╵ ╶ ╷
    [0, 0, 0, 2],
    [2, 0, 0, 0],
    [0, 2, 0, 0],
    [0, 0, 2, 0], // ╸ ╹ ╺ ╻
    [0, 2, 0, 1],
    [1, 0, 2, 0],
    [0, 1, 0, 2],
    [2, 0, 1, 0], // ╼ ╽ ╾ ╿
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Glyph {
    Lines(Arms),
    /// Dashes along one axis: count, weight, horizontal.
    Dashes(u8, u8, bool),
    /// A rounded corner joining two arms: the corner's quadrant, 0 ╭ 1 ╮ 2 ╯ 3 ╰.
    Arc(u8),
    /// A filled part of the cell: left, top, right, bottom in eighths.
    Block(u8, u8, u8, u8),
    /// A whole-cell shade at this opacity.
    Shade(f32),
    /// Quadrants filled: upper left, upper right, lower left, lower right bits.
    Quadrants(u8),
}

pub fn glyph(c: char) -> Option<Glyph> {
    let u = c as u32;
    Some(match u {
        0x2504 | 0x2505 => Glyph::Dashes(3, (u - 0x2504 + 1) as u8, true),
        0x2506 | 0x2507 => Glyph::Dashes(3, (u - 0x2506 + 1) as u8, false),
        0x2508 | 0x2509 => Glyph::Dashes(4, (u - 0x2508 + 1) as u8, true),
        0x250A | 0x250B => Glyph::Dashes(4, (u - 0x250A + 1) as u8, false),
        0x254C | 0x254D => Glyph::Dashes(2, (u - 0x254C + 1) as u8, true),
        0x254E | 0x254F => Glyph::Dashes(2, (u - 0x254E + 1) as u8, false),
        0x2500..=0x254B => Glyph::Lines(LINES[(u - 0x2500) as usize]),
        0x2550..=0x256C => Glyph::Lines(DOUBLES[(u - 0x2550) as usize]),
        0x256D..=0x2570 => Glyph::Arc((u - 0x256D) as u8),
        0x2574..=0x257F => Glyph::Lines(HALVES[(u - 0x2574) as usize]),
        0x2580 => Glyph::Block(0, 0, 8, 4),
        0x2581..=0x2587 => Glyph::Block(0, 8 - (u - 0x2580) as u8, 8, 8),
        0x2588 => Glyph::Block(0, 0, 8, 8),
        0x2589..=0x258F => Glyph::Block(0, 0, 8 - (u - 0x2588) as u8, 8),
        0x2590 => Glyph::Block(4, 0, 8, 8),
        0x2591 => Glyph::Shade(0.25),
        0x2592 => Glyph::Shade(0.5),
        0x2593 => Glyph::Shade(0.75),
        0x2594 => Glyph::Block(0, 0, 8, 1),
        0x2595 => Glyph::Block(7, 0, 8, 8),
        0x2596 => Glyph::Quadrants(0b0010),
        0x2597 => Glyph::Quadrants(0b0001),
        0x2598 => Glyph::Quadrants(0b1000),
        0x2599 => Glyph::Quadrants(0b1011),
        0x259A => Glyph::Quadrants(0b1001),
        0x259B => Glyph::Quadrants(0b1110),
        0x259C => Glyph::Quadrants(0b1101),
        0x259D => Glyph::Quadrants(0b0100),
        0x259E => Glyph::Quadrants(0b0110),
        0x259F => Glyph::Quadrants(0b0111),
        _ => return None,
    })
}

/// Rounded to the device's pixels, so lines meet their neighbours' exactly.
fn snap(v: f32, scale: f32) -> f32 {
    (v * scale).round() / scale
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Bounds<Pixels> {
    Bounds::new(
        point(px(x0), px(y0)),
        size(px((x1 - x0).max(0.)), px((y1 - y0).max(0.))),
    )
}

/// Paints `g` in `cell` (whose width is one cell, or two for a wide one) in `color`.
pub fn paint(g: Glyph, cell: Bounds<Pixels>, color: Hsla, font_size: f32, window: &mut Window) {
    let scale = window.scale_factor();
    let (x0, y0) = (f32::from(cell.origin.x), f32::from(cell.origin.y));
    let (w, h) = (f32::from(cell.size.width), f32::from(cell.size.height));
    let (x1, y1) = (x0 + w, y0 + h);
    // A light line a device pixel or more, as the font's own would be at this size.
    let light = snap((font_size / 13.).max(1. / scale), scale).max(1. / scale);
    let thick = |weight: u8| if weight == 2 { light * 2. } else { light };
    let cx = snap(x0 + w / 2. - light / 2., scale) + light / 2.;
    let cy = snap(y0 + h / 2. - light / 2., scale) + light / 2.;
    let mut paint_rect = |r: Bounds<Pixels>, color: Hsla| window.paint_quad(fill(r, color));
    match g {
        Glyph::Lines(arms) => {
            let [up, right, down, left] = arms;
            let t = light;
            let half = |w: u8| match w {
                0 => 0.,
                3 => 1.5 * t,
                w => thick(w) / 2.,
            };
            let (vh, hh) = (half(up).max(half(down)), half(left).max(half(right)));
            let (double_v, double_h) = (up == 3 || down == 3, left == 3 || right == 3);
            // How far past the center a double arm's line on the `near` side reaches: short of it
            // at a double arm on that side (the inner corner), past it to a double arm across (the
            // outer one), over a single line's half, else to the center.
            let reach = |near: u8, far: u8, single: f32| {
                if near == 3 {
                    -0.5 * t
                } else if far == 3 {
                    1.5 * t
                } else if near > 0 || far > 0 {
                    single
                } else {
                    0.
                }
            };
            let mut p =
                |x0: f32, y0: f32, x1: f32, y1: f32| paint_rect(rect(x0, y0, x1, y1), color);
            let (above, below) = ((cy - 1.5 * t, cy - 0.5 * t), (cy + 0.5 * t, cy + 1.5 * t));
            let (leftside, rightside) =
                ((cx - 1.5 * t, cx - 0.5 * t), (cx + 0.5 * t, cx + 1.5 * t));
            match left {
                3 => {
                    p(x0, above.0, cx + reach(up, down, vh), above.1);
                    p(x0, below.0, cx + reach(down, up, vh), below.1);
                }
                0 => {}
                w => p(
                    x0,
                    cy - thick(w) / 2.,
                    if double_v { cx - 0.5 * t } else { cx + vh },
                    cy + thick(w) / 2.,
                ),
            }
            match right {
                3 => {
                    p(cx - reach(up, down, vh), above.0, x1, above.1);
                    p(cx - reach(down, up, vh), below.0, x1, below.1);
                }
                0 => {}
                w => p(
                    if double_v { cx + 0.5 * t } else { cx - vh },
                    cy - thick(w) / 2.,
                    x1,
                    cy + thick(w) / 2.,
                ),
            }
            match up {
                3 => {
                    p(leftside.0, y0, leftside.1, cy + reach(left, right, hh));
                    p(rightside.0, y0, rightside.1, cy + reach(right, left, hh));
                }
                0 => {}
                w => p(
                    cx - thick(w) / 2.,
                    y0,
                    cx + thick(w) / 2.,
                    if double_h { cy - 0.5 * t } else { cy + hh },
                ),
            }
            match down {
                3 => {
                    p(leftside.0, cy - reach(left, right, hh), leftside.1, y1);
                    p(rightside.0, cy - reach(right, left, hh), rightside.1, y1);
                }
                0 => {}
                w => p(
                    cx - thick(w) / 2.,
                    if double_h { cy + 0.5 * t } else { cy - hh },
                    cx + thick(w) / 2.,
                    y1,
                ),
            }
        }
        Glyph::Dashes(count, weight, horizontal) => {
            let t = thick(weight);
            let n = count as f32;
            let (len, along) = if horizontal { (w, x0) } else { (h, y0) };
            let step = len / n;
            for i in 0..count {
                let a = snap(along + step * i as f32 + step * 0.15, scale);
                let b = snap(along + step * (i as f32 + 1.) - step * 0.15, scale);
                let r = if horizontal {
                    rect(a, cy - t / 2., b, cy + t / 2.)
                } else {
                    rect(cx - t / 2., a, cx + t / 2., b)
                };
                paint_rect(r, color);
            }
        }
        Glyph::Arc(corner) => {
            let t = light;
            let r = (w.min(h) / 2.).max(t);
            // A quad as big as the arc's square, bordered on the two sides that meet at the
            // corner, clipped to the cell: the arc and both arms out to the cell's edges.
            let (left, top) = (cx - t / 2., cy - t / 2.);
            let big = w.max(h) * 2.;
            let (bounds, radii, borders) = match corner {
                0 => (
                    rect(left, top, left + big, top + big),
                    Corners {
                        top_left: px(r),
                        ..Default::default()
                    },
                    Edges {
                        top: px(t),
                        left: px(t),
                        ..Default::default()
                    },
                ),
                1 => (
                    rect(left + t - big, top, left + t, top + big),
                    Corners {
                        top_right: px(r),
                        ..Default::default()
                    },
                    Edges {
                        top: px(t),
                        right: px(t),
                        ..Default::default()
                    },
                ),
                2 => (
                    rect(left + t - big, top + t - big, left + t, top + t),
                    Corners {
                        bottom_right: px(r),
                        ..Default::default()
                    },
                    Edges {
                        bottom: px(t),
                        right: px(t),
                        ..Default::default()
                    },
                ),
                _ => (
                    rect(left, top + t - big, left + big, top + t),
                    Corners {
                        bottom_left: px(r),
                        ..Default::default()
                    },
                    Edges {
                        bottom: px(t),
                        left: px(t),
                        ..Default::default()
                    },
                ),
            };
            window.with_content_mask(Some(gpui::ContentMask { bounds: cell }), |window| {
                window.paint_quad(quad(
                    bounds,
                    radii,
                    gpui::transparent_black(),
                    borders,
                    color,
                    gpui::BorderStyle::Solid,
                ));
            });
        }
        Glyph::Block(l, t, r, b) => {
            let fx = |e: u8| snap(x0 + w * e as f32 / 8., scale);
            let fy = |e: u8| snap(y0 + h * e as f32 / 8., scale);
            paint_rect(rect(fx(l), fy(t), fx(r), fy(b)), color);
        }
        Glyph::Shade(alpha) => paint_rect(cell, color.opacity(alpha)),
        Glyph::Quadrants(bits) => {
            let mx = snap(x0 + w / 2., scale);
            let my = snap(y0 + h / 2., scale);
            if bits & 0b1000 != 0 {
                paint_rect(rect(x0, y0, mx, my), color);
            }
            if bits & 0b0100 != 0 {
                paint_rect(rect(mx, y0, x1, my), color);
            }
            if bits & 0b0010 != 0 {
                paint_rect(rect(x0, my, mx, y1), color);
            }
            if bits & 0b0001 != 0 {
                paint_rect(rect(mx, my, x1, y1), color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_frames_agents_draw_are_drawn_not_taken_from_the_font() {
        assert_eq!(glyph('─'), Some(Glyph::Lines([0, 1, 0, 1])));
        assert_eq!(glyph('│'), Some(Glyph::Lines([1, 0, 1, 0])));
        assert_eq!(glyph('┼'), Some(Glyph::Lines([1, 1, 1, 1])));
        assert_eq!(glyph('┏'), Some(Glyph::Lines([0, 2, 2, 0])));
        assert_eq!(glyph('╋'), Some(Glyph::Lines([2, 2, 2, 2])));
        assert_eq!(glyph('╭'), Some(Glyph::Arc(0)));
        assert_eq!(glyph('╯'), Some(Glyph::Arc(2)));
        assert_eq!(glyph('╿'), Some(Glyph::Lines([2, 0, 1, 0])));
        assert_eq!(glyph('┄'), Some(Glyph::Dashes(3, 1, true)));
        assert_eq!(glyph('█'), Some(Glyph::Block(0, 0, 8, 8)));
        assert_eq!(glyph('▁'), Some(Glyph::Block(0, 7, 8, 8)));
        assert_eq!(glyph('▏'), Some(Glyph::Block(0, 0, 1, 8)));
        assert_eq!(glyph('▌'), Some(Glyph::Block(0, 0, 4, 8)));
        assert_eq!(glyph('▄'), Some(Glyph::Block(0, 4, 8, 8)));
        assert_eq!(glyph('╔'), Some(Glyph::Lines([0, 3, 3, 0])));
        assert_eq!(glyph('╬'), Some(Glyph::Lines([3, 3, 3, 3])));
        // Diagonals come from the font.
        assert_eq!(glyph('╱'), None);
        assert_eq!(glyph('a'), None);
    }

    #[test]
    fn every_line_character_has_arms() {
        for u in 0x2500..=0x254Bu32 {
            match glyph(char::from_u32(u).unwrap()) {
                Some(Glyph::Lines(arms)) => assert!(arms.iter().any(|&a| a > 0), "{u:x}"),
                Some(Glyph::Dashes(..)) => {}
                other => panic!("{u:x}: {other:?}"),
            }
        }
    }
}
