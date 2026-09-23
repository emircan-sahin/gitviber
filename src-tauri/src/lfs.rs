//! Git LFS files in a diff. git stores only a small pointer for them; the file itself is in
//! `.git/lfs/objects` once downloaded. A diff never downloads one: that's a network transfer
//! the user didn't ask for.

use crate::git;
use std::path::{Path, PathBuf};

const SPEC: &str = "version https://git-lfs.github.com/spec/v1";

#[derive(Debug, PartialEq)]
pub struct Pointer {
    /// sha256, 64 lowercase hex digits.
    pub oid: String,
    /// The real file's size in bytes.
    pub size: u64,
}

/// The pointer `bytes` are, if they are one. Pointers are under 1024 bytes by the spec.
pub fn pointer(bytes: &[u8]) -> Option<Pointer> {
    if bytes.len() >= 1024 {
        return None;
    }
    let mut lines = std::str::from_utf8(bytes).ok()?.lines();
    if lines.next()? != SPEC {
        return None;
    }
    let (mut oid, mut size) = (None, None);
    for line in lines {
        if let Some(h) = line.strip_prefix("oid sha256:") {
            oid = Some(h);
        } else if let Some(n) = line.strip_prefix("size ") {
            size = n.parse().ok();
        }
    }
    // Checked before it goes into a path.
    let oid = oid.filter(|h| {
        h.len() == 64
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })?;
    Some(Pointer {
        oid: oid.to_string(),
        size: size?,
    })
}

/// The downloaded object, if it is. Worktrees share the main repository's store.
pub fn object(repo: &Path, p: &Pointer) -> Option<PathBuf> {
    let common = git::run_text(repo, &["rev-parse", "--git-common-dir"]).ok()?;
    let path = repo
        .join(common.trim())
        .join("lfs/objects")
        .join(&p.oid[..2])
        .join(&p.oid[2..4])
        .join(&p.oid);
    path.is_file().then_some(path)
}

pub fn not_downloaded(p: &Pointer) -> String {
    format!("LFS object not downloaded ({})", size_label(p.size))
}

fn size_label(n: u64) -> String {
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    for unit in ["KB", "MB", "GB"] {
        if v < 1024.0 || unit == "GB" {
            return format!("{v:.1} {unit}");
        }
        v /= 1024.0;
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

    #[test]
    fn parses_pointers() {
        let text = format!("{SPEC}\noid sha256:{OID}\nsize 12345\n");
        assert_eq!(
            pointer(text.as_bytes()),
            Some(Pointer {
                oid: OID.into(),
                size: 12345
            })
        );
        // Extension lines are allowed between version and oid.
        let ext = format!("{SPEC}\next-0-foo sha256:{OID}\noid sha256:{OID}\nsize 1\n");
        assert!(pointer(ext.as_bytes()).is_some());
        assert_eq!(pointer(b"hello\n"), None);
        assert_eq!(
            pointer(format!("{SPEC}\noid sha256:{OID}\n").as_bytes()),
            None
        );
        let bad = format!("{SPEC}\noid sha256:../../{}\nsize 1\n", &OID[6..]);
        assert_eq!(pointer(bad.as_bytes()), None);
        let long = format!("{SPEC}\noid sha256:{OID}\nsize 1\n{}", "x".repeat(1024));
        assert_eq!(pointer(long.as_bytes()), None);
    }

    #[test]
    fn labels_sizes() {
        assert_eq!(size_label(900), "900 B");
        assert_eq!(size_label(13_002_342), "12.4 MB");
        assert_eq!(size_label(5 << 40), "5120.0 GB");
    }
}
