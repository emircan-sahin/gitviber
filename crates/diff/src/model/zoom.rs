//! How an SVG preview is zoomed and moved: fitted to its room, or `scale` view pixels per SVG
//! pixel with the point (u, v) of the drawing, as fractions of its size, held at the room's
//! center. The wheel zooms around the pointer, a drag moves the drawing, and neither lets it
//! leave a gap at an edge it could fill.

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
}

impl Frame {
    /// The scale that fits the drawing inside the room's margins.
    pub fn fit(&self) -> f32 {
        let (rw, rh) = self.room;
        let (nw, nh) = self.natural;
        ((rw - 2. * PAD) / nw).min((rh - 2. * PAD) / nh).max(0.)
    }

    /// How far it may zoom out and in.
    pub fn limits(&self) -> (f32, f32) {
        let fit = self.fit();
        let min = fit.min(1.) / 2.;
        let max = (fit.max(1.) * 16.).min(MAX_DRAWN / self.natural.0.max(self.natural.1));
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
        let (min, max) = self.limits();
        let from = self.scale(z);
        let to = (from * factor).clamp(min, max);
        let (nw, nh) = self.natural;
        Zoom {
            scale: Some(to),
            u: zoom_axis(self.room.0, nw * from, nw * to, z.u, at.0),
            v: zoom_axis(self.room.1, nh * from, nh * to, z.v, at.1),
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
}
