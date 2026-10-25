//! Files only the user can read, written whole or not at all: the agents' skill and saved
//! sessions. Core keeps its own copy of these crate-private.

use std::path::Path;

/// `text` in a new file at `path` only the user can read.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(text.as_bytes())
}

/// `text` at `path`, readable by the user alone: written beside it as `.<name>.<random>.part`,
/// then renamed over what's there (`replace`; a link is replaced, never written through), or
/// else linked, which fails with AlreadyExists where anything is.
pub(crate) fn place_private(path: &Path, text: &str, replace: bool) -> std::io::Result<()> {
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(std::io::Error::other(format!(
            "{} isn't a file's path",
            path.display()
        )));
    };
    let hex = gitviber_core::local_socket::random_hex(8)
        .ok_or_else(|| std::io::Error::other("couldn't name a temporary file"))?;
    // Its own: two writers at once each write theirs whole.
    let part = folder.join(format!(".{}.{hex}.part", name.to_string_lossy()));
    let placed = write_private(&part, text).and_then(|()| {
        if replace {
            std::fs::rename(&part, path)
        } else {
            // A link fails where a file is already, which a check before it wouldn't.
            std::fs::hard_link(&part, path)
        }
    });
    let _ = std::fs::remove_file(&part);
    placed
}

/// A folder of the test's own in the temp folder, gone with everything in it when dropped.
#[cfg(test)]
pub(crate) struct ScratchDir(std::path::PathBuf);

#[cfg(test)]
impl ScratchDir {
    pub(crate) fn new(purpose: &str) -> Result<Self, String> {
        let name = gitviber_core::local_socket::random_hex(8).ok_or("no random name")?;
        let dir = std::env::temp_dir().join(format!("gitviber-{purpose}-{name}"));
        std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
        Ok(Self(dir))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
