//! A folder of the app's own in the system's temp folder, for files a command needs on disk.

use std::path::{Path, PathBuf};

/// Readable only by the user (it can hold the repo's content), under a name no one can guess, and
/// removed with everything in it when dropped.
pub(crate) struct ScratchDir(PathBuf);

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
        Ok(ScratchDir(dir))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
