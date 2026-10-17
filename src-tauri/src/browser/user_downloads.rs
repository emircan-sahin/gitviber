//! What the user's own tabs download (macos/user_downloads.rs): saved in the Downloads folder
//! under a name that was free when the download began, never over a file, each told to the app
//! page as it goes (`browser-download`). Apart from AppKit so it can be tested.

use super::downloads::free_path;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
