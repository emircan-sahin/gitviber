//! What an agent's tab downloads (macos/downloads.rs): saved in a folder of the tab's own under
//! the app's cache, never the repository or ~/Downloads, gone with the tab; `downloads` lists
//! it and `wait --download` waits for the next. Each is told once, by the pane's next reply
//! after it ends: a `wait --download` on its tab returns it, any other reply notes it. Apart
//! from AppKit, so it can be tested.

use super::control::Reply;
use super::notes;
use crate::state::Res;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Manager};

const FOLDER: &str = "browser-downloads";
/// A tab keeps its newest files, a day at most: an agent reads one soon after it lands, and a
/// tab may download in a loop.
const KEEP: usize = 20;
const KEEP_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// The longest name a file gets, in bytes, under the file system's 255.
const NAME_MAX: usize = 200;
/// The longest extension kept apart from its name when one is cut.
const EXT_MAX: usize = 16;
const POLL: Duration = Duration::from_millis(200);

/// The app's cache folder, found once for everything here.
static CACHE: OnceLock<PathBuf> = OnceLock::new();

pub fn cache(app: &AppHandle) -> Res<PathBuf> {
    if let Some(cache) = CACHE.get() {
        return Ok(cache.clone());
    }
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    Ok(CACHE.get_or_init(|| cache).clone())
}

/// Every tab's downloads, under the app's cache folder.
pub fn base(cache: &Path) -> PathBuf {
    cache.join(FOLDER)
}

/// Tab `id`'s folder, made, with what's past keeping gone: its own old files, and other tabs'
/// folders untouched for a day (left by a run that ended without closing them), never one
/// still `live`.
pub fn tab_folder(
    cache: &Path,
    id: &str,
    now: SystemTime,
    live: impl Fn(&str) -> bool,
) -> std::io::Result<PathBuf> {
    let base = base(cache);
    let folder = base.join(id);
    std::fs::create_dir_all(&folder)?;
    for entry in std::fs::read_dir(&base)?.flatten() {
        let theirs = entry.file_name().to_string_lossy().into_owned();
        if theirs != id && !live(&theirs) && older(&entry.path(), now, KEEP_FOR) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    prune(&folder, KEEP - 1, now);
    Ok(folder)
}

/// Tab `id`'s folder gone, with the tab.
pub fn remove_folder(id: &str) {
    if let Some(cache) = CACHE.get() {
        let _ = std::fs::remove_dir_all(base(cache).join(id));
    }
}

fn older(path: &Path, now: SystemTime, age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|at| now.duration_since(at).is_ok_and(|a| a > age))
}

/// The folder's files, newest first: name, path, size, when last written.
fn files(folder: &Path) -> Vec<(String, PathBuf, u64, SystemTime)> {
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            let name = e.file_name().to_string_lossy().into_owned();
            Some((name, e.path(), meta.len(), meta.modified().ok()?))
        })
        .collect();
    files.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
    files
}

