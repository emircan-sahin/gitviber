//! An element picked on a page (scripts/picker.js), for the note that goes to an agent: what
//! the picker reads, checked and bounded, and where its picture is kept a day.

use super::cut;
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
    #[serde(default)]
    pub screenshot: Option<String>,
    #[serde(default)]
    pub url: String,
    /// Picked with ⇧ held: one of a few, the picker still on for the next.
    #[serde(default)]
    pub more: bool,
    /// Its place among this picker's picks, from 1: they may arrive out of order.
    #[serde(default)]
    pub number: u32,
}

#[derive(Deserialize)]
struct Message {
    pick: Option<Pick>,
    #[serde(default)]
    cancelled: bool,
}

/// A picker message: Some(pick), or None when the user cancelled. Err for anything else, which
/// only GitViber's own world could have sent and shouldn't.
pub fn read(json: &str) -> Result<Option<Pick>, String> {
    let message: Message = serde_json::from_str(json).map_err(|e| e.to_string())?;
    match (message.pick, message.cancelled) {
        (_, true) => Ok(None),
        (Some(mut pick), false) => {
            // Bounded here too, to what picker.js cuts them to: the agent gets this as typed text.
            pick.selector = cut(&pick.selector, 1000);
            pick.tag = cut(&pick.tag, 64);
            pick.html = cut(&pick.html, 600);
            pick.text = cut(&pick.text, 200);
            pick.styles.retain(|k, _| k.len() <= 40);
            pick.styles.values_mut().for_each(|v| *v = cut(v, 200));
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

pub fn folder(cache: &Path) -> PathBuf {
    cache.join("browser-picks")
}

pub fn picture_path(folder: &Path, at: SystemTime) -> PathBuf {
    let ms = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    folder.join(format!("pick-{ms}.png"))
}

/// A picture saved under a name of its own: a second in the same millisecond (two tabs) gets
/// the next free one, never another's file.
pub fn save(folder: &Path, png: &[u8], at: SystemTime) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let first = picture_path(folder, at);
    let stem = first.with_extension("");
    for n in 0..100 {
        let path = if n == 0 {
            first.clone()
        } else {
            PathBuf::from(format!("{}-{n}.png", stem.display()))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => return std::io::Write::write_all(&mut file, png).map(|()| path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::ErrorKind::AlreadyExists.into())
}

/// A PNG in the app's cache folder for pictures, its path; None when it couldn't be written.
pub fn save_png(app: &tauri::AppHandle, png: &[u8]) -> Option<String> {
    use tauri::Manager;
    let path = save(
        &folder(&app.path().app_cache_dir().ok()?),
        png,
        SystemTime::now(),
    )
    .ok()?;
    Some(path.to_string_lossy().into_owned())
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
        assert!(pick.text.chars().count() == 200 && pick.text.chars().all(|c| c == 'ü'));
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

    #[test]
    fn a_message_that_says_both_or_bends_the_shape_is_a_cancel_or_nothing() {
        // Cancelled wins over a pick sent with it.
        let both = PICK.replacen('{', r#"{"cancelled":true,"#, 1);
        assert_eq!(read(&both), Ok(None));
        for no in [
            r#"{"pick":{"selector":"a","tag":"a","html":"","text":"","box":{"x":1e400,"y":0,"w":1,"h":1},"styles":{},"nonce":"n"}}"#,
            r#"{"pick":{"selector":"a","tag":"a","html":"","text":"","box":{"x":0,"y":0,"w":1},"styles":{},"nonce":"n"}}"#,
            r#"{"pick":{"selector":"a","tag":"a","html":"","text":"","box":{"x":0,"y":0,"w":1,"h":1},"styles":{"a":1},"nonce":"n"}}"#,
            r#"{"cancelled":"yes"}"#,
            "[]",
        ] {
            assert!(read(no).is_err(), "{no}");
        }
        // Every field bounded in characters, and odd style names dropped.
        let wide = "界".repeat(2000);
        let message = serde_json::json!({ "pick": {
            "selector": wide, "tag": wide, "html": wide, "text": wide,
            "box": { "x": -1e9, "y": 1e9, "w": 0, "h": -5 },
            "styles": { "color": wide, "x".repeat(41): "dropped" },
            "nonce": "n", "components": ["ignored-from-the-page"], "screenshot": "/etc/passwd"
        }});
        let pick = read(&message.to_string()).unwrap().unwrap();
        for (text, max) in [
            (&pick.selector, 1000),
            (&pick.tag, 64),
            (&pick.html, 600),
            (&pick.text, 200),
        ] {
            assert!(
                text.chars().count() == max && text.chars().all(|c| c == '界'),
                "{max}"
            );
        }
        assert_eq!(pick.styles.len(), 1);
        assert_eq!(pick.styles["color"].chars().count(), 200);
        // An empty or inside-out box shows nowhere.
        assert_eq!(picture_rect(pick.bounds, 1.0, (1000.0, 1000.0)), None);
    }

    #[test]
    fn a_picture_never_leaves_the_view_at_any_zoom() {
        let mut seed: u64 = 17;
        let mut next = |span: f64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 11) as f64 / (1u64 << 53) as f64) * span
        };
        for _ in 0..20_000 {
            let b = Bounds {
                x: next(3000.0) - 1000.0,
                y: next(3000.0) - 1000.0,
                w: next(1500.0),
                h: next(1500.0),
            };
            let zoom = 0.05 + next(3.0);
            let size = (1.0 + next(2000.0), 1.0 + next(2000.0));
            if let Some(r) = picture_rect(b, zoom, size) {
                assert!(r.x >= 0.0 && r.y >= 0.0 && r.w > 0.0 && r.h > 0.0, "{r:?}");
                assert!(
                    r.x + r.w <= size.0 + 1e-9 && r.y + r.h <= size.1 + 1e-9,
                    "{r:?} in {size:?}"
                );
                // It holds the element's part that shows.
                assert!(r.x <= (b.x * zoom).max(0.0) + 1e-9 && r.y <= (b.y * zoom).max(0.0) + 1e-9);
            }
        }
        assert_eq!(
            picture_rect(
                Bounds {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0
                },
                0.0,
                (100.0, 100.0)
            ),
            None
        );
    }

    #[test]
    fn pruning_a_missing_or_odd_folder_leaves_the_rest() {
        let dir = std::env::temp_dir().join(format!("gitviber-picks-odd-{}", std::process::id()));
        prune(&dir, SystemTime::now());
        std::fs::create_dir_all(dir.join("pick-dir.png")).unwrap();
        let later = SystemTime::now() + KEEP * 2;
        prune(&dir, later);
        assert!(
            dir.join("pick-dir.png").is_dir(),
            "a folder named like a picture stays"
        );
        assert_eq!(folder(&dir), dir.join("browser-picks"));
        let at = SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        assert_eq!(picture_path(&dir, at), dir.join("pick-1700000000123.png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_pictures_in_one_millisecond_keep_both() {
        let dir = std::env::temp_dir().join(format!("gitviber-picks-save-{}", std::process::id()));
        let at = SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        let a = save(&dir, b"a", at).unwrap();
        let b = save(&dir, b"b", at).unwrap();
        assert_eq!(a, dir.join("pick-1700000000123.png"));
        assert_eq!(b, dir.join("pick-1700000000123-1.png"));
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
