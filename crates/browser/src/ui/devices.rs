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
    }
}
