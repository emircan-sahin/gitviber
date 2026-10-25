//! How an SVG or image preview is zoomed and moved: fitted to its room, or `scale` view pixels
//! per pixel of the picture with the point (u, v) of it, as fractions of its size, held at the
//! room's center. Zooming keeps the point under the pointer, a drag moves the picture, and
//! neither lets it leave a gap at an edge it could fill.

/// What the zoom buttons' tooltip says the pointer does.
pub const TIP: &str = "Pinch or ⌘-scroll to zoom, drag to move, double-click for 1:1 or Fit";

/// Margin around a drawing fitted to its room.
pub const PAD: f32 = 16.;
/// The longest side zooming may draw.
const MAX_DRAWN: f32 = 16384.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    /// None: fitted.
    pub scale: Option<f32>,
    pub u: f32,
    pub v: f32,
}

pub const FIT: Zoom = Zoom {
    scale: None,
    u: 0.5,
    v: 0.5,
};

/// 1:1, centered.
pub const ACTUAL: Zoom = Zoom {
    scale: Some(1.),
    u: 0.5,
    v: 0.5,
};

impl Default for Zoom {
    fn default() -> Self {
        FIT
    }
}

/// What a drawing `natural` big is shown in: a `room` of view pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub room: (f32, f32),
    pub natural: (f32, f32),
    /// Fitting may enlarge it past its own size: a drawing's does, an image's pixels don't.
    pub grow: bool,
}

impl Frame {
    /// The scale that fills the room's margins.
    fn fill(&self) -> f32 {
        let (rw, rh) = self.room;
        let (nw, nh) = self.natural;
        ((rw - 2. * PAD) / nw).min((rh - 2. * PAD) / nh).max(0.)
    }

    /// The scale that fits the drawing inside the room's margins.
    pub fn fit(&self) -> f32 {
        let fill = self.fill();
        if self.grow { fill } else { fill.min(1.) }
    }

    /// How far it may zoom out and in: a small image as far in as a drawing that fills the
    /// room. Only a drawing is held to a longest side: an image's part on screen is all that's
    /// decoded, however far in.
    pub fn limits(&self) -> (f32, f32) {
        let fill = self.fill();
        let min = fill.min(1.) / 2.;
        let mut max = fill.max(1.) * 16.;
        if self.grow {
            max = max.min(MAX_DRAWN / self.natural.0.max(self.natural.1));
        }
        (min, min.max(max))
    }

    /// The scale `z` draws at; a file that grew under a zoom is held to the limit.
    pub fn scale(&self, z: Zoom) -> f32 {
        z.scale.unwrap_or_else(|| self.fit()).min(self.limits().1)
    }

    /// Where the drawing goes in the room at `z`: left, top, width, height.
    pub fn drawn(&self, z: Zoom) -> [f32; 4] {
        let s = self.scale(z);
        let (w, h) = (self.natural.0 * s, self.natural.1 * s);
        [place(self.room.0, w, z.u), place(self.room.1, h, z.v), w, h]
    }

    /// Bigger than the room: a drag moves it.
    pub fn pannable(&self, z: Zoom) -> bool {
        let [_, _, w, h] = self.drawn(z);
        w > self.room.0 + 0.5 || h > self.room.1 + 0.5
    }

    /// `z` zoomed by `factor`, the point at `at` (room pixels) staying where it is.
    pub fn zoomed(&self, z: Zoom, factor: f32, at: (f32, f32)) -> Zoom {
        self.zoomed_to(z, self.scale(z) * factor, at)
    }

    /// `z` at the scale `to` (within the limits), the point at `at` staying where it is.
    fn zoomed_to(&self, z: Zoom, to: f32, at: (f32, f32)) -> Zoom {
        let (min, max) = self.limits();
        let from = self.scale(z);
        let to = to.clamp(min, max);
        let (nw, nh) = self.natural;
        Zoom {
            scale: Some(to),
            u: zoom_axis(self.room.0, nw * from, nw * to, z.u, at.0),
            v: zoom_axis(self.room.1, nh * from, nh * to, z.v, at.1),
        }
    }

    /// A double-click at `at`: fitted goes to 1:1 around it, anything else back to fitted.
    pub fn toggled(&self, z: Zoom, at: (f32, f32)) -> Zoom {
        match z.scale {
            None => self.zoomed_to(z, 1., at),
            Some(_) => FIT,
        }
    }

