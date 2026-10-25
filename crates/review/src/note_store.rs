//! The review notes of each worktree (model::notes), kept in the app's config dir, never in the
//! repo: the 30 worktrees noted on most recently, as the old app kept them.

use std::collections::VecDeque;
use std::path::PathBuf;

use gpui_kit::{App, AppContext as _, BorrowAppContext as _, Global};
use serde_json::Value;

use crate::model::notes::ReviewNote;

const MAX_ROOTS: usize = 30;

#[derive(Default)]
pub struct ReviewNotes {
    loaded: bool,
    /// Worktree root → its notes, the one noted on last at the end.
    roots: VecDeque<(String, Vec<ReviewNote>)>,
}

impl Global for ReviewNotes {}

fn path() -> PathBuf {
    gitviber_ui::store::config_dir().join("review-notes.json")
}

/// `root → notes` as stored (this build's file, or the old app's), notes that aren't dropped.
fn read(v: Value) -> Vec<(String, Vec<ReviewNote>)> {
    let Value::Object(map) = v else { return vec![] };
    map.into_iter()
        .filter_map(|(root, notes)| {
            let notes: Vec<ReviewNote> = notes
                .as_array()?
                .iter()
                .filter_map(ReviewNote::read)
                .collect();
            (!notes.is_empty()).then_some((root, notes))
        })
        .collect()
}

impl ReviewNotes {
    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Ok(text) = std::fs::read_to_string(path()) else {
            return;
        };
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => self.take(read(v)),
            Err(_) => {
                gitviber_ui::store::quarantine(&path());
            }
        }
    }

    fn take(&mut self, roots: Vec<(String, Vec<ReviewNote>)>) {
        for (root, notes) in roots {
            self.put(root, notes);
        }
    }

    fn put(&mut self, root: String, notes: Vec<ReviewNote>) {
        self.roots.retain(|(r, _)| *r != root);
        if !notes.is_empty() {
            self.roots.push_back((root, notes));
        }
        while self.roots.len() > MAX_ROOTS {
            self.roots.pop_front();
        }
    }

    fn write(&self, cx: &mut App) {
        let map: serde_json::Map<String, Value> = self
            .roots
            .iter()
            .filter_map(|(root, notes)| Some((root.clone(), serde_json::to_value(notes).ok()?)))
            .collect();
        if let Ok(text) = serde_json::to_string(&Value::Object(map)) {
            gitviber_ui::store::write_in_background(cx, path(), text);
        }
    }

    /// `root`'s notes, in the order they were written.
    pub fn get(cx: &mut App, root: &str) -> Vec<ReviewNote> {
        if !cx.try_global::<ReviewNotes>().is_some_and(|n| n.loaded) {
            cx.default_global::<ReviewNotes>().ensure_loaded();
        }
        cx.global::<ReviewNotes>()
            .roots
            .iter()
            .find(|(r, _)| r == root)
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
    }

    /// Replaces `root`'s notes (none: forgets the worktree) and saves them.
    pub fn set(cx: &mut App, root: &str, notes: Vec<ReviewNote>) {
        cx.default_global::<ReviewNotes>();
        cx.update_global::<ReviewNotes, _>(|n, cx| {
            n.ensure_loaded();
            n.put(root.to_string(), notes);
            n.write(cx);
        });
    }
}

/// A new note's id: unique enough within a worktree's notes.
fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{nanos:x}-{:x}", N.fetch_add(1, Ordering::Relaxed))
}

impl ReviewNotes {
    /// Adds `note` to `root`'s, with the id it's given here.
    pub fn add(cx: &mut App, root: &str, mut note: ReviewNote) -> String {
        note.id = new_id();
        let id = note.id.clone();
        let mut notes = Self::get(cx, root);
        notes.push(note);
        Self::set(cx, root, notes);
        id
    }

    /// Changes the note `id` of `root`'s, if it's there.
    pub fn update(cx: &mut App, root: &str, id: &str, f: impl FnOnce(&mut ReviewNote)) {
        let mut notes = Self::get(cx, root);
        if let Some(n) = notes.iter_mut().find(|n| n.id == id) {
            f(n);
            Self::set(cx, root, notes);
        }
    }

    pub fn remove(cx: &mut App, root: &str, id: &str) {
        let mut notes = Self::get(cx, root);
        notes.retain(|n| n.id != id);
        Self::set(cx, root, notes);
    }

    pub fn clear_resolved(cx: &mut App, root: &str) {
        let mut notes = Self::get(cx, root);
        notes.retain(|n| !n.resolved);
        Self::set(cx, root, notes);
    }

    /// Open notes first, each file's in line order: as the list shows them.
    pub fn ordered(cx: &mut App, root: &str) -> Vec<ReviewNote> {
        let mut notes = Self::get(cx, root);
        notes.sort_by(|a, b| {
            a.resolved
                .cmp(&b.resolved)
                .then_with(|| a.path.cmp(&b.path))
                .then(a.anchor.start.cmp(&b.anchor.start))
        });
        notes
    }

