//! Logins an agent saved from a tab (`session save`) to load into another: its cookies, and the
//! storage of the page's origin. They hold login tokens: they're kept in the app's data folder,
//! in one folder for each repository, readable by this user alone, and never printed in full nor
//! sent anywhere.

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

/// What a worktree's sessions are kept by: its repository, so its other worktrees share them
/// (git's common folder); the folder itself outside one. Any other failure is said, not taken
/// for no repository, where the sessions saved would seem gone.
pub fn repository(root: &str) -> Result<String, String> {
    let root = Path::new(root);
    let common =
        crate::git::reading(|| crate::git::run_text(root, &["rev-parse", "--git-common-dir"]));
    match common {
        Ok(common) => root
            .join(common.trim())
            .canonicalize()
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| format!("Couldn't find the repository's sessions: {e}")),
        Err(e) if crate::git::outside_any_repository(&e) => Ok(root_key(&root.to_string_lossy())),
        Err(e) => Err(format!("Couldn't find the repository's sessions: {e}")),
    }
}

/// The sessions saved for one repository. Names are one whatever their case, as the Mac's
/// folders take them.
pub struct Shelf {
    dir: PathBuf,
}

impl Shelf {
    /// `data`: the app's data folder; `repository`: what `repository` gave.
    pub fn of(data: &Path, repository: &str) -> Self {
        let key = format!("{:x}", Sha256::digest(repository));
        Self {
            dir: shelves(data).join(&key[..32]),
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.json", name.to_ascii_lowercase()))
    }

    /// Written whole or not at all, readable by this user alone; over one of the same name only
    /// when `force`.
    pub fn save(&self, name: &str, session: &Session, force: bool) -> Result<(), String> {
        let failed = |e: std::io::Error| format!("Couldn't save the session: {e}");
        private_dirs(&self.dir).map_err(failed)?;
        let json = serde_json::to_string(session).map_err(|e| e.to_string())?;
        // Its own: two saves at once each write theirs whole.
        let part = self.dir.join(format!(
            ".{}.part",
            crate::local_socket::random_hex(8).ok_or("Couldn't save the session.")?
        ));
        let file = self.file(name);
        let placed = crate::scratch::write_private(&part, &json).and_then(|()| {
            if force {
                std::fs::rename(&part, &file)
            } else {
                // A link fails where a file is already, which a check before it wouldn't.
                std::fs::hard_link(&part, &file)
            }
        });
        let _ = std::fs::remove_file(&part);
        match placed {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(format!(
                "A session {name} is saved already: `session save {name} --force` replaces it"
            )),
            placed => placed.map_err(failed),
        }
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

    /// Every repository's, as Clear Browsing Data empties the browser.
    pub fn clear_all(data: &Path) -> std::io::Result<()> {
        match std::fs::remove_dir_all(shelves(data)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            done => done,
        }
    }
}

fn shelves(data: &Path) -> PathBuf {
    data.join("browser-sessions")
}

/// The app's data folder made if it isn't there, and below it the sessions' folder and the
/// repository's, each this user's alone.
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
        one.save("mert", &session("http://localhost:3000"), false)
            .unwrap();
        one.save("ayse", &session("https://app.test"), false)
            .unwrap();
        assert_eq!(one.load("mert"), Ok(session("http://localhost:3000")));
        // Saved again: refused, unless forced; then the newer one, whole.
        let again = one.save("mert", &session("https://app.test"), false);
        assert!(again.unwrap_err().contains("--force"));
        assert_eq!(one.load("mert").unwrap().origin, "http://localhost:3000");
        one.save("mert", &session("https://app.test"), true)
            .unwrap();
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
        let left: Vec<_> = std::fs::read_dir(&one.dir).unwrap().flatten().collect();
        assert_eq!(left.len(), 2, "no part of a save left behind");
        // Clear Browsing Data: every repository's go.
        Shelf::clear_all(&data).unwrap();
        assert!(one.list().is_empty() && !shelves(&data).exists());
        Shelf::clear_all(&data).unwrap();
        let _ = std::fs::remove_dir_all(&data);
    }

