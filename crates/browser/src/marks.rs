//! An agent's screenshot (`screenshot`), apart from AppKit so it can be tested: what it asks
//! for, its size in pixels, and where a ref's tag goes on it. AppKit draws (macos/page_tools.rs).

use super::picks::Bounds;

/// The longest side a screenshot gets: a bigger picture breaks an agent's conversation.
pub const MOST_SIDE: f64 = 2000.0;

/// A screenshot as asked: `rect` of the viewport (CSS px) at `scale` pixels a CSS px, with
/// `labels` (CSS px in the viewport), as a PNG or a JPEG.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub rect: Bounds,
    pub scale: f64,
    pub labels: Vec<Label>,
    pub jpeg: bool,
}

/// A ref's box, and the ref.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A screenshot taken: its file's bytes, its size, and the pixels a CSS px it ended up at.
pub struct Taken {
    pub bytes: Vec<u8>,
    pub size: (u32, u32),
    pub scale: f64,
}

/// The pixels for `w`×`h` CSS px at `scale`, and the scale they end up at: the longest side
/// MOST_SIDE at most.
pub fn picture_size(w: f64, h: f64, scale: f64) -> (u32, u32, f64) {
    let longest = w.max(h) * scale;
    let scale = if longest > MOST_SIDE {
        scale * MOST_SIDE / longest
    } else {
        scale
    };
    let side = |css: f64| ((css * scale).round() as u32).clamp(1, MOST_SIDE as u32);
    (side(w), side(h), scale)
}

/// Pictures a full-page screenshot comes in at most: a taller page is cut after them.
pub const MOST_PIECES: usize = 5;

/// Tiles a full-page screenshot scrolls through at most, whatever the viewport's height.
pub const MOST_TILES: usize = 40;

/// How a full page is taken: tiles of it, one viewport each, drawn into pictures of at most
/// MOST_SIDE px.
#[derive(Debug, PartialEq)]
pub struct Plan {
    /// Where the page is scrolled to for each tile, CSS px.
    pub tiles: Vec<f64>,
    /// Each picture's top and height on the page, CSS px.
    pub pieces: Vec<(f64, f64)>,
    /// The page goes on past the last picture.
    pub cut: bool,
}

/// The tiles and pictures for a page `page_h` CSS px tall under a `view` (w, h) viewport at `scale`.
pub fn plan_page(page_h: f64, view: (f64, f64), scale: f64) -> Plan {
    let (view_w, view_h) = (view.0.max(1.0), view.1.max(1.0));
    let page_h = page_h.max(view_h);
    let (_, _, scale) = picture_size(view_w, view_h, scale);
    let piece = (MOST_SIDE / scale).floor().max(1.0);
    let total = page_h
        .min(piece * MOST_PIECES as f64)
        .min(view_h * MOST_TILES as f64);
    let pieces = (0..(total / piece).ceil() as usize)
        .map(|i| (i as f64 * piece, piece.min(total - i as f64 * piece)))
        .collect();
    let last = page_h - view_h;
    let tiles = (0..(total / view_h).ceil() as usize)
        .map(|i| (i as f64 * view_h).min(last))
        .collect();
    Plan {
        tiles,
        pieces,
        cut: page_h > total,
    }
}

/// Rows of a tile drawn on a picture, in the picture's pixels.
#[derive(Debug, PartialEq)]
pub struct Cut {
    pub tile: usize,
    /// The first row in the tile, from its top.
    pub src: u32,
    /// The first row in the picture.
    pub dst: u32,
    pub rows: u32,
}

/// The rows of each tile (scrolled to `tiles`, `view_h` tall) that make the picture `piece`
/// (top, height; CSS px) at `scale`: what an earlier tile showed already is left out, so a fixed
/// header seen in the first tile isn't drawn again where the next one overlaps it.
pub fn cuts(tiles: &[f64], view_h: f64, piece: (f64, f64), scale: f64) -> Vec<Cut> {
    let px = |css: f64| (css * scale).round().max(0.0) as u32;
    let mut done = f64::MIN;
    let mut out = Vec::new();
    for (tile, &y) in tiles.iter().enumerate() {
        let lo = done.max(piece.0).max(y);
        let hi = (y + view_h).min(piece.0 + piece.1);
        done = done.max(y + view_h);
        if hi > lo {
            let (dst, end) = (px(lo - piece.0), px(hi - piece.0));
            out.push(Cut {
                tile,
                src: px(lo - y),
                dst,
                rows: end.saturating_sub(dst),
            });
        }
    }
    out
}

