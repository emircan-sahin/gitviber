//! A folder of the app's own in the system's temp folder, for files a command needs on disk.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Readable only by the user (it can hold the repo's content), under a name no one can guess, and
/// removed with everything in it when dropped, unless kept.
pub(crate) struct ScratchDir(Option<PathBuf>);

impl ScratchDir {
    pub(crate) fn new(purpose: &str) -> Result<Self, String> {
        let name = crate::askpass::random_hex(8).ok_or("could not name a temporary folder")?;
        let dir = std::env::temp_dir().join(format!("gitviber-{purpose}-{name}"));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        // Fails if anything already sits at that path, so nobody can plant one for us.
        builder
            .create(&dir)
            .map_err(|e| format!("could not make a temporary folder: {e}"))?;
        Ok(ScratchDir(Some(dir)))
    }

    /// A new one, with `purpose`'s folders older than `age` swept (on their own thread, as reading
    /// a crowded temp folder takes a while), and its path as an agent's tools resolve it: /var is a
    /// link to /private/var on macOS.
    pub(crate) fn fresh(purpose: &'static str, age: Duration) -> Result<(Self, PathBuf), String> {
        std::thread::spawn(move || sweep(purpose, age));
        let dir = Self::new(purpose)?;
        let path = dir.path().canonicalize().map_err(|e| e.to_string())?;
        Ok((dir, path))
    }

    pub(crate) fn path(&self) -> &Path {
        self.0
            .as_deref()
            .expect("a ScratchDir has its folder until it's kept or dropped")
    }

    /// Leaves the folder in place: for a file a program reads after this returns, which `sweep`
    /// removes once it's older than that program could still want it.
    pub(crate) fn keep(mut self) {
        self.0 = None;
    }
}

/// `text` in a new file at `path` only the user can read.
pub(crate) fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(text.as_bytes())
}

/// Removes `purpose`'s folders older than `age`: left by a run the app quit during, which never
/// got to drop them.
pub(crate) fn sweep(purpose: &str, age: Duration) {
    let prefix = format!("gitviber-{purpose}-");
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    // The name first: a temp folder can hold a hundred thousand entries.
    for entry in entries.flatten() {
        let ours = entry.file_name().to_string_lossy().starts_with(&prefix);
        let old = || {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t.elapsed().is_ok_and(|e| e > age))
        };
        if ours && old() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if let Some(dir) = &self.0 {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[cfg(unix)]
    #[test]
    fn sweep_removes_only_its_own_old_folders() {
        let old = ScratchDir::new("sweep-test").unwrap();
        let new = ScratchDir::new("sweep-test").unwrap();
        let other = ScratchDir::new("sweep-other").unwrap();
        let hours_ago = SystemTime::now() - Duration::from_secs(2 * 3600);
        for dir in [&old, &other] {
            let f = std::fs::File::open(dir.path()).unwrap();
            f.set_modified(hours_ago).unwrap();
        }
        sweep("sweep-test", Duration::from_secs(3600));
        assert!(!old.path().exists());
        assert!(new.path().exists() && other.path().exists());
    }

    #[test]
    fn a_kept_folder_outlives_its_scratch_dir() {
        let dir = ScratchDir::new("keep-test").unwrap();
        let path = dir.path().to_path_buf();
        dir.keep();
        assert!(path.is_dir());
        std::fs::remove_dir_all(path).unwrap();
        let gone = ScratchDir::new("keep-test").unwrap().path().to_path_buf();
        assert!(!gone.exists());
    }
}
