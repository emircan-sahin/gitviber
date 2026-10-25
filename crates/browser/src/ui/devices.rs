//! The devices a browser tab can show its page as (device mode), from devices.json, and where a
//! device's body, screen and page go in the tab's area: shrunk to fit, never enlarged,
//! centered, and turned on its side when rotated. As in a phone's browser, the page gets the
//! screen less its status bar and home indicator, which the tab draws.

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Insets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

/// The camera's island, notch or hole, in CSS px from the screen's top left, portrait.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Cutout {
    #[serde(rename = "type")]
    pub kind: CutoutKind,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub r: f64,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CutoutKind {
    Island,
    Notch,
    Punch,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    /// The screen in CSS px, portrait.
    pub w: f64,
    pub h: f64,
    /// 0: this Mac's own, as Responsive.
    pub dpr: f64,
    /// "": WebKit's own, as Responsive.
    pub ua: String,
    pub radius: f64,
    pub safe: Insets,
    pub cutout: Option<Cutout>,
    /// The body's edge around the screen, in CSS px.
    pub bezel: f64,
    pub platform: String,
}

impl Device {
    /// A phone or a tablet, not Responsive.
    pub fn touch(&self) -> bool {
        self.platform != "web"
    }
}

#[derive(Deserialize)]
struct Devices {
    devices: Vec<Device>,
}

pub static DEVICES: LazyLock<Vec<Device>> = LazyLock::new(|| {
    serde_json::from_str::<Devices>(include_str!("devices.json"))
        .map(|d| d.devices)
        .unwrap_or_default()
});

pub const RESPONSIVE: &str = "Responsive";
const RESPONSIVE_SIZE: (f64, f64) = (400.0, 800.0);
pub const RESPONSIVE_MIN: f64 = 200.0;
pub const RESPONSIVE_MAX: f64 = 3000.0;

/// A tab's device, saved with it: one of DEVICES by name, or Responsive at a size of its own.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct DeviceChoice {
    pub name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rotated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
}

impl DeviceChoice {
    /// Device mode's first device, until another is picked.
    pub fn default_device() -> Self {
        Self {
            name: "iPhone 16 Pro".into(),
            ..Self::default()
        }
    }
}

/// A Responsive side, whole CSS px in bounds.
pub fn clamp_side(n: f64) -> f64 {
    n.clamp(RESPONSIVE_MIN, RESPONSIVE_MAX).round()
}