/// Past the newest `keep`, or older than KEEP_FOR, gone; but one still downloading.
pub fn prune(folder: &Path, keep: usize, now: SystemTime) {
    let busy = state().busy();
    for (at, (_, path, _, modified)) in files(folder).into_iter().enumerate() {
        let old = now.duration_since(modified).is_ok_and(|a| a > KEEP_FOR);
        if (at >= keep || old) && !busy.contains(&path) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Characters that show nothing but turn text around or hide in it: `cod.exe` written as
/// `exe.doc` with a right-to-left override.
fn unseen(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
}

/// A file name from the one the server or page suggests: no folders, nothing hidden, unseen or
/// empty, and short enough, its extension kept.
pub fn file_name(suggested: &str) -> String {
    let last = suggested.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = last
        .chars()
        .filter(|&c| !unseen(c))
        .map(|c| if c.is_control() || c == ':' { '_' } else { c })
        .collect();
    let clean = clean.trim().trim_start_matches('.').trim();
    if clean.is_empty() {
        return "download".into();
    }
    if clean.len() <= NAME_MAX {
        return clean.into();
    }
    // A part after the last dot too long to be an extension is part of the name.
    let (stem, ext) = match split(clean) {
        (stem, ext) if ext.len() <= EXT_MAX => (stem, ext),
        _ => (clean, ""),
    };
    let mut stem = stem.to_string();
    while stem.len() + ext.len() > NAME_MAX {
        stem.pop();
    }
    format!("{stem}{ext}")
}

/// `report.pdf` as (`report`, `.pdf`); `.tar.gz` stays whole in the extension.
fn split(name: &str) -> (&str, &str) {
    let at = name
        .strip_suffix(".gz")
        .and_then(|n| n.strip_suffix(".tar"))
        .map(|n| n.len())
        .or_else(|| name.rfind('.').filter(|&at| at > 0));
    at.map_or((name, ""), |at| name.split_at(at))
}

/// `name` in `folder`, or `name (2).ext`, `(3)`, ... while `taken` says it's there.
pub fn free_path(folder: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let (stem, ext) = split(name);
    (1..)
        .map(|n| match n {
            1 => folder.join(name),
            n => folder.join(format!("{stem} ({n}){ext}")),
        })
        .find(|p| !taken(p))
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Done {
        name: String,
        path: PathBuf,
        size: u64,
        mime: String,
    },
    /// Failed, or cancelled (said so in `why`).
    Failed { name: String, why: String },
}

impl Outcome {
    /// As a reply's note tells it.
    pub fn note(&self) -> String {
        match self {
            Self::Done {
                name, path, size, ..
            } => format!(
                "downloaded {name} ({}): {}",
                size_text(*size),
                path.display()
            ),
            Self::Failed { name, why } => format!("download of {name} failed: {why}"),
        }
    }
}

/// A tab's downloads going, and the ones ended that no reply has told yet.
#[derive(Default)]
struct Tab {
    busy: HashSet<PathBuf>,
    ended: Vec<Outcome>,
    /// A `wait --download` is on it: its ended ones are the wait's, not another reply's.
    waiting: bool,
    /// Its pane and name, for the note another reply tells.
    pane: Option<(u32, String)>,
}

#[derive(Default)]
pub struct Downloads {
    tabs: HashMap<String, Tab>,
}

impl Downloads {
    /// A download of tab `id` began; `pane`: the tab's pane and name.
    pub fn began(&mut self, id: &str, pane: Option<(u32, String)>) {
        let tab = self.tabs.entry(id.into()).or_default();
        tab.pane = pane.or(tab.pane.take());
    }

    /// A path for `name` in `folder` that's free: nothing on disk there (a dangling link counts),
    /// and no download still going holds it, as WebKit makes the file only once data comes.
    /// Held from now until it ends.
    pub fn claim(&mut self, id: &str, folder: &Path, name: &str) -> PathBuf {
        let busy = self.busy();
        let path = free_path(folder, name, |p| {
            busy.contains(p) || p.symlink_metadata().is_ok()
        });
        let tab = self.tabs.entry(id.into()).or_default();
        tab.busy.insert(path.clone());
        path
    }

    /// One of the tab's ended, held for the next reply to tell: whether a wait is on to take it.
    /// Nothing for a tab gone meanwhile.
    pub fn ended(&mut self, id: &str, path: Option<&Path>, outcome: Outcome) -> bool {
        let Some(tab) = self.tabs.get_mut(id) else {
            return false;
        };
        if let Some(path) = path {
            tab.busy.remove(path);
        }
        tab.ended.push(outcome);
        tab.waiting
    }

    fn wait_on(&mut self, id: &str) {
        self.tabs.entry(id.into()).or_default().waiting = true;
    }

    fn take(&mut self, id: &str) -> Option<Outcome> {
        let tab = self.tabs.get_mut(id)?;
        (!tab.ended.is_empty()).then(|| tab.ended.remove(0))
    }

    fn stop(&mut self, id: &str) {
        if let Some(tab) = self.tabs.get_mut(id) {
            tab.waiting = false;
        }
    }

    /// What pane `pty`'s reply tells of its tabs' downloads, as notes: (tab id, name, note).
    /// Not one a wait is on, which takes its own.
    fn told(&mut self, pty: u32) -> Vec<(String, String, String)> {
        let mut told = vec![];
        for (id, tab) in &mut self.tabs {
            let Some((_, name)) = tab.pane.as_ref().filter(|p| p.0 == pty && !tab.waiting) else {
                continue;
            };
            for outcome in tab.ended.drain(..) {
                told.push((id.clone(), name.clone(), outcome.note()));
            }
        }
        told
    }

    fn busy(&self) -> HashSet<PathBuf> {
        self.tabs.values().flat_map(|t| t.busy.clone()).collect()
    }

    /// Whether tab `id` has downloaded in this run and is still open.
    pub fn live(&self, id: &str) -> bool {
        self.tabs.contains_key(id)
    }

    pub fn ids(&self) -> Vec<String> {
        self.tabs.keys().cloned().collect()
    }

    pub fn forget(&mut self, id: &str) {
        self.tabs.remove(id);
    }
}

/// Written on the main thread, read by a command's waiting on its own.
static STATE: LazyLock<Mutex<Downloads>> = LazyLock::new(Mutex::default);

pub fn state() -> MutexGuard<'static, Downloads> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Pane `pty`'s downloads no reply has told, now notes for the one being made (notes.rs).
pub fn deliver(pty: u32) {
    let told = state().told(pty);
    for (id, name, note) in told {
        notes::note(pty, Some((&id, &name)), note);
    }
}

/// `wait --download`: the download of tab `id` that ended since the pane's last reply, else the
/// next to end.
pub fn wait(id: &str, timeout: Duration) -> Res<String> {
    let until = Instant::now() + timeout;
    state().wait_on(id);
    let outcome = loop {
        let mut s = state();
        let outcome = s.take(id);
        if outcome.is_some() || Instant::now() >= until {
            // In one hold of the lock: one ending now is this wait's, or held for its reply.
            s.stop(id);
            break outcome;
        }
        drop(s);
        std::thread::sleep(POLL);
    };
    match outcome {
        Some(Outcome::Done {
            name,
            path,
            size,
            mime,
        }) => Ok(format!(
            "Downloaded {name} ({}, {mime}): {}",
            size_text(size),
            path.display()
        )),
        Some(Outcome::Failed { name, why }) => Err(format!("Download of {name} failed: {why}")),
        None => Err(format!(
            "Timed out after {} ms waiting for a download",
            timeout.as_millis()
        )),
    }
}

/// `downloads [--clear]` in the tab's folder under `cache`.
pub fn listing(cache: &Path, id: &str, clear: bool, now: SystemTime) -> Reply {
    let folder = base(cache).join(id);
    let files = files(&folder);
    if clear {
        let busy = state().busy();
        let gone = files
            .iter()
            .filter(|f| !busy.contains(&f.1) && std::fs::remove_file(&f.1).is_ok())
            .count();
        return Reply::out(format!("Cleared {gone} downloaded files."));
    }
    if files.is_empty() {
        return Reply::data("No downloads in this tab.", json!({ "downloads": [] }));
    }
    let busy = state().busy();
    let lines = files.iter().map(|(name, path, size, modified)| {
        let age = now.duration_since(*modified).unwrap_or_default();
        let still = if busy.contains(path) {
            "  (downloading)"
        } else {
            ""
        };
        format!(
            "{name}  {}  {}  {}{still}",
            size_text(*size),
            super::agent::ago(age),
            path.display()
        )
    });
    let data = files.iter().map(|(name, path, size, modified)| {
        let ms = modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        json!({ "name": name, "path": path, "size": size, "modified_ms": ms, "downloading": busy.contains(path) })
    });
    Reply::data(
        lines.collect::<Vec<_>>().join("\n"),
        json!({ "downloads": data.collect::<Vec<_>>() }),
    )
}

/// A size as a person reads it: 512 B, 24 KB, 3.4 MB.
pub fn size_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{} KB", (b / KB).round()),
        1_048_576..1_073_741_824 => format!("{:.1} MB", b / KB / KB),
        _ => format!("{:.1} GB", b / KB / KB / KB),
    }
}