    /// Two tabs saving under one name at once (each tab has its own turn, so nothing orders
    /// them): the file reads as one of them whole, never a mix.
    #[test]
    fn saves_under_one_name_at_once_leave_one_whole_session() {
        let data =
            std::env::temp_dir().join(format!("gitviber-sessions-race-{}", std::process::id()));
        let shelf = std::sync::Arc::new(Shelf::of(&data, "/w/race"));
        let mut long = session("https://app.test");
        long.local = (0..400)
            .map(|i| (format!("k{i}"), "v".repeat(64)))
            .collect();
        let short = session("https://b.test");
        let (rounds, mut torn, mut failed) = (3000, 0, 0);
        for _ in 0..rounds {
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let saves: Vec<_> = [long.clone(), short.clone()]
                .into_iter()
                .map(|s| {
                    let (shelf, barrier) = (shelf.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        shelf.save("same", &s, true)
                    })
                })
                .collect();
            let said: Vec<_> = saves.into_iter().map(|t| t.join().unwrap()).collect();
            let got = shelf.load("same");
            if got != Ok(long.clone()) && got != Ok(short.clone()) {
                torn += 1;
            }
            failed += said.iter().filter(|s| s.is_err()).count();
        }
        let _ = std::fs::remove_dir_all(&data);
        assert_eq!(
            torn, 0,
            "of {rounds} rounds, {torn} left a file neither save wrote ({failed} saves failed)"
        );
    }

    /// macOS folders ignore case by default: names apart only in case are one session, whatever
    /// the folder does.
    #[test]
    fn names_that_differ_only_in_case_are_one_session() {
        let data =
            std::env::temp_dir().join(format!("gitviber-sessions-case-{}", std::process::id()));
        let shelf = Shelf::of(&data, "/w/case");
        shelf
            .save("Prod", &session("https://prod.test"), false)
            .unwrap();
        let taken = shelf.save("prod", &session("http://localhost:3000"), false);
        let kept = shelf.load("PROD").map(|s| s.origin);
        shelf
            .save("prod", &session("http://localhost:3000"), true)
            .unwrap();
        let replaced = shelf.load("Prod").map(|s| s.origin);
        let names: Vec<String> = shelf.list().into_iter().map(|s| s.0).collect();
        let _ = std::fs::remove_dir_all(&data);
        assert!(taken.is_err(), "prod is Prod");
        assert_eq!(kept, Ok("https://prod.test".into()));
        assert_eq!(replaced, Ok("http://localhost:3000".into()));
        assert_eq!(names, ["prod"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_repositorys_worktrees_share_its_sessions() {
        let base =
            std::env::temp_dir().join(format!("gitviber-sessions-repo-{}", std::process::id()));
        let (main, linked, plain) = (base.join("main"), base.join("linked"), base.join("plain"));
        std::fs::create_dir_all(&main).unwrap();
        std::fs::create_dir_all(&plain).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@example.test"])
                .args(args)
                .current_dir(&main)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["commit", "-q", "--allow-empty", "-m", "first"]);
        git(&["worktree", "add", "-q", "../linked"]);
        let key = |p: &Path| repository(&p.to_string_lossy()).unwrap();
        let shared = key(&main);
        assert_eq!(key(&linked), shared);
        assert_eq!(key(&main.join(".")), shared);
        // Outside a repository, the folder itself.
        assert_eq!(key(&plain), root_key(&plain.to_string_lossy()));
        // Outside it through a symlink: the folder it leads to.
        std::os::unix::fs::symlink(&plain, base.join("alias")).unwrap();
        assert_eq!(key(&base.join("alias")), key(&plain));
        // A folder git can't even start in isn't taken for one outside a repository, nor is a
        // worktree whose repository moved away.
        assert!(repository("/no/such/folder").is_err());
        std::fs::rename(&main, base.join("moved")).unwrap();
        assert!(repository(&linked.to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// git answers a subfolder with a path relative to it (`../../.git`), and a symlinked one
    /// with its own spelling: both still reach the repository's sessions.
    #[cfg(unix)]
    #[test]
    fn a_pane_in_a_subfolder_or_through_a_symlink_keeps_to_its_repositorys_sessions() {
        let base =
            std::env::temp_dir().join(format!("gitviber-sessions-sub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (main, deeper) = (base.join("main"), base.join("main/app/src"));
        std::fs::create_dir_all(&deeper).unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&main)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git init");
        std::os::unix::fs::symlink(&main, base.join("alias")).unwrap();
        let key = |p: &Path| repository(&p.to_string_lossy()).unwrap();
        let shared = key(&main);
        assert_eq!(key(&deeper), shared);
        assert_eq!(key(&base.join("alias")), shared);
        assert_eq!(key(&base.join("alias/app")), shared);
        let _ = std::fs::remove_dir_all(&base);
    }
}
