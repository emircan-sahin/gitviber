//! The review notes of each worktree (model::notes), kept in the app's config dir, never in the
//! repo: the 30 worktrees noted on most recently, as the old app kept them.

use std::collections::VecDeque;
use std::path::PathBuf;

use gpui_kit::{App, BorrowAppContext as _, Global};
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
        let dir =
            std::env::temp_dir().join(format!("gitviber-review-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // SAFETY: each test runs in its own process (nextest), before any thread reads it.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
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
