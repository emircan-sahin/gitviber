//! The sidebars' and the terminal's sizes in pixels, as an IDE keeps them: what the user dragged
//! is saved, a window too small for it squeezes what's drawn, and the saved size comes back with
//! the room. Pure, so it's tested without a window.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Its size until the user sets one.
    pub base: f32,
    pub min: f32,
    /// A share of the window.
    pub max_share: f32,
}

pub const LEFT: Limits = Limits {
    base: 320.,
    min: 240.,
    max_share: 0.45,
};
pub const RIGHT: Limits = Limits {
    base: 260.,
    min: 200.,
    max_share: 0.40,
};
/// The code view keeps at least this between the sidebars.
pub const CENTER_MIN: f32 = 360.;
/// The terminal: a share of the column until set, and at least this tall.
pub const BOTTOM_BASE_SHARE: f32 = 0.35;
pub const BOTTOM_MIN: f32 = 100.;
/// The code view keeps at least this above the terminal.
pub const EDITOR_MIN: f32 = 120.;

impl Limits {
    fn clamp(self, size: f32, total: f32) -> f32 {
        size.clamp(self.min, (self.max_share * total).max(self.min))
    }
}

/// A drag of a sidebar's edge to `want` pixels: None closes it (pulled under half its min),
/// else the size within its limits.
pub fn dragged(want: f32, limits: Limits, total: f32) -> Option<f32> {
    if want < limits.min / 2. {
        None
    } else {
        Some(limits.clamp(want, total))
    }
}

/// The sidebars' widths as drawn in a window `total` wide, from their saved sizes (None: closed,
/// Some(None): open at the base size). The right one gives way first, then the left, down to
/// their mins; one that still doesn't fit isn't drawn.
pub fn fit_sides(
    total: f32,
    left: Option<Option<f32>>,
    right: Option<Option<f32>>,
) -> (Option<f32>, Option<f32>) {
    let mut l = left.map(|s| LEFT.clamp(s.unwrap_or(LEFT.base), total));
    let mut r = right.map(|s| RIGHT.clamp(s.unwrap_or(RIGHT.base), total));
    let room = (total - CENTER_MIN).max(0.);
    let mins =
        |l: Option<f32>, r: Option<f32>| l.map_or(0., |_| LEFT.min) + r.map_or(0., |_| RIGHT.min);
    if mins(l, r) > room {
        r = None;
    }
    if mins(l, r) > room {
        l = None;
    }
    let mut over = l.unwrap_or(0.) + r.unwrap_or(0.) - room;
    for (side, min) in [(&mut r, RIGHT.min), (&mut l, LEFT.min)] {
        if let Some(w) = side.as_mut().filter(|_| over > 0.) {
            let give = (*w - min).min(over);
            *w -= give;
            over -= give;
        }
    }
    (l, r)
}

/// The terminal's height in a column `total` tall.
pub fn fit_bottom(total: f32, saved: Option<f32>) -> f32 {
    let want = saved.unwrap_or(total * BOTTOM_BASE_SHARE);
    let max = (total - EDITOR_MIN).max(BOTTOM_MIN);
    want.clamp(BOTTOM_MIN, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_sizes_draw_as_saved_with_room() {
        assert_eq!(
            fit_sides(1480., Some(Some(320.)), Some(Some(260.))),
            (Some(320.), Some(260.))
        );
        assert_eq!(fit_sides(1480., Some(None), None), (Some(320.), None));
    }

    #[test]
    fn sizes_stay_inside_their_limits() {
        assert_eq!(fit_sides(1480., Some(Some(50.)), None).0, Some(240.));
        assert_eq!(fit_sides(1000., Some(Some(900.)), None).0, Some(450.));
    }

    #[test]
    fn a_narrow_window_squeezes_right_first_then_hides() {
        // 900 wide: 540 for both sidebars.
        let (l, r) = fit_sides(900., Some(Some(320.)), Some(Some(260.)));
        assert_eq!(l, Some(320.));
        assert_eq!(r, Some(220.));
        // 700 wide: 340 left over; the right can't keep its min next to the left's, and the left
        // stops at its 45% share.
        let (l, r) = fit_sides(700., Some(Some(320.)), Some(Some(260.)));
        assert_eq!((l, r), (Some(315.), None));
        assert_eq!(fit_sides(500., Some(Some(320.)), None), (None, None));
    }

    #[test]
    fn dragging_under_half_the_min_closes() {
        assert_eq!(dragged(100., LEFT, 1480.), None);
        assert_eq!(dragged(200., LEFT, 1480.), Some(240.));
        assert_eq!(dragged(2000., LEFT, 1480.), Some(666.));
    }

    #[test]
    fn the_terminal_keeps_the_editor_room() {
        assert_eq!(fit_bottom(800., None), 280.);
        assert_eq!(fit_bottom(800., Some(750.)), 680.);
        assert_eq!(fit_bottom(800., Some(10.)), 100.);
        assert_eq!(fit_bottom(150., Some(300.)), 100.);
    }
}
