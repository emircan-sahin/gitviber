//! The device last shown in device mode, for its next toggle in any tab: kept in the config
//! folder's browser-device.json, and on the first launch taken from the old app's
//! `gitviber.browserDevice`.

use super::devices::{self, DeviceChoice};
use gitviber_ui::store::{config_dir, write_in_background};
use gpui::{App, Global};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Default)]
struct LastDevice(Option<DeviceChoice>);

impl Global for LastDevice {}

fn path() -> PathBuf {
    config_dir().join("browser-device.json")
}

/// A device the old app kept, while devices.json still knows it.
fn from_legacy(value: impl Fn(&str) -> Option<Value>) -> Option<DeviceChoice> {
    let choice: DeviceChoice = serde_json::from_value(value("gitviber.browserDevice")?).ok()?;
    devices::device_of(&choice).is_some().then_some(choice)
}

fn read() -> Option<DeviceChoice> {
    let text = std::fs::read_to_string(path()).ok()?;
    serde_json::from_str::<DeviceChoice>(&text)
        .ok()
        .filter(|c| devices::device_of(c).is_some())
}

/// Once, in init: this build's file, else on the first launch the old app's.
pub(super) fn load(cx: &mut App) {
    let last = if path().exists() {
        read()
    } else {
        let old = from_legacy(gitviber_ui::legacy::value);
        if let Some(choice) = &old {
            write_in_background(
                cx,
                path(),
                serde_json::to_string(choice).unwrap_or_default(),
            );
        }
        old
    };
    cx.set_global(LastDevice(last));
}

pub fn last_device(cx: &App) -> Option<DeviceChoice> {
    cx.try_global::<LastDevice>().and_then(|l| l.0.clone())
}

pub fn set_last_device(choice: &DeviceChoice, cx: &mut App) {
    if last_device(cx).as_ref() == Some(choice) {
        return;
    }
    cx.set_global(LastDevice(Some(choice.clone())));
    write_in_background(
        cx,
        path(),
        serde_json::to_string(choice).unwrap_or_default(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_apps_device_comes_over_while_its_name_is_known() {
        let old = serde_json::json!({ "name": "iPhone 16 Pro", "rotated": true });
        let got = from_legacy(|k| (k == "gitviber.browserDevice").then(|| old.clone())).unwrap();
        assert_eq!(got.name, "iPhone 16 Pro");
        assert!(got.rotated);
        let responsive = serde_json::json!({ "name": "Responsive", "w": 500, "h": 900 });
        let got = from_legacy(|_| Some(responsive.clone())).unwrap();
        assert_eq!((got.w, got.h), (Some(500.0), Some(900.0)));
        assert_eq!(
            from_legacy(|_| Some(serde_json::json!({ "name": "Nokia" }))),
            None
        );
        assert_eq!(from_legacy(|_| Some(serde_json::json!(42))), None);
        assert_eq!(from_legacy(|_| None), None);
    }
}
