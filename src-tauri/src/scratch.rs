//! A folder of the app's own in the system's temp folder, for files a command needs on disk.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Readable only by the user (it can hold the repo's content), under a name no one can guess, and
/// removed with everything in it when dropped.
pub(crate) struct ScratchDir(PathBuf);

impl ScratchDir {
    pub(crate) fn new(purpose: &str) -> Result<Self, String> {
        let name = crate::local_socket::random_hex(8).ok_or("could not name a temporary folder")?;
        let dir = std::env::temp_dir().join(format!("gitviber-{purpose}-{name}"));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        // Fails if anything already sits at that path, so nobody can plant one for us.
        builder
            .create(&dir)
            .map_err(|e| format!("could not make a temporary folder: {e}"))?;
        Ok(ScratchDir(dir))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
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
        let _ = std::fs::remove_dir_all(&self.0);
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
}
