//! The app's own unix sockets, for helpers this binary runs as (askpass.rs, browser/client.rs):
//! each in a folder only this user can enter, one JSON line asked and one answered.

/// `n` random bytes, in hex.
pub fn random_hex(n: usize) -> Option<String> {
    use std::io::Read;
    let mut bytes = vec![0u8; n];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(unix)]
pub use unix::{ask_line, private_dir, serve_line, AskError};

#[cfg(unix)]
mod unix {
    use serde::{de::DeserializeOwned, Serialize};
    use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::time::Duration;

    /// A folder of this user's alone, made so or found so: a file, a link or another user's
    /// folder there, and none.
    pub fn private_dir(dir: &Path) -> Option<()> {
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
        let meta = std::fs::symlink_metadata(dir).ok()?;
        if !meta.is_dir() || meta.uid() != unsafe { libc::getuid() } {
            return None;
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).ok()
    }

    /// One request line read (at most `max` bytes, `wait` for it), `answer` given it (None for
    /// anything that isn't one), and its reply written back.
    pub fn serve_line<Req: DeserializeOwned, Rep: Serialize>(
        stream: &UnixStream,
        max: u64,
        wait: Duration,
        answer: impl FnOnce(Option<Req>) -> Rep,
    ) {
        let request = stream.set_read_timeout(Some(wait)).ok().and_then(|()| {
            let mut line = String::new();
            BufReader::new(Read::take(stream, max))
                .read_line(&mut line)
                .ok()?;
            serde_json::from_str(&line).ok()
        });
        let reply = answer(request);
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        if let Ok(line) = serde_json::to_string(&reply) {
            let _ = (&*stream).write_all((line + "\n").as_bytes());
        }
    }

    pub enum AskError {
        /// Nothing listening there; how it failed.
        Connect(ErrorKind),
        /// Connected, but no reply came that reads as one.
        NoAnswer,
    }

    /// The helper's side: one request line, one reply line, waiting `wait` at most (None: as long
    /// as it takes). Not until the connection closes: on macOS an accepted socket is marked
    /// close-on-exec a step after accept(), and a child spawned in between keeps it open after
    /// the app has answered.
    pub fn ask_line<Req: Serialize, Rep: DeserializeOwned>(
        socket: &Path,
        request: &Req,
        wait: Option<Duration>,
    ) -> Result<Rep, AskError> {
        let mut stream = UnixStream::connect(socket).map_err(|e| AskError::Connect(e.kind()))?;
        let _ = stream.set_read_timeout(wait);
        let mut line = serde_json::to_string(request).map_err(|_| AskError::NoAnswer)?;
        line.push('\n');
        stream
            .write_all(line.as_bytes())
            .map_err(|_| AskError::NoAnswer)?;
        let mut reply = String::new();
        BufReader::new(&stream)
            .read_line(&mut reply)
            .map_err(|_| AskError::NoAnswer)?;
        serde_json::from_str(&reply).map_err(|_| AskError::NoAnswer)
    }
}
