//! What the user's own tabs download (macos/user_downloads.rs): saved in the Downloads folder
//! under a name that was free when the download began, never over a file, each told to the app
//! page as it goes (`browser-download`). Apart from AppKit so it can be tested.

use super::downloads::free_path;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// A page starts this many downloads in a burst; the next ones are refused until the window
/// passes, as are any while `MOST_GOING` are running in the whole app.
const BURST: usize = 3;
const BURST_WINDOW: Duration = Duration::from_secs(10);
pub const MOST_GOING: usize = 4;

/// Whether a tab may start one more download at `now`, given those it started lately and how
/// many are `going` in all; counts it.
pub fn allow(recent: &mut Vec<Instant>, going: usize, now: Instant) -> bool {
    recent.retain(|&at| now.duration_since(at) < BURST_WINDOW);
    let ok = going < MOST_GOING && recent.len() < BURST;
    if ok {
        recent.push(now);
    }
    ok
}

/// Whether `path` is a plain file made at or after `since`: one a download made, not one that
/// was there, or that someone put there (a link, say) while it was reserved.
pub fn made_since(path: &Path, since: SystemTime) -> bool {
    path.symlink_metadata()
        .ok()
        .filter(|m| m.is_file())
        .and_then(|m| m.created().ok())
        .is_some_and(|made| made >= since)
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Going,
    Done,
    Failed,
    Cancelled,
}

/// One download as the app page shows it: `total` is -1 while the server hasn't said.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Progress<'a> {
    pub tab: &'a str,
    pub key: u64,
    pub name: &'a str,
    pub state: State,
    pub received: i64,
    pub total: i64,
    /// Where a finished one is.
    pub path: Option<&'a str>,
    /// Why one failed.
    pub why: Option<&'a str>,
}

/// The paths the downloads going hold: WebKit makes a file only once data comes, so two
/// downloads of one name would otherwise pick the same one.
#[derive(Default)]
pub struct Claims(HashSet<PathBuf>);

impl Claims {
    /// A path for `name` in `folder` nothing is at (a dangling link counts) and no download going
    /// holds; held until `release`.
    pub fn claim(&mut self, folder: &Path, name: &str) -> PathBuf {
        let path = free_path(folder, name, |p| {
            self.0.contains(p) || p.symlink_metadata().is_ok()
        });
        self.0.insert(path.clone());
        path
    }

    pub fn release(&mut self, path: &Path) {
        self.0.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_of_one_name_never_share_a_path_or_replace_a_file() {
        let dir = std::env::temp_dir().join(format!("gitviber-saves-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("report.pdf"), b"mine").unwrap();
        let mut claims = Claims::default();
        let first = claims.claim(&dir, "report.pdf");
        let second = claims.claim(&dir, "report.pdf");
        assert_eq!(first, dir.join("report (2).pdf"));
        assert_eq!(second, dir.join("report (3).pdf"));
        // One that ended frees its path for the next, if nothing was written there.
        claims.release(&first);
        assert_eq!(claims.claim(&dir, "report.pdf"), first);
        assert_eq!(std::fs::read(dir.join("report.pdf")).unwrap(), b"mine");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_tab_starts_a_few_downloads_at_a_time_and_the_app_a_few_in_all() {
        let start = Instant::now();
        let mut recent = Vec::new();
        for n in 0..BURST as u64 {
            assert!(allow(&mut recent, 0, start + Duration::from_millis(n)));
        }
        assert!(!allow(&mut recent, 0, start + Duration::from_secs(5)));
        assert!(allow(&mut recent, 0, start + BURST_WINDOW));
        // Too many running refuses a tab with none lately, and isn't counted against it.
        let mut quiet = Vec::new();
        assert!(!allow(&mut quiet, MOST_GOING, start));
        assert!(quiet.is_empty());
    }

    #[test]
    fn only_a_file_made_since_the_reservation_is_ours_to_remove() {
        let dir = std::env::temp_dir().join(format!("gitviber-made-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old");
        std::fs::write(&old, b"x").unwrap();
        let since = SystemTime::now() + Duration::from_millis(50);
        std::thread::sleep(Duration::from_millis(120));
        let new = dir.join("new");
        std::fs::write(&new, b"x").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&new, &link).unwrap();
        assert!(!made_since(&old, since), "there before");
        assert!(made_since(&new, since));
        assert!(!made_since(&link, since), "a link isn't a file it made");
        assert!(!made_since(&dir.join("none"), since));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_download_is_told_in_camel_case_with_its_state_in_words() {
        let said = serde_json::to_value(Progress {
            tab: "t1",
            key: 3,
            name: "a.zip",
            state: State::Cancelled,
            received: 10,
            total: -1,
            path: None,
            why: None,
        })
        .unwrap();
        assert_eq!(said["state"], "cancelled");
        assert_eq!(said["total"], -1);
        assert_eq!(said["key"], 3);
    }
}
