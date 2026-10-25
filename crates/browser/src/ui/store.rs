//! What the browser remembers across its tabs, in the config folder's browser.json: the device
//! last shown in device mode (its next toggle in any tab starts there) and the model last picked
//! to ask about a page with. On the first launch it's taken from the old app's
//! `gitviber.browserDevice` and `gitviber.askModel.browser`.

use super::devices::{self, DeviceChoice};
use gitviber_ui::store::{config_dir, write_in_background};
use gpui::{App, Global};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    #[serde(skip_serializing_if = "Option::is_none")]
    device: Option<DeviceChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ask_model: Option<String>,
}

impl Global for Saved {}

fn path() -> PathBuf {
    config_dir().join("browser.json")
}

/// A device devices.json still knows.
fn known(choice: DeviceChoice) -> Option<DeviceChoice> {
    devices::device_of(&choice).is_some().then_some(choice)
}

/// What the old app kept.
fn from_legacy(value: impl Fn(&str) -> Option<Value>) -> Saved {
    Saved {
        device: value("gitviber.browserDevice")
            .and_then(|v| serde_json::from_value(v).ok())
            .and_then(known),
        ask_model: value("gitviber.askModel.browser").and_then(|v| v.as_str().map(String::from)),
    }
}

fn read() -> Saved {
    let saved: Saved = std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    Saved {
        device: saved.device.and_then(known),
        ..saved
    }
}

fn write(saved: &Saved, cx: &mut App) {
    let text = serde_json::to_string_pretty(saved).unwrap_or_default();
    write_in_background(cx, path(), text);
}

/// Once, in init: this build's file, else on the first launch the old app's.
pub(super) fn load(cx: &mut App) {
    let saved = if path().exists() {
        read()
    } else {
        let old = from_legacy(gitviber_ui::legacy::value);
        if old != Saved::default() {
            write(&old, cx);
        }
        old
    };
    cx.set_global(saved);
}

fn update(cx: &mut App, f: impl FnOnce(&mut Saved)) {
    let mut next = cx.try_global::<Saved>().cloned().unwrap_or_default();
    f(&mut next);
    if cx.try_global::<Saved>() == Some(&next) {
        return;
    }
    write(&next, cx);
    cx.set_global(next);
}

pub fn last_device(cx: &App) -> Option<DeviceChoice> {
    cx.try_global::<Saved>().and_then(|s| s.device.clone())
}

pub fn set_last_device(choice: &DeviceChoice, cx: &mut App) {
    update(cx, |s| s.device = Some(choice.clone()));
}

pub fn ask_model(cx: &App) -> Option<String> {
    cx.try_global::<Saved>().and_then(|s| s.ask_model.clone())
}

pub fn set_ask_model(model: &str, cx: &mut App) {
    update(cx, |s| s.ask_model = Some(model.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_apps_device_and_model_come_over_while_known() {
        let old = |k: &str| match k {
            "gitviber.browserDevice" => {
                Some(serde_json::json!({ "name": "iPhone 16 Pro", "rotated": true }))
            }
            "gitviber.askModel.browser" => Some(serde_json::json!("sonnet")),
            _ => None,
        };
        let got = from_legacy(old);
        assert_eq!(got.device.as_ref().unwrap().name, "iPhone 16 Pro");
        assert!(got.device.as_ref().unwrap().rotated);
        assert_eq!(got.ask_model.as_deref(), Some("sonnet"));
        let responsive = serde_json::json!({ "name": "Responsive", "w": 500, "h": 900 });
        let got = from_legacy(|_| Some(responsive.clone()));
        let d = got.device.unwrap();
        assert_eq!((d.w, d.h), (Some(500.0), Some(900.0)));
        let gone = from_legacy(|_| Some(serde_json::json!({ "name": "Nokia" })));
        assert_eq!(gone.device, None);
        assert_eq!(from_legacy(|_| None), Saved::default());
    }
}
