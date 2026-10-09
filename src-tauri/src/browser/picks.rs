//! An element picked on a page (scripts/picker.js), for the note that goes to an agent: what
//! the picker reads, checked and bounded, and where its picture is kept a day.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How long a pick's picture is kept: an agent reads it within the session it was sent in.
const KEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// Room around the element in its picture, in the page's CSS px.
const MARGIN: f64 = 8.0;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Pick {
    pub selector: String,
    pub tag: String,
    pub html: String,
    pub text: String,
    /// In the page's CSS px, from its viewport's top left.
    #[serde(rename = "box")]
    pub bounds: Bounds,
    pub styles: BTreeMap<String, String>,
    /// Tags the element for the page's own world, which reads its React components.
    #[serde(skip_serializing)]
    pub nonce: String,
    /// The React components it's in, nearest first; none outside React.
    #[serde(default)]
    pub components: Vec<String>,
    /// Its picture, a PNG kept a day.
    #[serde(default)]
    pub screenshot: Option<String>,
    #[serde(default)]
    pub url: String,
}

#[derive(Deserialize)]
struct Message {
    pick: Option<Pick>,
    #[serde(default)]
    cancelled: bool,
}

fn cut(s: &mut String, max: usize) {
    if s.len() > max {
        let mut at = max;
        while !s.is_char_boundary(at) {
            at -= 1;
        }
        s.truncate(at);
    }
}

/// A picker message: Some(pick), or None when the user cancelled. Err for anything else, which
/// only GitViber's own world could have sent and shouldn't.
pub fn read(json: &str) -> Result<Option<Pick>, String> {
    let message: Message = serde_json::from_str(json).map_err(|e| e.to_string())?;
    match (message.pick, message.cancelled) {
        (_, true) => Ok(None),
        (Some(mut pick), false) => {
            // Bounded here too: the agent gets this as typed text.
            cut(&mut pick.selector, 1000);
            cut(&mut pick.tag, 64);
            cut(&mut pick.html, 610);
            cut(&mut pick.text, 210);
            pick.styles.retain(|k, _| k.len() <= 40);
            pick.styles.values_mut().for_each(|v| cut(v, 200));
            let b = pick.bounds;
            if ![b.x, b.y, b.w, b.h].iter().all(|v| v.is_finite()) {
                return Err("A pick without a box".into());
            }
            Ok(Some(pick))
        }
        (None, false) => Err("Neither a pick nor a cancel".into()),
    }
}

/// The part of a view `size` points big to take a picture of: the element at the page's `zoom`,
/// with a margin, kept inside the view. None when none of it shows.
pub fn picture_rect(b: Bounds, zoom: f64, size: (f64, f64)) -> Option<Bounds> {
    let x0 = ((b.x - MARGIN) * zoom).max(0.0);
    let y0 = ((b.y - MARGIN) * zoom).max(0.0);
    let x1 = ((b.x + b.w + MARGIN) * zoom).min(size.0);
    let y1 = ((b.y + b.h + MARGIN) * zoom).min(size.1);
    (x1 > x0 && y1 > y0).then_some(Bounds {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Where picks' pictures go, in the app's cache folder.
pub fn folder(cache: &Path) -> PathBuf {
    cache.join("browser-picks")
}

/// A pick's picture, named for when it was taken.
pub fn picture_path(folder: &Path, at: SystemTime) -> PathBuf {
    let ms = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    folder.join(format!("pick-{ms}.png"))
}

/// Pictures more than a day old, gone; run as the app starts.
pub fn prune(folder: &Path, now: SystemTime) {
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let ours = name
            .to_str()
            .is_some_and(|n| n.starts_with("pick-") && n.ends_with(".png"));
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age > KEEP));
        if ours && old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PICK: &str = r##"{"pick":{"selector":"#save","tag":"button","html":"<button id=\"save\">Save</button>","text":"Save","box":{"x":10,"y":20,"w":80,"h":30},"styles":{"color":"rgb(0, 0, 0)"},"nonce":"n1"}}"##;

    #[test]
    fn a_pick_or_a_cancel_and_nothing_else() {
        let pick = read(PICK).unwrap().unwrap();
        assert_eq!(
            (pick.selector.as_str(), pick.nonce.as_str()),
            ("#save", "n1")
        );
        assert!(pick.components.is_empty() && pick.screenshot.is_none());
        assert_eq!(read(r#"{"cancelled":true}"#), Ok(None));
        for no in ["", "{}", r#"{"pick":null}"#, r#"{"pick":{"selector":"a"}}"#] {
            assert!(read(no).is_err(), "{no}");
        }
        // What the agent reads leaves out the nonce.
        assert!(!serde_json::to_string(&pick).unwrap().contains("n1"));
    }

    #[test]
    fn long_fields_are_bounded() {
        let long = PICK.replace(
            r#""text":"Save""#,
            &format!(r#""text":"{}""#, "ü".repeat(500)),
        );
        let pick = read(&long).unwrap().unwrap();
        assert!(pick.text.len() <= 210 && pick.text.chars().all(|c| c == 'ü'));
    }

    #[test]
    fn the_picture_is_the_element_with_a_margin_at_the_pages_zoom_inside_the_view() {
        let b = Bounds {
            x: 10.0,
            y: 20.0,
            w: 80.0,
            h: 30.0,
        };
        assert_eq!(
            picture_rect(b, 1.0, (1000.0, 1000.0)),
            Some(Bounds {
                x: 2.0,
                y: 12.0,
                w: 96.0,
                h: 46.0
            })
        );
        // Device mode at half size: in the view's points.
        assert_eq!(
            picture_rect(b, 0.5, (1000.0, 1000.0)),
            Some(Bounds {
                x: 1.0,
                y: 6.0,
                w: 48.0,
                h: 23.0
            })
        );
        // At the view's edge, cut to it; scrolled out of sight, none.
        let corner = Bounds {
            x: -5.0,
            y: -5.0,
            w: 10.0,
            h: 10.0,
        };
        assert_eq!(
            picture_rect(corner, 1.0, (100.0, 100.0)),
            Some(Bounds {
                x: 0.0,
                y: 0.0,
                w: 13.0,
                h: 13.0
            })
        );
        assert_eq!(
            picture_rect(
                Bounds {
                    x: 0.0,
                    y: 500.0,
                    w: 10.0,
                    h: 10.0
                },
                1.0,
                (100.0, 100.0)
            ),
            None
        );
    }

    #[test]
    fn pictures_older_than_a_day_go_and_others_stay() {
        let dir = std::env::temp_dir().join(format!("gitviber-picks-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        let fresh = picture_path(&dir, now);
        let other = dir.join("notes.txt");
        std::fs::write(&fresh, b"png").unwrap();
        std::fs::write(&other, b"x").unwrap();
        prune(&dir, now + Duration::from_secs(60));
        assert!(fresh.exists() && other.exists());
        prune(&dir, now + KEEP + Duration::from_secs(60));
        assert!(
            !fresh.exists() && other.exists(),
            "only our own pictures go"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