    /// `z` with the drawing dragged `delta` room pixels.
    pub fn panned(&self, z: Zoom, delta: (f32, f32)) -> Zoom {
        let s = self.scale(z);
        let (nw, nh) = self.natural;
        Zoom {
            u: pan_axis(self.room.0, nw * s, z.u, delta.0),
            v: pan_axis(self.room.1, nh * s, z.v, delta.1),
            ..z
        }
    }
}

/// Where along a `room`-long view a drawing `size` long starts: centered if it fits, else never
/// leaving a gap.
pub fn place(room: f32, size: f32, u: f32) -> f32 {
    if size <= room {
        (room - size) / 2.
    } else {
        (room / 2. - u * size).clamp(room - size, 0.)
    }
}

/// `u` limited to what `place` can show, so dragging past an edge doesn't build up.
fn clamp_center(room: f32, size: f32, u: f32) -> f32 {
    if size <= room {
        return 0.5;
    }
    let half = room / 2. / size;
    u.clamp(half, 1. - half)
}

/// Along one axis: the center after zooming from `size` to `next`, the point at `at` kept.
fn zoom_axis(room: f32, size: f32, next: f32, u: f32, at: f32) -> f32 {
    let under = (at - place(room, size, u)) / size;
    clamp_center(room, next, (room / 2. - at) / next + under)
}

/// Along one axis: the center after dragging the drawing `delta` pixels.
fn pan_axis(room: f32, size: f32, u: f32, delta: f32) -> f32 {
    clamp_center(room, size, u - delta / size)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON: Frame = Frame {
        room: (432., 232.),
        natural: (100., 50.),
        grow: true,
    };

    #[test]
    fn fits_inside_the_margins_and_centers() {
        // 400 × 200 inside the margins: twice, wide and tall alike.
        assert_eq!(ICON.fit(), 4.);
        assert_eq!(ICON.drawn(FIT), [16., 16., 400., 200.]);
        assert_eq!(ICON.drawn(ACTUAL), [166., 91., 100., 50.]);
        assert!(!ICON.pannable(FIT));
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        // Zoomed in around the drawing's top left quarter point.
        let at = (116., 66.);
        let z = ICON.zoomed(FIT, 2., at);
        assert_eq!(z.scale, Some(8.));
        let [x, y, w, h] = ICON.drawn(z);
        assert!((x + 0.25 * w - at.0).abs() < 0.01 && (y + 0.25 * h - at.1).abs() < 0.01);
        assert!(ICON.pannable(z));
        // Held to the limits.
        assert_eq!(ICON.zoomed(FIT, 1000., at).scale, Some(64.));
        assert_eq!(ICON.zoomed(FIT, 0.001, at).scale, Some(0.5));
    }

    #[test]
    fn a_drag_moves_it_but_never_past_an_edge() {
        let z = Zoom {
            scale: Some(8.),
            ..FIT
        };
        // 800 × 400 in a 432 × 232 room: its left edge can go to 0 and no further.
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        let left = ICON.panned(z, (10_000., 0.));
        assert!(near(ICON.drawn(left)[0], 0.));
        let back = ICON.panned(left, (-10., 0.));
        assert!(near(ICON.drawn(back)[0], -10.));
        // A drawing that fits stays centered.
        assert_eq!(ICON.panned(FIT, (50., 50.)), FIT);
    }

    #[test]
    fn an_image_fits_without_growing_but_zooms_as_far() {
        let photo = Frame {
            room: (432., 232.),
            natural: (4000., 1000.),
            grow: false,
        };
        assert_eq!(photo.fit(), 0.1);
        assert_eq!(photo.limits(), (0.05, 16.));
        let icon = Frame {
            grow: false,
            ..ICON
        };
        assert_eq!(icon.fit(), 1.);
        assert_eq!(icon.drawn(FIT), ICON.drawn(ACTUAL));
        assert_eq!(icon.limits(), (0.5, 64.));
    }

    #[test]
    fn a_double_click_goes_between_fit_and_one_to_one() {
        let at = (116., 66.);
        let one = ICON.toggled(FIT, at);
        assert_eq!(one.scale, Some(1.));
        // The point under the pointer stays where it was, as far as the edges allow.
        let [x, ..] = ICON.drawn(one);
        assert_eq!(x, 166.);
        assert_eq!(ICON.toggled(one, at), FIT);
        let zoomed = ICON.zoomed(FIT, 3., at);
        assert_eq!(ICON.toggled(zoomed, at), FIT);
    }
}