/// Whether a response's Content-Disposition asks for a download over showing it.
pub fn attachment(disposition: &str) -> bool {
    disposition
        .trim_start()
        .get(..10)
        .is_some_and(|d| d.eq_ignore_ascii_case("attachment"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gv-downloads-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, age: Duration) {
        std::fs::write(path, b"x").unwrap();
        let at = SystemTime::now() - age;
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    #[test]
    fn a_suggested_name_is_one_file_in_the_folder_never_a_path_or_hidden() {
        assert_eq!(file_name("report.pdf"), "report.pdf");
        assert_eq!(file_name("../../etc/passwd"), "passwd");
        assert_eq!(file_name("..\\..\\boot.ini"), "boot.ini");
        assert_eq!(file_name(".bashrc"), "bashrc");
        assert_eq!(file_name(""), "download");
        assert_eq!(file_name(".."), "download");
        assert_eq!(file_name("a\u{0}b\nc:d.txt"), "a_b_c_d.txt");
        let long = format!("{}.pdf", "é".repeat(300));
        let name = file_name(&long);
        assert!(name.len() <= NAME_MAX && name.ends_with(".pdf"), "{name}");
        // Turned around or hidden, it'd read as another file than it is.
        assert_eq!(file_name("invoice\u{202E}fdp.exe"), "invoicefdp.exe");
        assert_eq!(
            file_name("\u{200E}a\u{2066}b\u{2069}\u{200F}.txt"),
            "ab.txt"
        );
        // A tail too long for an extension stays the name's, cut.
        let name = file_name(&format!("a.{}", "b".repeat(300)));
        assert_eq!(name, format!("a.{}", "b".repeat(NAME_MAX - 2)));
    }

    #[test]
    fn a_name_taken_gets_the_next_number_before_its_extension() {
        let dir = Path::new("/d");
        let taken =
            |names: &'static [&'static str]| move |p: &Path| names.iter().any(|n| p == dir.join(n));
        assert_eq!(
            free_path(dir, "report.pdf", taken(&[])),
            dir.join("report.pdf")
        );
        assert_eq!(
            free_path(dir, "report.pdf", taken(&["report.pdf", "report (2).pdf"])),
            dir.join("report (3).pdf")
        );
        assert_eq!(
            free_path(dir, "data.tar.gz", taken(&["data.tar.gz"])),
            dir.join("data (2).tar.gz")
        );
        assert_eq!(
            free_path(dir, "README", taken(&["README"])),
            dir.join("README (2)")
        );
    }

    #[test]
    fn two_downloads_of_one_name_get_two_files_and_anything_on_disk_counts() {
        let folder = scratch("claim");
        let mut d = Downloads::default();
        // WebKit makes neither file yet: the first one going holds its name.
        let first = d.claim("a", &folder, "report.pdf");
        let second = d.claim("b", &folder, "report.pdf");
        assert_eq!(first, folder.join("report.pdf"));
        assert_eq!(second, folder.join("report (2).pdf"));
        // Ended, its name is free again unless its file stays.
        d.ended(
            "a",
            Some(&first),
            Outcome::Failed {
                name: "report.pdf".into(),
                why: "x".into(),
            },
        );
        assert_eq!(d.claim("c", &folder, "report.pdf"), first);
        std::fs::write(folder.join("data.csv"), b"x").unwrap();
        std::os::unix::fs::symlink(folder.join("nowhere"), folder.join("link.txt")).unwrap();
        assert_eq!(
            d.claim("a", &folder, "data.csv"),
            folder.join("data (2).csv")
        );
        assert_eq!(
            d.claim("a", &folder, "link.txt"),
            folder.join("link (2).txt")
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_tab_keeps_its_newest_files_a_day_at_most_and_old_tabs_folders_go() {
        let cache = scratch("prune");
        let now = SystemTime::now();
        let (stale, other) = (base(&cache).join("old-tab"), base(&cache).join("tab-2"));
        for dir in [&stale, &other] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::File::open(&stale)
            .and_then(|f| f.set_modified(now - KEEP_FOR * 2))
            .unwrap();
        let folder = base(&cache).join("tab-1");
        std::fs::create_dir_all(&folder).unwrap();
        for i in 0..KEEP + 3 {
            touch(
                &folder.join(format!("f{i}")),
                Duration::from_secs(100 - i as u64),
            );
        }
        touch(&folder.join("ancient"), KEEP_FOR * 2);
        let live = base(&cache).join("tab-live");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::File::open(&live)
            .and_then(|f| f.set_modified(now - KEEP_FOR * 2))
            .unwrap();
        let open = |id: &str| id == "tab-live";
        assert_eq!(tab_folder(&cache, "tab-1", now, open).unwrap(), folder);
        let left: Vec<String> = files(&folder).into_iter().map(|f| f.0).collect();
        assert_eq!(left.len(), KEEP - 1, "room for the one coming");
        assert_eq!(left[0], format!("f{}", KEEP + 2), "the newest stay");
        assert!(!left.contains(&"ancient".to_string()));
        assert!(!stale.exists(), "untouched for a day, a closed tab's goes");
        assert!(other.exists(), "a recent one stays");
        assert!(live.exists(), "an open tab's stays, however old");
        // A day on, nothing of a tab is kept.
        prune(&folder, KEEP, now + KEEP_FOR * 2);
        assert!(files(&folder).is_empty());
        let _ = std::fs::remove_dir_all(&cache);
    }

    fn done(name: &str) -> Outcome {
        Outcome::Done {
            name: name.into(),
            path: PathBuf::from(format!("/c/{name}")),
            size: 24,
            mime: "application/pdf".into(),
        }
    }

    #[test]
    fn a_download_is_held_for_the_next_reply_and_a_wait_on_keeps_its_own() {
        let mut d = Downloads::default();
        d.began("a", Some((1, "t1".into())));
        d.began("b", Some((1, "t2".into())));
        let path = d.claim("a", Path::new("/c"), "a.pdf");
        assert_eq!(d.busy(), HashSet::from([path.clone()]));
        assert!(!d.ended("a", Some(&path), done("a.pdf")));
        assert!(d.busy().is_empty());
        // A wait on b keeps b's from another reply.
        d.wait_on("b");
        assert!(d.ended("b", None, done("b.pdf")));
        let told = d.told(1);
        assert_eq!(told.len(), 1);
        assert_eq!((told[0].0.as_str(), told[0].1.as_str()), ("a", "t1"));
        assert!(told[0].2.starts_with("downloaded a.pdf (24 B)"));
        assert!(d.told(1).is_empty() && d.take("a").is_none(), "told once");
        assert_eq!(d.take("b"), Some(done("b.pdf")));
        // Another pane's reply tells none of these.
        d.stop("b");
        d.ended("b", None, done("c.pdf"));
        assert!(d.told(2).is_empty());
        assert_eq!(d.told(1).len(), 1);
        d.forget("a");
        assert!(!d.live("a") && !d.ended("a", None, done("e.pdf")));
    }

    #[test]
    fn click_then_wait_gets_the_download_that_ended_between() {
        let (id, pty) = ("tab-click-wait", 9101);
        state().began(id, Some((pty, "t1".into())));
        // The click's reply went before the download ended.
        assert!(notes::take(pty, Some(id)).is_empty());
        assert!(!state().ended(id, None, done("a.pdf")));
        let waited = wait(id, Duration::from_millis(300));
        let told = notes::take(pty, Some(id));
        state().forget(id);
        assert!(waited
            .unwrap()
            .starts_with("Downloaded a.pdf (24 B, application/pdf)"));
        assert!(told.is_empty(), "told twice: {told:?}");
    }

    #[test]
    fn click_then_another_command_tells_it_and_the_wait_after_has_none() {
        let (id, pty) = ("tab-told", 9102);
        state().began(id, Some((pty, "t1".into())));
        assert!(!state().ended(id, None, done("a.pdf")));
        // The command between: its reply notes it.
        let told = notes::take(pty, Some(id));
        assert_eq!(told.len(), 1);
        assert!(told[0].starts_with("→ downloaded a.pdf"), "{told:?}");
        let next = wait(id, Duration::from_millis(300));
        state().forget(id);
        assert!(
            next.unwrap_err().starts_with("Timed out"),
            "the wait returned the told one"
        );
    }

    #[test]
    fn a_wait_tells_the_download_that_ends_while_it_waits() {
        let (id, pty) = ("tab-waits", 9103);
        state().began(id, Some((pty, "t1".into())));
        let ender = std::thread::spawn(move || {
            std::thread::sleep(POLL);
            let failed = Outcome::Failed {
                name: "x.zip".into(),
                why: "cancelled".into(),
            };
            state().ended(id, None, failed)
        });
        let waited = wait(id, Duration::from_secs(5));
        assert!(ender.join().unwrap(), "a wait was on");
        let told = notes::take(pty, Some(id));
        state().forget(id);
        assert_eq!(waited, Err("Download of x.zip failed: cancelled".into()));
        assert!(told.is_empty(), "told twice: {told:?}");
    }

    #[test]
    fn outcomes_and_sizes_read_as_an_agent_needs() {
        let done = Outcome::Done {
            name: "report.pdf".into(),
            path: "/c/report.pdf".into(),
            size: 24 * 1024 + 100,
            mime: "application/pdf".into(),
        };
        assert_eq!(done.note(), "downloaded report.pdf (24 KB): /c/report.pdf");
        let failed = Outcome::Failed {
            name: "big.zip".into(),
            why: "cancelled".into(),
        };
        assert_eq!(failed.note(), "download of big.zip failed: cancelled");
        assert_eq!(size_text(0), "0 B");
        assert_eq!(size_text(1023), "1023 B");
        assert_eq!(size_text(1024), "1 KB");
        assert_eq!(size_text(5 * 1024 * 1024 + 300_000), "5.3 MB");
        assert_eq!(size_text(3 << 30), "3.0 GB");
        for yes in [
            "attachment",
            "Attachment; filename=\"a.csv\"",
            "  ATTACHMENT;x",
        ] {
            assert!(attachment(yes), "{yes}");
        }
        for no in ["inline", "", "attach", "inline; filename=attachment.csv"] {
            assert!(!attachment(no), "{no}");
        }
    }

    #[test]
    fn the_list_is_newest_first_and_clear_leaves_one_still_downloading() {
        let cache = scratch("list");
        let folder = tab_folder(&cache, "tab-l", SystemTime::now(), |_| false).unwrap();
        touch(&folder.join("old.csv"), Duration::from_secs(600));
        touch(&folder.join("new.pdf"), Duration::from_secs(5));
        assert_eq!(
            state().claim("tab-l", &folder, "part.zip"),
            folder.join("part.zip")
        );
        touch(&folder.join("part.zip"), Duration::from_secs(1));
        let reply = listing(&cache, "tab-l", false, SystemTime::now());
        let out = reply.out.unwrap();
        let names: Vec<&str> = out.lines().map(|l| l.split("  ").next().unwrap()).collect();
        assert_eq!(names, ["part.zip", "new.pdf", "old.csv"]);
        assert!(out.lines().next().unwrap().ends_with("(downloading)"));
        assert!(out.contains("10m ago"));
        assert_eq!(reply.data.unwrap()["downloads"][1]["name"], "new.pdf");
        let cleared = listing(&cache, "tab-l", true, SystemTime::now());
        assert_eq!(cleared.out.unwrap(), "Cleared 2 downloaded files.");
        assert_eq!(files(&folder).len(), 1);
        state().forget("tab-l");
        let empty = listing(&cache, "nothing", false, SystemTime::now());
        assert_eq!(empty.out.unwrap(), "No downloads in this tab.");
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn a_suggested_absolute_or_dotted_name_still_lands_in_the_folder() {
        assert_eq!(file_name("/etc/passwd"), "passwd");
        assert_eq!(file_name("C:\\Windows\\win.ini"), "win.ini");
        assert_eq!(file_name("reports/"), "download");
        assert_eq!(file_name("."), "download");
        assert_eq!(file_name("..."), "download");
        assert_eq!(file_name("  ..  "), "download");
        assert_eq!(file_name("a\u{0}"), "a_");
        for long in ["x".repeat(400), format!("a.{}", "b".repeat(300))] {
            let name = file_name(&long);
            assert!(!name.is_empty() && name.len() <= NAME_MAX, "{name}");
            assert!(!name.contains('/'), "{name}");
        }
        // Room left for free_path's " (n)" under the file system's 255 bytes.
        let taken = free_path(Path::new("/d"), &file_name(&"é".repeat(300)), |_| false);
        assert!(taken.file_name().unwrap().len() + " (99)".len() <= 255);
    }
}
