//! The sidebars' and the terminal's sizes in pixels, as an IDE keeps them: what the user dragged
//! is saved, a window too small for it squeezes what's drawn, and the saved size comes back with
//! the room. Nothing here opens or closes a panel. Pure, so it's tested without a window.

use gitviber_ui::state::PanelSizes;

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
/// The terminal's first height, a share of the column; kept in points from then on, so a window
/// resize doesn't change its rows under a running program.
pub const BOTTOM_BASE_SHARE: f32 = 0.35;
pub const BOTTOM_MIN: f32 = 100.;
/// The code view keeps at least this above the terminal.
pub const EDITOR_MIN: f32 = 120.;

impl Limits {
    fn clamp(self, size: f32, total: f32) -> f32 {
        size.clamp(self.min, (self.max_share * total).max(self.min))
    }

    /// The widest it may be set beside the other sidebar drawn `other` wide: the code view keeps
    /// its min and the other side its width.
    fn widest(self, total: f32, other: f32) -> f32 {
        (self.max_share * total)
            .min(total - CENTER_MIN - other)
            .max(self.min)
    }
}

/// The sizes a launch draws from: one that can't be drawn (≤ 0, only from a corrupt state.json)
/// counts as never set, so the panel opens at its base size, not its min.
pub fn saved(mut sizes: PanelSizes) -> PanelSizes {
    for size in [
        &mut sizes.left.size,
        &mut sizes.right.size,
        &mut sizes.bottom.size,
    ] {
        *size = size.filter(|s| s.is_finite() && *s > 0.);
    }
    sizes
}

/// A drag of a sidebar's edge to `want` pixels beside the other one drawn `other` wide: None
/// closes it (pulled under half its min), else the size within its limits, in whole points so
/// its edge lands on a pixel.
pub fn dragged(want: f32, limits: Limits, total: f32, other: f32) -> Option<f32> {
    if want < limits.min / 2. {
        None
    } else {
        Some(want.round().clamp(limits.min, limits.widest(total, other)))
    }
}

/// A sidebar's divider double-clicked: as wide as its header needs to show all its tabs (`spare`
/// is the room their row has at `drawn` past them, less than zero when they're cut off), at least
/// its base size, beside the other one drawn `other` wide.
pub fn fit_header(limits: Limits, drawn: f32, spare: f32, total: f32, other: f32) -> f32 {
    (drawn - spare)
        .max(limits.base)
        .clamp(limits.min, limits.widest(total, other))
}

/// The sidebars' widths as drawn in a window `total` wide, from their saved sizes (None: closed,
/// Some(None): open at the base size). The code view gives way first, then the right sidebar,
/// then the left, down to their mins; one that still doesn't fit isn't drawn, and comes back
/// with the room, as nothing saved changes here.
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

/// The terminal's height the first time it shows, or after a double click on its edge.
pub fn bottom_base(column: f32) -> f32 {
    fit_bottom(column, (column * BOTTOM_BASE_SHARE).round())
}

