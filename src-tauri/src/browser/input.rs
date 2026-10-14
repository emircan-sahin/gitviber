//! Trusted input for an agent's tab: the mouse and wheel events a command becomes, and where on
//! the view they land, apart from AppKit (macos/native.rs makes them NSEvents) so it can be
//! tested. Points are the page's CSS px from its viewport's top left.

/// A wheel event's failure where AppKit won't make one for the view's window: the page scrolls
/// by script instead.
pub const NO_WHEEL: &str = "No native wheel event here.";

/// What one native pointer event does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pointer {
    Move,
    /// A press; `clicks` is the click count (2 for the second press of a double click).
    Down {
        right: bool,
        clicks: u8,
    },
    Up {
        right: bool,
        clicks: u8,
    },
    /// A move with the left button held.
    Drag,
    /// A wheel turn, in px: up is negative.
    Wheel {
        dy: f64,
    },
}

impl Pointer {
    /// The buttons held while WebKit reads it (`NSEvent.pressedMouseButtons`): pointer events
    /// carry them as `buttons`, and a drag handler checks them.
    pub fn buttons(self) -> usize {
        match self {
            Self::Down { right: false, .. } | Self::Drag => 1,
            Self::Down { right: true, .. } => 2,
            _ => 0,
        }
    }
}

/// A click's events at one point: a move there first, as a mouse gets there, then each press.
pub fn clicks(count: u8, right: bool) -> Vec<Pointer> {
    let mut out = vec![Pointer::Move];
    for clicks in 1..=count.max(1) {
        out.push(Pointer::Down { right, clicks });
        out.push(Pointer::Up { right, clicks });
    }
    out
}

/// How many moves a drag takes from one end to the other: enough for a sortable list's
/// threshold and a slider's steps, few enough to stay quick.
const DRAG_STEPS: usize = 10;

/// A drag from `a` to `b`: pressed at `a`, moved there with the button held, let go at `b`.
pub fn drag(a: (f64, f64), b: (f64, f64)) -> Vec<(Pointer, (f64, f64))> {
    let mut out = vec![
        (Pointer::Move, a),
        (
            Pointer::Down {
                right: false,
                clicks: 1,
            },
            a,
        ),
    ];
    for step in 1..=DRAG_STEPS {
        let t = step as f64 / DRAG_STEPS as f64;
        out.push((
            Pointer::Drag,
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t),
        ));
    }
    out.push((
        Pointer::Up {
            right: false,
            clicks: 1,
        },
        b,
    ));
    out
}

/// The most one wheel event turns, as a mouse's notches do: a page that eases or snaps its own
/// scrolling sees several.
const WHEEL_STEP: u32 = 120;

/// The wheel events that scroll `px` down (up when `up`).
pub fn wheel(px: u32, up: bool) -> Vec<Pointer> {
    let sign = if up { -1.0 } else { 1.0 };
    let mut left = px;
    let mut out = vec![];
    while left > 0 {
        let step = left.min(WHEEL_STEP);
        out.push(Pointer::Wheel {
            dy: sign * f64::from(step),
        });
        left -= step;
    }
    out
}

/// A point in the page's CSS px, in the view's points: CSS px times the page's zoom, from the
/// top left; up from the bottom in a view that isn't flipped.
pub fn to_view(at: (f64, f64), zoom: f64, height: f64, flipped: bool) -> (f64, f64) {
    let (x, y) = (at.0 * zoom, at.1 * zoom);
    (x, if flipped { y } else { height - y })
}

/// `412,300` as a point; None for anything else (a ref, a selector).
pub fn point(s: &str) -> Option<(f64, f64)> {
    let (x, y) = s.split_once(',')?;
    let n = |s: &str| {
        let s = s.trim();
        let digits = !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.');
        digits.then(|| s.parse::<f64>().ok()).flatten()
    };
    Some((n(x)?, n(y)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_moves_there_first_and_a_double_click_counts_its_presses() {
        let down = |clicks| Pointer::Down {
            right: false,
            clicks,
        };
        let up = |clicks| Pointer::Up {
            right: false,
            clicks,
        };
        assert_eq!(clicks(1, false), [Pointer::Move, down(1), up(1)]);
        assert_eq!(
            clicks(2, false),
            [Pointer::Move, down(1), up(1), down(2), up(2)]
        );
        assert_eq!(
            clicks(1, true)[1],
            Pointer::Down {
                right: true,
                clicks: 1
            }
        );
        // Pressed, the button is held as WebKit reads it; let go or moving, none is.
        assert_eq!(down(1).buttons(), 1);
        assert_eq!(clicks(1, true)[1].buttons(), 2);
        assert_eq!(up(1).buttons(), 0);
        assert_eq!(Pointer::Move.buttons(), 0);
    }

    #[test]
    fn a_drag_holds_the_button_on_every_move_and_ends_where_it_was_going() {
        let steps = drag((10.0, 20.0), (110.0, 220.0));
        assert_eq!(steps[0], (Pointer::Move, (10.0, 20.0)));
        assert_eq!(steps[1].0.buttons(), 1);
        let moves: Vec<_> = steps.iter().filter(|s| s.0 == Pointer::Drag).collect();
        assert_eq!(moves.len(), DRAG_STEPS);
        assert!(moves.iter().all(|m| m.0.buttons() == 1));
        assert_eq!(moves[0].1, (20.0, 40.0));
        assert_eq!(moves.last().unwrap().1, (110.0, 220.0));
        assert_eq!(
            steps.last(),
            Some(&(
                Pointer::Up {
                    right: false,
                    clicks: 1
                },
                (110.0, 220.0)
            ))
        );
    }

    #[test]
    fn the_wheel_turns_in_notches_that_add_up_to_the_distance() {
        let dys = |p: Vec<Pointer>| -> Vec<f64> {
            p.into_iter()
                .map(|w| match w {
                    Pointer::Wheel { dy } => dy,
                    _ => panic!("{w:?}"),
                })
                .collect()
        };
        assert_eq!(dys(wheel(300, false)), [120.0, 120.0, 60.0]);
        assert_eq!(dys(wheel(50, true)), [-50.0]);
        assert!(wheel(0, false).is_empty());
    }

    #[test]
    fn css_px_land_on_the_view_by_the_pages_zoom_either_way_up() {
        assert_eq!(to_view((100.0, 40.0), 1.0, 800.0, true), (100.0, 40.0));
        assert_eq!(to_view((100.0, 40.0), 1.0, 800.0, false), (100.0, 760.0));
        // Device mode zooms a phone's 390 px into a narrower tab.
        assert_eq!(to_view((100.0, 40.0), 0.5, 800.0, true), (50.0, 20.0));
        assert_eq!(to_view((100.0, 40.0), 1.5, 800.0, false), (150.0, 740.0));
    }

    #[test]
    fn a_point_is_two_numbers_and_anything_else_is_a_ref_or_selector() {
        assert_eq!(point("412,300"), Some((412.0, 300.0)));
        assert_eq!(point(" 12.5 , 7 "), Some((12.5, 7.0)));
        for no in ["e3", "a, b", "-1,2", "1,", ",2", "1,2,3", "1e3,2", "#x"] {
            assert_eq!(point(no), None, "{no}");
        }
    }
}
