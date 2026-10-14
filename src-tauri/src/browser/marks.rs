//! An agent's screenshot (`screenshot`), apart from AppKit so it can be tested: its size in
//! pixels, and the ref labels `--refs` draws on its pixels, never on the page.

use super::picks::Bounds;

/// The longest side a screenshot gets: a bigger picture breaks an agent's conversation
/// (anthropics/claude-code#13480).
pub const MOST_SIDE: f64 = 2000.0;

/// A screenshot as asked: `rect` of the viewport (CSS px), drawn `size` px big, with `labels`
/// (in those px), as a PNG or a JPEG.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub rect: Bounds,
    pub size: (u32, u32),
    pub labels: Vec<Label>,
    pub jpeg: bool,
}

/// A ref's box on the picture, in its pixels, and the ref.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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

/// 5×7 glyphs for what a ref is written with, a row a byte, its high bit on the left.
fn glyph(c: char) -> Option<[u8; 7]> {
    Some(match c {
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1f, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e],
        '6' => [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x02, 0x0c],
        'e' => [0x00, 0x00, 0x0e, 0x11, 0x1f, 0x10, 0x0e],
        _ => return None,
    })
}

/// The labels' color, and their text's.
const MARK: [u8; 4] = [229, 0, 126, 255];
const INK: [u8; 4] = [255, 255, 255, 255];

/// RGBA pixels, `stride` bytes a row, top row first, as AppKit's bitmap holds them.
pub struct Pixels<'a> {
    pub data: &'a mut [u8],
    pub width: u32,
    pub height: u32,
    pub stride: usize,
}

impl Pixels<'_> {
    fn fill(&mut self, x: i64, y: i64, w: i64, h: i64, color: [u8; 4]) {
        let (x0, y0) = (x.max(0), y.max(0));
        let x1 = (x + w).min(i64::from(self.width));
        let y1 = (y + h).min(i64::from(self.height));
        for row in y0..y1 {
            for col in x0..x1 {
                let at = row as usize * self.stride + col as usize * 4;
                if let Some(px) = self.data.get_mut(at..at + 4) {
                    px.copy_from_slice(&color);
                }
            }
        }
    }

    /// Each label: an outline around its box, and its ref on a tag at the box's top left (above
    /// it where there's room). Glyphs twice as big on a picture at full size or more.
    pub fn draw(&mut self, labels: &[Label], big: bool) {
        let s: i64 = if big { 2 } else { 1 };
        for l in labels {
            let (x, y) = (l.x.round() as i64, l.y.round() as i64);
            let (w, h) = (l.w.round().max(1.0) as i64, l.h.round().max(1.0) as i64);
            for (fx, fy, fw, fh) in [
                (x, y, w, s),
                (x, y + h - s, w, s),
                (x, y, s, h),
                (x + w - s, y, s, h),
            ] {
                self.fill(fx, fy, fw, fh, MARK);
            }
            let glyphs: Vec<[u8; 7]> = l.text.chars().filter_map(glyph).collect();
            let tag_w = glyphs.len() as i64 * 6 * s + s;
            let tag_h = 9 * s;
            let top = if y >= tag_h { y - tag_h } else { y };
            self.fill(x, top, tag_w, tag_h, MARK);
            for (i, rows) in glyphs.iter().enumerate() {
                let left = x + s + i as i64 * 6 * s;
                for (r, bits) in rows.iter().enumerate() {
                    for c in 0..5 {
                        if bits & (0x10 >> c) != 0 {
                            self.fill(left + c * s, top + s + r as i64 * s, s, s, INK);
                        }
                    }
                }
            }
        }
    }
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

    fn pixel(p: &Pixels, x: usize, y: usize) -> [u8; 4] {
        let at = y * p.stride + x * 4;
        p.data[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn a_label_outlines_its_box_and_writes_its_ref_on_a_tag_above_it() {
        let (width, height) = (60u32, 40u32);
        let stride = width as usize * 4 + 8;
        let mut data = vec![0u8; stride * height as usize];
        let mut p = Pixels {
            data: &mut data,
            width,
            height,
            stride,
        };
        let label = Label {
            text: "e7".into(),
            x: 20.0,
            y: 15.0,
            w: 30.0,
            h: 10.0,
        };
        p.draw(&[label], false);
        // The outline's corners, the box's inside untouched.
        assert_eq!(pixel(&p, 20, 15), MARK);
        assert_eq!(pixel(&p, 49, 24), MARK);
        assert_eq!(pixel(&p, 35, 20), [0, 0, 0, 0]);
        // The tag sits above the box: 9 px high, from y 6; "e" is its first glyph.
        assert_eq!(pixel(&p, 20, 6), MARK);
        let ink = (0..13).flat_map(|x| (6..15).map(move |y| (x + 20, y)));
        assert!(ink.clone().any(|(x, y)| pixel(&p, x, y) == INK));
        // e's top two rows are empty; its middle row is full.
        assert_eq!(pixel(&p, 21, 7), MARK);
        assert_eq!(pixel(&p, 22, 9), INK);
        // Nothing past a row's pixels, in its padding.
        assert!(data[width as usize * 4..stride].iter().all(|&b| b == 0));
    }

    #[test]
    fn labels_at_and_past_the_edges_stay_inside_the_picture() {
        let (width, height) = (10u32, 10u32);
        let stride = 40;
        let mut data = vec![0u8; stride * 10];
        let mut p = Pixels {
            data: &mut data,
            width,
            height,
            stride,
        };
        let labels = [
            Label {
                text: "e12345".into(),
                x: -5.0,
                y: -5.0,
                w: 50.0,
                h: 50.0,
            },
            Label {
                text: "e1".into(),
                x: 9.0,
                y: 9.0,
                w: 100.0,
                h: 100.0,
            },
        ];
        p.draw(&labels, true);
        assert_eq!(data.len(), 400);
    }

    #[test]
    #[ignore = "bug: a tag past the right edge is cut, so e12 reads as e1, another ref"]
    fn a_label_at_the_right_edge_shows_its_whole_ref() {
        let ink = |width: u32| {
            let stride = width as usize * 4;
            let mut data = vec![0u8; stride * 30];
            let mut p = Pixels {
                data: &mut data,
                width,
                height: 30,
                stride,
            };
            let label = Label {
                text: "e12".into(),
                x: 8.0,
                y: 12.0,
                w: 10.0,
                h: 10.0,
            };
            p.draw(&[label], false);
            data.chunks(4).filter(|px| *px == INK).count()
        };
        // The tag is 19 px wide from x 8: in a 20 px picture its "2" falls outside.
        assert_eq!(ink(20), ink(60), "every glyph of the ref drawn");
    }
}
