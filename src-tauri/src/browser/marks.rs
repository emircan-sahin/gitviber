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
