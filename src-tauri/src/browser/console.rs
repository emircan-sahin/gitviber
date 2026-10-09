//! What a page logs as errors and warnings (scripts/console.js), kept per view for the browser
//! tab's console badge and panel: the latest few hundred, with a mark where each load began.

use super::cut;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Entries a view keeps; older ones go.
const KEEP: usize = 500;
/// What console.js cuts a message and stack to, in characters, and the backend holds it to
/// whatever arrives.
const MAX_TEXT: usize = 2048;
/// A line console.js sends is three such texts and a little: past this it's no line of its, and
/// isn't parsed at all.
const MAX_LINE: usize = 3 * 4 * MAX_TEXT + 1024;

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Entry {
    /// "error", "warn", or "load" for the mark a new page left.
    pub level: String,
    pub msg: String,
    #[serde(default)]
    pub stack: String,
    pub url: String,
    /// Milliseconds since the epoch.
    pub ts: f64,
}

/// What the badge shows: since the panel was last cleared.
#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Counts {
    pub errors: u32,
    pub warnings: u32,
}

#[derive(Default)]
pub struct Log {
    entries: VecDeque<Entry>,
    counts: Counts,
}

impl Log {
    fn push(&mut self, entry: Entry) {
        if self.entries.len() == KEEP {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// One line of console.js's JSON; anything else (a page posting its own) is no entry.
    pub fn add(&mut self, json: &str) -> bool {
        if json.len() > MAX_LINE {
            return false;
        }
        let Ok(mut entry) = serde_json::from_str::<Entry>(json) else {
            return false;
        };
        match entry.level.as_str() {
            "error" => self.counts.errors = self.counts.errors.saturating_add(1),
            "warn" => self.counts.warnings = self.counts.warnings.saturating_add(1),
            _ => return false,
        }
        entry.msg = cut(&entry.msg, MAX_TEXT);
        entry.stack = cut(&entry.stack, MAX_TEXT);
        entry.url = cut(&entry.url, MAX_TEXT);
        self.push(entry);
        true
    }

    /// A new page loaded: what came before belongs to the last one.
    pub fn loaded(&mut self, url: &str, ts: f64) {
        self.push(Entry {
            level: "load".into(),
            msg: String::new(),
            stack: String::new(),
            url: cut(url, MAX_TEXT),
            ts,
        });
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.entries.iter().cloned().collect()
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: &str, msg: &str) -> String {
        serde_json::json!({ "level": level, "msg": msg, "stack": "at f (app.js:1:2)", "url": "http://localhost:5173/", "ts": 1.0 }).to_string()
    }

    #[test]
    fn errors_and_warnings_count_and_the_oldest_go_past_the_limit() {
        let mut log = Log::default();
        assert!(log.add(&line("error", "boom")));
        assert!(log.add(&line("warn", "careful")));
        log.loaded("http://localhost:5173/next", 2.0);
        assert_eq!(
            log.counts(),
            Counts {
                errors: 1,
                warnings: 1
            }
        );
        assert_eq!(
            log.entries()
                .iter()
                .map(|e| e.level.as_str())
                .collect::<Vec<_>>(),
            ["error", "warn", "load"]
        );
        for i in 0..KEEP {
            log.add(&line("error", &i.to_string()));
        }
        let entries = log.entries();
        assert_eq!(entries.len(), KEEP);
        assert_eq!(entries[0].msg, "0", "the first three went");
        assert_eq!(log.counts().errors, 1 + KEEP as u32);
        log.clear();
        assert!(log.entries().is_empty());
        assert_eq!(log.counts(), Counts::default());
    }

    #[test]
    fn what_isnt_console_js_is_no_entry_and_long_text_is_cut() {
        let mut log = Log::default();
        for no in [
            "",
            "nonsense",
            "{}",
            r#"{"level":"info","msg":"x","url":"u","ts":1}"#,
            r#"{"level":"load","msg":"","url":"u","ts":1}"#,
        ] {
            assert!(!log.add(no), "{no}");
        }
        // A missing stack is an empty one.
        assert!(log.add(r#"{"level":"error","msg":"x","url":"u","ts":1}"#));
        let long = "é".repeat(MAX_TEXT + 5);
        assert!(log.add(&line("warn", &long)));
        let msg = &log.entries()[1].msg;
        assert!(msg.chars().count() == MAX_TEXT && msg.chars().all(|c| c == 'é'));
        assert_eq!(
            log.counts(),
            Counts {
                errors: 1,
                warnings: 1
            }
        );
    }

    #[test]
    fn the_ring_keeps_load_marks_in_line_and_the_newest_five_hundred_of_anything() {
        let mut log = Log::default();
        for i in 0..2 * KEEP {
            if i % 7 == 0 {
                log.loaded(&format!("http://localhost:5173/{i}"), i as f64);
            } else {
                assert!(log.add(&line(
                    if i % 2 == 0 { "error" } else { "warn" },
                    &i.to_string()
                )));
            }
        }
        let entries = log.entries();
        assert_eq!(entries.len(), KEEP);
        // Oldest first, without a gap: each entry is the next number, by its message or its mark.
        let order: Vec<usize> = entries
            .iter()
            .map(|e| {
                let n = if e.level == "load" {
                    e.url.rsplit('/').next().unwrap()
                } else {
                    &e.msg
                };
                n.parse().unwrap()
            })
            .collect();
        assert!(order.windows(2).all(|w| w[1] == w[0] + 1), "{order:?}");
        assert_eq!(*order.last().unwrap(), 2 * KEEP - 1);
        let counted = log.counts();
        assert_eq!(
            counted.errors + counted.warnings,
            (2 * KEEP - (2 * KEEP).div_ceil(7)) as u32
        );
        // A mark's url is cut like a line's.
        log.loaded(&"x".repeat(5 * MAX_TEXT), 0.0);
        assert_eq!(log.entries().last().unwrap().url.chars().count(), MAX_TEXT);
    }

    #[test]
    fn whatever_a_page_posts_is_at_most_one_bounded_entry() {
        let mut log = Log::default();
        for no in [
            r#"{"level":"ERROR","msg":"x","url":"u","ts":1}"#,
            r#"{"level":"error","msg":1,"url":"u","ts":1}"#,
            r#"{"level":"error","msg":"x","url":"u","ts":"1"}"#,
            r#"{"level":"error","msg":"x","ts":1}"#,
            r#"{"level":"error","msg":"x","url":"u","ts":1e400}"#,
            r#"["error","x"]"#,
            "null",
            r#"{"level":"error","msg":"x","url":"u","ts":1"#,
        ] {
            assert!(!log.add(no), "{no}");
        }
        assert_eq!(log.counts(), Counts::default());
        // Extra fields are ignored; a page's own huge line is held to the limit, on a char edge.
        let emoji = "😀".repeat(MAX_TEXT);
        let big = serde_json::json!({ "level": "error", "msg": emoji, "stack": emoji, "url": emoji, "ts": 1.0, "extra": [1, 2] }).to_string();
        assert!(log.add(&big));
        let e = &log.entries()[0];
        for text in [&e.msg, &e.stack, &e.url] {
            assert!(text.chars().count() == MAX_TEXT && text.chars().all(|c| c == '😀'));
        }
        // Random bytes never panic.
        let mut seed: u64 = 3;
        for _ in 0..5000 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let n = (seed >> 58) as usize;
            let junk: String = (0..n)
                .map(|k| char::from(b"{}\":,levrmsgu1 \\"[((seed >> (k % 50)) as usize + k) % 16]))
                .collect();
            let _ = log.add(&junk);
        }
    }

    #[test]
    fn an_oversized_line_is_refused_unread_and_counts_never_wrap() {
        let mut log = Log::default();
        let huge = format!(
            r#"{{"level":"error","msg":"{}","url":"u","ts":1}}"#,
            "x".repeat(MAX_LINE)
        );
        assert!(!log.add(&huge));
        log.counts.errors = u32::MAX;
        assert!(log.add(&line("error", "one more")));
        assert_eq!(log.counts().errors, u32::MAX);
    }
}
