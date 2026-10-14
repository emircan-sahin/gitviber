//! Logins an agent saved from a tab (`session save`) to load into another: its cookies, and the
//! storage of the page's origin. They hold login tokens, so they're kept in the app's data folder,
//! a folder a repository, readable by this user alone; never printed in full nor sent anywhere.

use super::root_key;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Session {
    /// The page's origin as saved, where its storage is written back.
    pub origin: String,
    /// Seconds since 1970.
    pub saved: u64,
    pub cookies: Vec<Cookie>,
    pub local: Vec<(String, String)>,
    pub session: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    /// Seconds since 1970; None for one that ends with the browsing session.
    pub expires: Option<f64>,
    pub secure: bool,
    pub http_only: bool,
    /// "lax" or "strict"; None for WebKit's default.
    pub same_site: Option<String>,
}

/// Whether a page on `host` gets a cookie set for `domain` (a leading dot or not).
pub fn for_host(host: &str, domain: &str) -> bool {
    let domain = domain.trim_start_matches('.').to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
}

/// The sessions saved for one repository.
pub struct Shelf {
    dir: PathBuf,
}

impl Shelf {
    /// `data`: the app's data folder; `root`: the repository, as its tabs know it.
    pub fn of(data: &Path, root: &str) -> Self {
        let key = format!("{:x}", Sha256::digest(root_key(root)));
        Self {
            dir: data.join("browser-sessions").join(&key[..32]),
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json"))
    }

    /// Written whole or not at all, readable by this user alone.
    pub fn save(&self, name: &str, session: &Session) -> Result<(), String> {
        let failed = |e: std::io::Error| format!("Couldn't save the session: {e}");
        private_dirs(&self.dir).map_err(failed)?;
        let json = serde_json::to_vec(session).map_err(|e| e.to_string())?;
        let part = self.dir.join(format!(".{name}.part"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        std::io::Write::write_all(&mut options.open(&part).map_err(failed)?, &json)
            .map_err(failed)?;
        std::fs::rename(&part, self.file(name)).map_err(failed)
    }

    pub fn load(&self, name: &str) -> Result<Session, String> {
        let text = std::fs::read(self.file(name)).map_err(|_| {
            format!("No session {name}: `gitviber browser session list` names those saved")
        })?;
        serde_json::from_slice(&text).map_err(|_| format!("Session {name} isn't readable."))
    }

    /// By name, those that read.
    pub fn list(&self) -> Vec<(String, Session)> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return vec![];
        };
        let mut all: Vec<(String, Session)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.strip_suffix(".json")?.to_string();
                Some((name.clone(), self.load(&name).ok()?))
            })
            .collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    pub fn delete(&self, name: &str) -> Result<(), String> {
        std::fs::remove_file(self.file(name)).map_err(|_| format!("No session {name}"))
    }
}

/// The folder and those above it up to the app's data folder, the two of ours this user's alone.
fn private_dirs(dir: &Path) -> std::io::Result<()> {
    let shelf = dir.parent().unwrap_or(dir);
    std::fs::create_dir_all(shelf.parent().unwrap_or(shelf))?;
    for dir in [shelf, dir] {
        #[cfg(unix)]
        crate::local_socket::private_dir(dir)
            .ok_or_else(|| std::io::Error::other(format!("{} isn't ours alone", dir.display())))?;
        #[cfg(not(unix))]
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(origin: &str) -> Session {
        Session {
            origin: origin.into(),
            saved: 1_700_000_000,
            cookies: vec![Cookie {
                name: "sid".into(),
                value: "s3cret".into(),
                domain: ".app.test".into(),
                path: "/".into(),
                expires: Some(1_800_000_000.5),
                secure: true,
                http_only: true,
                same_site: Some("lax".into()),
            }],
            local: vec![("token".into(), "abc".into())],
            session: vec![],
        }
    }

    #[test]
    fn sessions_keep_to_their_repository_and_to_this_user() {
        let data = std::env::temp_dir().join(format!("gitviber-sessions-{}", std::process::id()));
        let one = Shelf::of(&data, "/w/one");
        let two = Shelf::of(&data, "/w/two");
        assert!(one.list().is_empty());
        one.save("mert", &session("http://localhost:3000")).unwrap();
        one.save("ayse", &session("https://app.test")).unwrap();
        assert_eq!(one.load("mert"), Ok(session("http://localhost:3000")));
        // Saved again: the newer one, whole.
        one.save("mert", &session("https://app.test")).unwrap();
        assert_eq!(one.load("mert").unwrap().origin, "https://app.test");
        let names: Vec<String> = one.list().into_iter().map(|s| s.0).collect();
        assert_eq!(names, ["ayse", "mert"]);
        assert!(two.list().is_empty() && two.load("mert").is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&one.file("mert")), 0o600);
            assert_eq!(mode(&one.dir), 0o700);
            assert_eq!(mode(one.dir.parent().unwrap()), 0o700);
        }
        one.delete("mert").unwrap();
        assert!(one.delete("mert").is_err() && one.load("mert").is_err());
        std::fs::write(one.file("broken"), "{").unwrap();
        assert_eq!(one.list().len(), 1, "one that doesn't read is left out");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_cookie_goes_to_its_domain_and_the_hosts_under_it() {
        assert!(for_host("app.test", ".app.test"));
        assert!(for_host("api.app.test", "app.test"));
        assert!(for_host("App.Test", "app.test"));
        assert!(!for_host("myapp.test", "app.test"));
        assert!(!for_host("app.test", "api.app.test"));
        assert!(!for_host("app.test", ""));
        assert!(!for_host("app.test", "."));
    }
}