    /// Keeps the notes on the files on disk up with them: each noted file read again, and its
    /// notes moved with their lines or marked outdated (and back, if the lines come back).
    pub fn check(cx: &mut App, root: &str) -> gpui_kit::Task<()> {
        let paths: Vec<String> = {
            let mut p: Vec<String> = Self::get(cx, root)
                .into_iter()
                .filter(|n| crate::model::notes::is_live(n) && !n.resolved)
                .map(|n| n.path)
                .collect();
            p.sort();
            p.dedup();
            p
        };
        if paths.is_empty() {
            return gpui_kit::Task::ready(());
        }
        let dir = PathBuf::from(root);
        let root = root.to_string();
        let read = cx.background_spawn(async move {
            paths
                .into_iter()
                .map(|p| {
                    let lines = std::fs::read_to_string(dir.join(&p)).ok().map(|t| {
                        t.split('\n')
                            .map(|l| l.trim_end_matches('\r').to_string())
                            .collect::<Vec<_>>()
                    });
                    (p, lines)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |cx| {
            let files = read.await;
            cx.update(|cx| {
                let mut notes = Self::get(cx, &root);
                let mut changed = false;
                // Files no live note is on any more give their lines back.
                let prefix = format!("{root}\0");
                let checked: std::collections::HashSet<String> =
                    files.iter().map(|(p, _)| format!("{prefix}{p}")).collect();
                cx.default_global::<Seen>()
                    .0
                    .retain(|k, _| !k.starts_with(&prefix) || checked.contains(k));
                for (path, lines) in files {
                    let key = format!("{root}\0{path}");
                    let prev = cx.default_global::<Seen>().0.remove(&key);
                    if let Some(next) = crate::model::notes::check_notes(
                        &notes,
                        &path,
                        lines.as_deref(),
                        prev.as_deref(),
                    ) {
                        notes = next;
                        changed = true;
                    }
                    if let Some(lines) = lines {
                        cx.default_global::<Seen>().0.insert(key, lines);
                    }
                }
                if changed {
                    Self::set(cx, &root, notes);
                }
            });
        })
    }

    /// `path`'s notes where typing took them, its text now saved as `lines` (`at`: each one's
    /// last line now, by id; model::notes::follow_notes).
    pub fn follow(
        cx: &mut App,
        root: &str,
        path: &str,
        lines: &[String],
        at: &std::collections::HashMap<String, usize>,
    ) {
        // The file as the last check read it is older than these places: the next goes by them.
        cx.default_global::<Seen>()
            .0
            .remove(&format!("{root}\0{path}"));
        let notes = Self::get(cx, root);
        if let Some(next) = crate::model::notes::follow_notes(&notes, path, lines, at) {
            Self::set(cx, root, next);
        }
    }
}

/// Each noted file's lines as last read, to tell how far edits above a note moved it.
#[derive(Default)]
struct Seen(std::collections::HashMap<String, Vec<String>>);

impl Global for Seen {}

/// On the first launch, the old app's notes, unless this build has a file of its own.
pub(crate) fn import_legacy(cx: &mut App) {
    import_from(cx, gitviber_ui::legacy::value);
}

fn import_from(cx: &mut App, value: impl Fn(&str) -> Option<Value>) {
    let Some(roots) = value("gitviber.reviewNotes")
        .map(read)
        .filter(|r| !r.is_empty())
    else {
        return;
    };
    if path().exists() {
        return;
    }
    cx.default_global::<ReviewNotes>();
    cx.update_global::<ReviewNotes, _>(|n, cx| {
        n.loaded = true;
        n.take(roots);
        n.write(cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui_kit::test]
    fn the_old_apps_notes_come_over_once_and_stay_bounded(cx: &mut gpui_kit::TestAppContext) {
        let dir = gitviber_ui::test_support::config_dir("review-notes");
        let note = serde_json::json!({ "id": "1", "path": "src/a.ts", "body": "Why?", "start": 2, "end": 3, "code": ["b", "c"], "before": ["a"], "after": [] });
        let old = serde_json::json!({
            "gitviber.reviewNotes": {
                "/repo": [note, { "id": "2", "path": "x", "body": "", "start": 5, "end": 1, "code": [] }, "junk"],
                "/empty": []
            }
        });
        cx.update(|cx| {
            import_from(cx, |key| old.get(key).cloned());
            let notes = ReviewNotes::get(cx, "/repo");
            assert_eq!(notes.len(), 1, "the one that's a note");
            assert_eq!((notes[0].body.as_str(), notes[0].anchor.start), ("Why?", 2));
            assert!(ReviewNotes::get(cx, "/empty").is_empty());
        });
        cx.run_until_parked();
        let kept = std::fs::read_to_string(dir.join("review-notes.json")).unwrap();
        assert!(kept.contains("Why?"));
        // Kept for the 30 worktrees noted on last.
        cx.update(|cx| {
            let one = ReviewNotes::get(cx, "/repo");
            for i in 0..31 {
                ReviewNotes::set(cx, &format!("/w{i}"), one.clone());
            }
            assert!(ReviewNotes::get(cx, "/repo").is_empty());
            assert!(ReviewNotes::get(cx, "/w0").is_empty());
            assert_eq!(ReviewNotes::get(cx, "/w30").len(), 1);
            // A file of its own now: the old app's aren't read again.
            import_from(cx, |key| old.get(key).cloned());
        });
        cx.run_until_parked();
        cx.update(|cx| assert!(ReviewNotes::get(cx, "/repo").is_empty()));
    }
}