/// The terminal's height as drawn in a column `total` tall, `want` as saved.
pub fn fit_bottom(total: f32, want: f32) -> f32 {
    let max = (total - EDITOR_MIN).max(BOTTOM_MIN);
    want.clamp(BOTTOM_MIN, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitviber_ui::state::PanelSize;

    /// xorshift64: the same "random" cases every run, without a crate for it.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        /// In `lo..hi`.
        fn range(&mut self, lo: f32, hi: f32) -> f32 {
            lo + (self.next() % 1_000_000) as f32 / 1_000_000. * (hi - lo)
        }
    }

    fn stored(size: f32) -> PanelSize {
        PanelSize {
            size: Some(size),
            collapsed: false,
        }
    }

    #[test]
    fn a_saved_size_that_cant_be_drawn_opens_at_the_base() {
        let s = saved(PanelSizes {
            left: stored(-50.),
            right: stored(0.),
            bottom: stored(f32::NAN),
        });
        assert_eq!(
            (s.left.size, s.right.size, s.bottom.size),
            (None, None, None)
        );
        assert_eq!(
            fit_sides(1480., Some(s.left.size), Some(s.right.size)),
            (Some(LEFT.base), Some(RIGHT.base))
        );
        let s = saved(PanelSizes {
            left: stored(400.),
            bottom: stored(f32::INFINITY),
            ..PanelSizes::default()
        });
        assert_eq!((s.left.size, s.bottom.size), (Some(400.), None));
    }

    /// Whatever state.json held, a panel opens drawable: at least its min, finite, and the code
    /// view keeps its room.
    #[test]
    fn any_stored_size_opens_positive() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut sizes = vec![
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MIN,
            f32::MAX,
            -1e30,
            -50.,
            -0.,
            0.,
            f32::MIN_POSITIVE,
            1.,
            239.,
            240.,
            5000.,
        ];
        sizes.extend((0..200).map(|_| rng.range(-1e4, 1e4)));
        for &size in &sizes {
            let s = saved(PanelSizes {
                left: stored(size),
                right: stored(size),
                bottom: stored(size),
            });
            for total in [900., 1200., 1480., 3008., 6000.] {
                let (l, r) = fit_sides(total, Some(s.left.size), Some(s.right.size));
                for (w, min) in [(l, LEFT.min), (r, RIGHT.min)] {
                    if let Some(w) = w {
                        assert!(w.is_finite() && w >= min, "{size} at {total}: {w}");
                    }
                }
                let sides = l.unwrap_or(0.) + r.unwrap_or(0.);
                assert!(
                    sides <= total - CENTER_MIN + 0.01,
                    "{size} at {total}: {l:?} {r:?}"
                );
                let column = total * 0.6;
                let h = fit_bottom(column, s.bottom.size.unwrap_or_else(|| bottom_base(column)));
                assert!(h.is_finite() && h >= BOTTOM_MIN, "{size} at {column}: {h}");
            }
        }
    }

    #[test]
    fn a_fitted_header_is_never_under_the_base_nor_past_the_room() {
        let mut rng = Rng(42);
        for _ in 0..2000 {
            let limits = if rng.next().is_multiple_of(2) { LEFT } else { RIGHT };
            let total = rng.range(900., 4000.);
            let other = rng.range(0., 0.4 * total);
            let drawn = rng.range(limits.min, total);
            let spare = rng.range(-2000., 2000.);
            let fit = fit_header(limits, drawn, spare, total, other);
            let widest = limits.widest(total, other);
            assert!(fit >= limits.min && fit <= widest, "{fit} of {widest}");
            if widest >= limits.base {
                assert!(fit >= limits.base, "{fit} under the base");
            }
            // The tabs' row needs drawn - spare; it gets that when there's room for it.
            let need = drawn - spare;
            if need >= limits.base && need <= widest {
                assert_eq!(fit, need);
            }
        }
    }

    /// A session of resizes, drags and toggles: what's drawn stays inside the limits, a closed
    /// side is never drawn, and the same state draws the same.
    #[test]
    fn random_resizes_drags_and_toggles_keep_the_layout_drawable() {
        let mut rng = Rng(7);
        let (mut left, mut right) = (None::<f32>, None::<f32>);
        let (mut left_open, mut right_open) = (true, true);
        let mut total = 1480.;
        for step in 0..5000 {
            let drawn = fit_sides(
                total,
                left_open.then_some(left),
                right_open.then_some(right),
            );
            match rng.next() % 5 {
                0 => total = rng.range(900., 6000.),
                1 => left_open = !left_open,
                2 => right_open = !right_open,
                3 if drawn.0.is_some() => {
                    let want = rng.range(0., total);
                    match dragged(want, LEFT, total, drawn.1.unwrap_or(0.)) {
                        Some(w) => left = Some(w),
                        None => left_open = false,
                    }
                }
                4 if drawn.1.is_some() => {
                    let want = rng.range(0., total);
                    match dragged(want, RIGHT, total, drawn.0.unwrap_or(0.)) {
                        Some(w) => right = Some(w),
                        None => right_open = false,
                    }
                }
                _ => {}
            }
            let (l, r) = fit_sides(
                total,
                left_open.then_some(left),
                right_open.then_some(right),
            );
            assert!(left_open || l.is_none(), "step {step}");
            assert!(right_open || r.is_none(), "step {step}");
            if let Some(l) = l {
                assert!(l >= LEFT.min && l <= (LEFT.max_share * total).max(LEFT.min));
            }
            if let Some(r) = r {
                assert!(r >= RIGHT.min && r <= (RIGHT.max_share * total).max(RIGHT.min));
            }
            let sides = l.unwrap_or(0.) + r.unwrap_or(0.);
            assert!(sides <= total - CENTER_MIN + 0.01, "step {step}");
            // Drawn again as it was: nothing here keeps state of its own.
            let again = fit_sides(
                total,
                left_open.then_some(left),
                right_open.then_some(right),
            );
            assert_eq!(again, (l, r));
        }
    }

    #[test]
    fn saved_sizes_draw_as_saved_with_room() {
        assert_eq!(
            fit_sides(1480., Some(Some(320.)), Some(Some(260.))),
            (Some(320.), Some(260.))
        );
        assert_eq!(fit_sides(1480., Some(None), None), (Some(320.), None));
    }

    #[test]
    fn a_double_click_fits_the_header_from_the_base_up() {
        // Tabs cut off by 60 at 280: 340. Room to spare: narrower, down to the base.
        assert_eq!(fit_header(LEFT, 280., -60., 1480., 260.), 340.);
        assert_eq!(fit_header(LEFT, 500., 100., 1480., 260.), 400.);
        assert_eq!(fit_header(RIGHT, 300., 200., 1480., 320.), 260.);
        assert_eq!(fit_header(LEFT, 400., -300., 1000., 0.), 450.);
        // Never into the other sidebar's room: 1000 - 360 - 260.
        assert_eq!(fit_header(LEFT, 400., -300., 1000., 260.), 380.);
    }

    #[test]
    fn sizes_stay_inside_their_limits() {
        assert_eq!(fit_sides(1480., Some(Some(50.)), None).0, Some(240.));
        assert_eq!(fit_sides(1000., Some(Some(900.)), None).0, Some(450.));
    }

    #[test]
    fn a_shrinking_window_squeezes_the_center_then_right_then_left() {
        // 1200 wide: 840 for both, the code view alone gives way.
        assert_eq!(
            fit_sides(1200., Some(Some(400.)), Some(Some(300.))),
            (Some(400.), Some(300.))
        );
        // 900 wide: 540 for both sidebars.
        let (l, r) = fit_sides(900., Some(Some(320.)), Some(Some(260.)));
        assert_eq!(l, Some(320.));
        assert_eq!(r, Some(220.));
        let (l, r) = fit_sides(900., Some(Some(400.)), Some(Some(300.)));
        assert_eq!((l, r), (Some(340.), Some(200.)));
        // 700 wide: 340 left over; the right can't keep its min next to the left's, and the left
        // stops at its 45% share.
        let (l, r) = fit_sides(700., Some(Some(320.)), Some(Some(260.)));
        assert_eq!((l, r), (Some(315.), None));
        assert_eq!(fit_sides(500., Some(Some(320.)), None), (None, None));
        // With the room back, the saved sizes are drawn again.
        assert_eq!(
            fit_sides(2400., Some(Some(400.)), Some(Some(300.))),
            (Some(400.), Some(300.))
        );
    }

    #[test]
    fn a_closed_side_stays_closed_at_any_width() {
        for total in [900., 1480., 3000., 6000.] {
            assert_eq!(fit_sides(total, None, Some(Some(260.))), (None, Some(260.)));
            assert_eq!(fit_sides(total, Some(Some(320.)), None), (Some(320.), None));
        }
    }

    #[test]
    fn dragging_under_half_the_min_closes() {
        assert_eq!(dragged(100., LEFT, 1480., 0.), None);
        assert_eq!(dragged(200., LEFT, 1480., 0.), Some(240.));
        assert_eq!(dragged(499.76, LEFT, 1480., 0.), Some(500.));
        assert_eq!(dragged(2000., LEFT, 1480., 0.), Some(666.));
    }

    #[test]
    fn a_drag_stops_where_the_other_side_and_the_code_view_begin() {
        // 1000 wide, the right drawn 260: 380 for the left, under its 450 share.
        assert_eq!(dragged(600., LEFT, 1000., 260.), Some(380.));
        assert_eq!(dragged(600., RIGHT, 1000., 320.), Some(320.));
        // No room past the min: the min.
        assert_eq!(dragged(600., LEFT, 900., 400.), Some(240.));
    }

    #[test]
    fn the_terminal_keeps_the_editor_room() {
        assert_eq!(bottom_base(800.), 280.);
        assert_eq!(fit_bottom(800., 750.), 680.);
        assert_eq!(fit_bottom(800., 10.), 100.);
        assert_eq!(fit_bottom(150., 300.), 100.);
        // Kept in points: a taller column draws it as it was.
        assert_eq!(fit_bottom(1400., 280.), 280.);
    }
}