/// The device a choice names; Responsive is a bare screen of its size. None for a name no
/// longer known.
pub fn device_of(choice: &DeviceChoice) -> Option<Device> {
    if choice.name != RESPONSIVE {
        return DEVICES.iter().find(|d| d.name == choice.name).cloned();
    }
    Some(Device {
        name: RESPONSIVE.into(),
        w: clamp_side(choice.w.unwrap_or(RESPONSIVE_SIZE.0)),
        h: clamp_side(choice.h.unwrap_or(RESPONSIVE_SIZE.1)),
        dpr: 0.0,
        ua: String::new(),
        radius: 0.0,
        safe: Insets::default(),
        cutout: None,
        bezel: 0.0,
        platform: "web".into(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Box2 {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    /// On-screen px per device CSS px.
    pub scale: f64,
    /// Big enough to show; smaller, a device would be a smudge.
    pub fits: bool,
    /// The page's viewport, in device CSS px: the screen less its bars.
    pub viewport: (f64, f64),
    /// In the area's px.
    pub frame: Box2,
    pub screen: Box2,
    pub page: Box2,
    pub radius: f64,
    /// The screen's rounded corners the page reaches: the page's view rounds those itself.
    pub corners: crate::Corners,
    /// The status bar (top, or left when turned) and the home indicator, in the area's px.
    pub bars: (f64, f64, f64),
    /// In the area's px, from the screen's top left.
    pub cutout: Option<(Box2, f64)>,
}

impl Fitted {
    /// The screen in the frame's own px, as the frame's child is placed: both boxes are kept in
    /// the area's, and the screen drawn at its area position landed a frame's offset away.
    pub fn screen_in_frame(&self) -> Box2 {
        Box2 {
            x: self.screen.x - self.frame.x,
            y: self.screen.y - self.frame.y,
            ..self.screen
        }
    }
}

/// Below this a device is too small to read, and the tab says so instead.
const SMALLEST: f64 = 0.15;

pub fn fit(device: &Device, rotated: bool, area: (f64, f64)) -> Fitted {
    let (w, h, cutout, safe) = if rotated {
        // Turned a quarter to the left, as a phone held in landscape: its top goes left.
        let s = device.safe;
        (
            device.h,
            device.w,
            device.cutout.map(|c| Cutout {
                x: c.y,
                y: device.w - c.x - c.w,
                w: c.h,
                h: c.w,
                ..c
            }),
            Insets {
                top: s.right,
                right: s.bottom,
                bottom: s.left,
                left: s.top,
            },
        )
    } else {
        (device.w, device.h, device.cutout, device.safe)
    };
    let b = device.bezel;
    let scale = (area.0 / (w + 2.0 * b))
        .min(area.1 / (h + 2.0 * b))
        .clamp(0.0, 1.0);
    let fw = (w + 2.0 * b) * scale;
    let fh = (h + 2.0 * b) * scale;
    let frame = Box2 {
        x: (area.0 - fw) / 2.0,
        y: (area.1 - fh) / 2.0,
        w: fw,
        h: fh,
    };
    let screen = Box2 {
        x: frame.x + b * scale,
        y: frame.y + b * scale,
        w: w * scale,
        h: h * scale,
    };
    // Upright, the status bar and the home indicator; turned, the camera's side.
    let (top, left, bottom) = if rotated {
        (0.0, safe.left, 0.0)
    } else {
        (safe.top, 0.0, safe.bottom)
    };
    let viewport = (w - left, h - top - bottom);
    Fitted {
        scale,
        fits: scale >= SMALLEST,
        viewport,
        frame,
        screen,
        page: Box2 {
            x: screen.x + left * scale,
            y: screen.y + top * scale,
            w: viewport.0 * scale,
            h: viewport.1 * scale,
        },
        radius: device.radius * scale,
        corners: crate::Corners {
            top_left: top == 0.0 && left == 0.0,
            top_right: top == 0.0,
            bottom_right: bottom == 0.0,
            bottom_left: bottom == 0.0 && left == 0.0,
        },
        bars: (top * scale, left * scale, bottom * scale),
        cutout: cutout.map(|c| {
            (
                Box2 {
                    x: c.x * scale,
                    y: c.y * scale,
                    w: c.w * scale,
                    h: c.h * scale,
                },
                c.r * scale,
            )
        }),
    }
}

/// Page zoom's steps, as Safari's.
pub const ZOOMS: [f64; 11] = [0.5, 0.75, 0.85, 1.0, 1.15, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

/// The step past `zoom` the way `by` says (one in, one out), or 100% for 0; at the ends it stays.
pub fn step_zoom(zoom: f64, by: i32) -> f64 {
    match by.signum() {
        0 => 1.0,
        1 => ZOOMS
            .iter()
            .copied()
            .find(|z| *z > zoom + 1e-9)
            .unwrap_or(zoom),
        _ => ZOOMS
            .iter()
            .rev()
            .copied()
            .find(|z| *z < zoom - 1e-9)
            .unwrap_or(zoom),
    }
}

pub fn zoom_label(zoom: f64) -> String {
    format!("{}%", (zoom * 100.0).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_device_reads_and_responsive_is_a_bare_screen() {
        assert!(DEVICES.len() >= 10);
        assert!(device_of(&DeviceChoice::default_device()).is_some());
        let r = device_of(&DeviceChoice {
            name: RESPONSIVE.into(),
            w: Some(50.0),
            h: Some(9999.0),
            ..Default::default()
        })
        .unwrap();
        assert_eq!((r.w, r.h), (RESPONSIVE_MIN, RESPONSIVE_MAX));
        assert!(!r.touch());
        assert!(device_of(&DeviceChoice {
            name: "Nokia 3310".into(),
            ..Default::default()
        })
        .is_none());
    }

    #[test]
    fn every_screen_lies_inside_its_frame_upright_and_turned() {
        for device in DEVICES.iter() {
            for rotated in [false, true] {
                let f = fit(device, rotated, (900.0, 500.0));
                let s = f.screen_in_frame();
                let b = device.bezel * f.scale;
                assert!(
                    (s.x - b).abs() < 1e-9 && (s.y - b).abs() < 1e-9,
                    "{}",
                    device.name
                );
                assert!(s.x + s.w <= f.frame.w + 1e-9 && s.y + s.h <= f.frame.h + 1e-9);
            }
        }
    }

    #[test]
    fn a_device_fits_centered_never_enlarged_and_its_page_is_the_screen_less_its_bars() {
        let phone = device_of(&DeviceChoice::default_device()).unwrap();
        let f = fit(&phone, false, (2000.0, 2000.0));
        assert_eq!(f.scale, 1.0);
        assert_eq!(
            f.viewport,
            (phone.w, phone.h - phone.safe.top - phone.safe.bottom)
        );
        assert!((f.frame.x * 2.0 + f.frame.w - 2000.0).abs() < 1e-9);
        let small = fit(&phone, false, (300.0, 300.0));
        assert!(small.scale < 1.0 && small.frame.h <= 300.0 + 1e-9);
        let turned = fit(&phone, true, (2000.0, 2000.0));
        assert_eq!(turned.viewport, (phone.h - phone.safe.top, phone.w));
        assert!(!fit(&phone, false, (20.0, 20.0)).fits);
    }

    #[test]
    fn zoom_steps_as_safaris_and_stays_at_the_ends() {
        assert_eq!(step_zoom(1.0, 1), 1.15);
        assert_eq!(step_zoom(1.0, -1), 0.85);
        assert_eq!(step_zoom(3.0, 1), 3.0);
        assert_eq!(step_zoom(0.5, -1), 0.5);
        assert_eq!(step_zoom(2.0, 0), 1.0);
        assert_eq!(zoom_label(1.15), "115%");
        assert_eq!(zoom_label(0.85), "85%");
        // One between stops (an older build's) goes to the next stop that way.
        assert_eq!(step_zoom(1.1, 1), 1.15);
        assert_eq!(step_zoom(1.1, -1), 1.0);
        let mut z = 0.5;
        let mut seen = vec![z];
        while step_zoom(z, 1) != z {
            z = step_zoom(z, 1);
            seen.push(z);
        }
        assert_eq!(seen, ZOOMS);
    }

    #[test]
    fn every_device_is_a_touch_screen_a_page_can_lay_out_on() {
        assert!(DEVICES.len() >= 11);
        let mut names = std::collections::HashSet::new();
        for d in DEVICES.iter() {
            let n = &d.name;
            assert!(names.insert(n), "{n}: names are unique");
            assert!(d.w > 0.0 && d.h >= d.w, "{n}: portrait, positive");
            assert!(
                d.w.fract() == 0.0 && d.h.fract() == 0.0,
                "{n}: whole CSS px"
            );
            // The phones and tablets ship at these ratios; a typo (25 for 2.5) shows here.
            assert!([2.0, 2.625, 3.0, 3.5].contains(&d.dpr), "{n}: {}", d.dpr);
            assert!(d.radius >= 0.0 && d.radius < d.w / 2.0, "{n}: radius");
            assert!(d.bezel > 0.0, "{n}: bezel");
            let s = d.safe;
            for inset in [s.top, s.right, s.bottom, s.left] {
                assert!((0.0..d.h / 4.0).contains(&inset), "{n}: safe area");
            }
            if let Some(c) = d.cutout {
                assert!(
                    c.w > 0.0 && c.h > 0.0 && c.x >= 0.0 && c.y >= 0.0,
                    "{n}: cutout"
                );
                assert!(c.x + c.w <= d.w, "{n}: cutout inside the screen");
                assert!(c.y + c.h <= s.top, "{n}: cutout inside the top safe area");
                // DevTools rounds a pill 37 high to 19.
                assert!(c.r <= (c.w.min(c.h) / 2.0).ceil(), "{n}: cutout radius");
            }
            assert!(d.touch(), "{n}");
        }
    }

    #[test]
    fn each_devices_user_agent_names_its_platform() {
        for d in DEVICES.iter() {
            let (n, ua) = (&d.name, d.ua.as_str());
            assert!(ua.starts_with("Mozilla/5.0 ("), "{n}");
            assert!(
                !ua.contains("%s"),
                "{n}: DevTools' version placeholder filled in"
            );
            let says = match d.platform.as_str() {
                "ios" => {
                    ua.contains("(iPhone; CPU iPhone OS ")
                        && ua.contains(" like Mac OS X)")
                        && ua.contains(" Mobile/")
                        && ua.contains(" Safari/")
                }
                // iPadOS asks for desktop sites on the larger iPads, as DevTools' iPad Pro does.
                "ipados" => {
                    (ua.contains("(iPad; CPU OS ") && ua.contains(" like Mac OS X)"))
                        || ua.contains("(Macintosh; Intel Mac OS X 10_15_7)")
                }
                "android" => {
                    ua.starts_with("Mozilla/5.0 (Linux; Android ")
                        && ua.contains(") AppleWebKit/537.36 ")
                        && ua.contains(" Chrome/")
                        && ua.ends_with(".0.0.0 Mobile Safari/537.36")
                }
                other => panic!("{n}: platform {other}"),
            };
            assert!(says, "{n}: {ua}");
        }
    }

    #[test]
    fn responsive_is_a_bare_screen_its_sides_whole_and_in_bounds() {
        let side = |w: Option<f64>, h: Option<f64>| {
            let r = device_of(&DeviceChoice {
                name: RESPONSIVE.into(),
                w,
                h,
                ..Default::default()
            })
            .unwrap();
            (r.w, r.h)
        };
        assert_eq!(side(None, None), (400.0, 800.0));
        assert_eq!(side(Some(500.4), Some(50.0)), (500.0, RESPONSIVE_MIN));
        assert_eq!(side(Some(0.4), Some(199.6)), (RESPONSIVE_MIN, 200.0));
        assert_eq!(
            side(Some(-5.0), Some(1e9)),
            (RESPONSIVE_MIN, RESPONSIVE_MAX)
        );
        assert_eq!(
            side(Some(2999.5), Some(3000.4)),
            (RESPONSIVE_MAX, RESPONSIVE_MAX)
        );
        let r = device_of(&DeviceChoice {
            name: RESPONSIVE.into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            (r.bezel, r.radius, r.dpr, r.ua.as_str()),
            (0.0, 0.0, 0.0, "")
        );
        assert_eq!((r.safe, r.cutout), (Insets::default(), None));
    }

    fn phone() -> Device {
        Device {
            name: "Test Phone".into(),
            w: 400.0,
            h: 800.0,
            dpr: 3.0,
            ua: "Mozilla/5.0 (Test)".into(),
            radius: 40.0,
            safe: Insets {
                top: 50.0,
                right: 0.0,
                bottom: 30.0,
                left: 0.0,
            },
            cutout: Some(Cutout {
                kind: CutoutKind::Island,
                x: 150.0,
                y: 10.0,
                w: 100.0,
                h: 30.0,
                r: 15.0,
            }),
            bezel: 10.0,
            platform: "ios".into(),
        }
    }

    fn b(x: f64, y: f64, w: f64, h: f64) -> Box2 {
        Box2 { x, y, w, h }
    }

    fn corners(
        top_left: bool,
        top_right: bool,
        bottom_right: bool,
        bottom_left: bool,
    ) -> crate::Corners {
        crate::Corners {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    #[test]
    fn a_device_that_fits_shows_at_its_own_size_its_page_the_screen_less_its_bars() {
        let f = fit(&phone(), false, (1000.0, 1000.0));
        assert_eq!(f.scale, 1.0);
        assert_eq!(f.frame, b(290.0, 90.0, 420.0, 820.0));
        assert_eq!(f.screen, b(300.0, 100.0, 400.0, 800.0));
        assert_eq!(f.radius, 40.0);
        assert_eq!(f.cutout, Some((b(150.0, 10.0, 100.0, 30.0), 15.0)));
        assert_eq!(f.bars, (50.0, 0.0, 30.0));
        assert_eq!(f.viewport, (400.0, 720.0));
        assert_eq!(f.page, b(300.0, 150.0, 400.0, 720.0));
        assert_eq!(f.corners, corners(false, false, false, false));
        // Without a home indicator the page reaches the bottom corners.
        let mut pixel = phone();
        pixel.safe.bottom = 0.0;
        let f = fit(&pixel, false, (1000.0, 1000.0));
        assert_eq!(f.corners, corners(false, false, true, true));
        // Turned: only the camera's side has a bar, and the page has the right corners.
        let side = fit(&phone(), true, (2000.0, 2000.0));
        assert_eq!(side.bars, (0.0, 50.0, 0.0));
        assert_eq!(side.viewport, (750.0, 400.0));
        assert_eq!(side.corners, corners(false, true, true, false));
    }

    #[test]
    fn one_that_doesnt_fit_shrinks_by_its_tighter_side() {
        let f = fit(&phone(), false, (1000.0, 410.0));
        assert_eq!(f.scale, 0.5);
        assert_eq!(f.screen, b(400.0, 5.0, 200.0, 400.0));
        assert_eq!(f.viewport, (400.0, 720.0));
        assert_eq!(f.radius, 20.0);
        assert_eq!(f.cutout, Some((b(75.0, 5.0, 50.0, 15.0), 7.5)));
    }

    #[test]
    fn rotated_the_sides_swap_and_the_cutout_turns_to_the_left() {
        let f = fit(&phone(), true, (2000.0, 2000.0));
        assert_eq!((f.screen.w, f.screen.h), (800.0, 400.0));
        let (c, r) = f.cutout.unwrap();
        assert_eq!((c, r), (b(10.0, 150.0, 30.0, 100.0), 15.0));
        assert!(c.x + c.w <= f.screen.w && c.y + c.h <= f.screen.h);
    }

    #[test]
    fn turned_a_devices_bar_and_cutout_go_where_its_top_went() {
        for d in DEVICES.iter() {
            let n = &d.name;
            let side = fit(d, true, (5000.0, 5000.0));
            assert_eq!((side.screen.w, side.screen.h), (d.h, d.w), "{n}");
            assert_eq!(side.bars, (0.0, d.safe.top, 0.0), "{n}");
            assert_eq!(side.viewport, (d.h - d.safe.top, d.w), "{n}");
            let Some(up) = d.cutout else {
                assert_eq!(side.cutout, None, "{n}");
                continue;
            };
            let (c, r) = side.cutout.unwrap();
            // The camera sits in the left bar now, the same size turned, the same distance in.
            assert_eq!((c.w, c.h, r), (up.h, up.w, up.r), "{n}");
            assert_eq!(c.x, up.y, "{n}");
            assert!(c.x + c.w <= side.bars.1, "{n}: inside the left bar");
            assert!(
                (c.y + c.h / 2.0 - d.w / 2.0).abs() <= 1.0,
                "{n}: a centered camera stays centered"
            );
        }
    }

    #[test]
    fn every_device_fits_whole_centered_and_never_enlarged_at_any_size() {
        let mut all = DEVICES.to_vec();
        for (w, h) in [(320.0, 2000.0), (3000.0, 200.0), (200.0, 3000.0)] {
            all.extend(device_of(&DeviceChoice {
                name: RESPONSIVE.into(),
                w: Some(w),
                h: Some(h),
                ..Default::default()
            }));
        }
        for d in &all {
            let n = &d.name;
            for rotated in [false, true] {
                assert_eq!(fit(d, rotated, (100_000.0, 100_000.0)).scale, 1.0, "{n}");
                let (sw, sh) = if rotated { (d.h, d.w) } else { (d.w, d.h) };
                let areas = (1..=60).map(|i| (37.0 * i as f64, 23.0 * i as f64 + 11.0));
                for area in areas.chain([(300.0, 300.0), (1600.0, 900.0), (1.0, 1.0)]) {
                    let f = fit(d, rotated, area);
                    let at = format!("{n} {rotated} {area:?}");
                    assert!(f.scale > 0.0 && f.scale <= 1.0, "{at}");
                    assert!(
                        (f.screen.w / f.screen.h - sw / sh).abs() < 1e-9,
                        "{at}: its shape"
                    );
                    assert!((f.screen.w - sw * f.scale).abs() < 1e-9, "{at}");
                    assert!((f.page.w - f.viewport.0 * f.scale).abs() < 1e-9, "{at}");
                    assert!(f.page.w <= f.screen.w + 1e-9, "{at}");
                    assert!(
                        f.frame.x >= -1e-9 && f.frame.y >= -1e-9,
                        "{at}: inside the area"
                    );
                    assert!(f.frame.x + f.frame.w <= area.0 + 1e-9, "{at}");
                    assert!(f.frame.y + f.frame.h <= area.1 + 1e-9, "{at}");
                    assert!(f.screen.x >= f.frame.x, "{at}");
                    assert!(
                        f.screen.x + f.screen.w <= f.frame.x + f.frame.w + 1e-9,
                        "{at}"
                    );
                    if let Some((c, _)) = f.cutout {
                        assert!(
                            c.x + c.w <= f.screen.w + 1e-9,
                            "{at}: cutout inside the screen"
                        );
                        assert!(c.y + c.h <= f.screen.h + 1e-9, "{at}");
                    }
                }
            }
        }
        // A screen far wider than the tab still stays in it, too small to read.
        let wide = device_of(&DeviceChoice {
            name: RESPONSIVE.into(),
            w: Some(3000.0),
            h: Some(800.0),
            ..Default::default()
        })
        .unwrap();
        let f = fit(&wide, false, (120.0, 400.0));
        assert!(f.screen.x >= 0.0 && f.screen.x + f.screen.w <= 120.0);
        assert!(!f.fits);
    }

    #[test]
    fn no_area_a_negative_one_or_a_sliver_gives_a_finite_centered_fit_too_small_to_show() {
        for area in [(0.0, 0.0), (-50.0, 300.0), (300.0, -1.0), (2.0, 2.0)] {
            let f = fit(&phone(), false, area);
            assert!(!f.fits, "{area:?}");
            assert!(f.scale >= 0.0 && f.scale < SMALLEST, "{area:?}");
            let r = f.frame;
            for v in [r.x, r.y, r.w, r.h, f.screen.x, f.screen.y, f.radius] {
                assert!(v.is_finite(), "{area:?}");
            }
            assert!((r.x - (area.0 - r.w) / 2.0).abs() < 1e-9, "{area:?}");
            assert_eq!(
                f.viewport,
                (400.0, 720.0),
                "the page keeps the device's size"
            );
        }
    }
}