/// The top left of a ref's tag `tag` (w, h) big, for its box's top left `at`, on a picture
/// `size` big (all in its pixels): above the box where there's room, else on it; moved in from
/// an edge whole, as a tag cut there reads as another ref (e12 as e1).
pub fn place_tag(at: (f64, f64), tag: (f64, f64), size: (f64, f64)) -> (f64, f64) {
    let y = if at.1 >= tag.1 { at.1 - tag.1 } else { at.1 };
    let inside = |v: f64, len: f64, room: f64| v.min(room - len).max(0.0);
    (inside(at.0, tag.0, size.0), inside(y, tag.1, size.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screenshot_is_its_css_size_at_its_scale_and_never_past_2000_px() {
        assert_eq!(picture_size(1280.0, 800.0, 1.0), (1280, 800, 1.0));
        assert_eq!(picture_size(1280.0, 800.0, 0.5), (640, 400, 0.5));
        let (w, h, scale) = picture_size(1280.0, 800.0, 2.0);
        assert_eq!((w, h), (2000, 1250));
        assert!((scale - 1.5625).abs() < 1e-9);
        // A tall clip is held by its height.
        assert_eq!(picture_size(300.0, 5000.0, 1.0).1, 2000);
        assert_eq!(picture_size(0.2, 0.2, 0.1), (1, 1, 0.1), "never no pixels");
    }

    #[test]
    fn a_page_is_tiles_of_the_viewport_in_pictures_of_at_most_2000_px() {
        // Shorter than the viewport: one tile, one picture.
        let plan = plan_page(500.0, (1280.0, 800.0), 1.0);
        assert_eq!(plan.tiles, [0.0]);
        assert_eq!(plan.pieces, [(0.0, 800.0)]);
        // 4100 px: the last tile is held to the page's end, the pictures break at 2000.
        let plan = plan_page(4100.0, (1280.0, 800.0), 1.0);
        assert_eq!(plan.tiles, [0.0, 800.0, 1600.0, 2400.0, 3200.0, 3300.0]);
        assert_eq!(
            plan.pieces,
            [(0.0, 2000.0), (2000.0, 2000.0), (4000.0, 100.0)]
        );
        assert!(!plan.cut);
        // A scale of 0.5 fits twice the page in a picture.
        assert_eq!(plan_page(4100.0, (1280.0, 800.0), 0.5).pieces.len(), 2);
    }

    #[test]
    fn a_page_taller_than_the_pictures_allowed_is_cut() {
        let plan = plan_page(50_000.0, (1280.0, 800.0), 1.0);
        assert_eq!(plan.pieces.len(), MOST_PIECES);
        assert_eq!(plan.pieces.last(), Some(&(8000.0, 2000.0)));
        assert!(plan.cut);
        // Tiles stop where the pictures do.
        assert_eq!(plan.tiles.len(), 13);
        assert_eq!(plan.tiles.last(), Some(&9600.0));
        // A short viewport is held to its tiles: 40 of 200 px.
        let short = plan_page(50_000.0, (1280.0, 200.0), 1.0);
        assert_eq!(
            (short.tiles.len(), short.pieces.len(), short.cut),
            (40, 4, true)
        );
        assert_eq!(short.pieces.last(), Some(&(6000.0, 2000.0)));
        // A wide viewport shrinks the scale, so a picture takes more of the page.
        let wide = plan_page(50_000.0, (4000.0, 800.0), 1.0);
        assert_eq!(wide.pieces[0], (0.0, 4000.0));
    }

    #[test]
    fn a_picture_is_the_new_rows_of_each_tile_it_covers() {
        let tiles = [0.0, 800.0, 1200.0];
        // Tile 2 was held to the page's end (2000 px tall) and overlaps tile 1 by 400.
        let first = cuts(&tiles, 800.0, (0.0, 2000.0), 1.0);
        assert_eq!(
            first,
            [
                Cut {
                    tile: 0,
                    src: 0,
                    dst: 0,
                    rows: 800
                },
                Cut {
                    tile: 1,
                    src: 0,
                    dst: 800,
                    rows: 800
                },
                Cut {
                    tile: 2,
                    src: 400,
                    dst: 1600,
                    rows: 400
                },
            ]
        );
        // The second picture starts inside tile 1: only what's below its top.
        let second = cuts(&tiles, 800.0, (1000.0, 1000.0), 1.0);
        assert_eq!(
            second,
            [
                Cut {
                    tile: 1,
                    src: 200,
                    dst: 0,
                    rows: 600
                },
                Cut {
                    tile: 2,
                    src: 400,
                    dst: 600,
                    rows: 400
                },
            ]
        );
        // At half the scale the rows are halved, edges rounded alike.
        let half = cuts(&tiles, 800.0, (0.0, 2000.0), 0.5);
        assert_eq!(half.iter().map(|c| c.rows).sum::<u32>(), 1000);
    }

    #[test]
    fn a_tag_sits_above_its_box_where_there_is_room() {
        assert_eq!(
            place_tag((20.0, 30.0), (19.0, 14.0), (200.0, 100.0)),
            (20.0, 16.0)
        );
        // No room above: on the box's top.
        assert_eq!(
            place_tag((20.0, 5.0), (19.0, 14.0), (200.0, 100.0)),
            (20.0, 5.0)
        );
    }

    #[test]
    fn a_label_at_the_right_edge_shows_its_whole_ref() {
        // A 19 px tag from x 8 on a 20 px picture: moved left until all of "e12" shows.
        let (x, _) = place_tag((8.0, 12.0), (19.0, 10.0), (20.0, 30.0));
        assert_eq!(x, 1.0);
        // Past the bottom and left edges too; on a picture smaller than the tag, from its corner.
        assert_eq!(
            place_tag((-5.0, 95.0), (19.0, 14.0), (200.0, 100.0)),
            (0.0, 81.0)
        );
        assert_eq!(
            place_tag((5.0, 5.0), (19.0, 14.0), (10.0, 10.0)),
            (0.0, 0.0)
        );
    }
}
